#!/usr/bin/env bash
set -euo pipefail

# CI benchmark validation: generate weights, compare with threshold, prepare patch if drifted.
# Exit: 0 = ok, 1 = error, 2 = drift (patch in .bench_patch/)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

NODE_BIN="$ROOT_DIR/target/production/node-subtensor"
RUNTIME_WASM="$ROOT_DIR/target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm"
TEMPLATE="$ROOT_DIR/.maintain/frame-weight-template.hbs"
WEIGHT_CMP="$ROOT_DIR/target/production/weight-compare"

PATCH_DIR="$ROOT_DIR/.bench_patch"
THRESHOLD="${THRESHOLD:-75}"
STEPS="${STEPS:-50}"
REPEAT="${REPEAT:-20}"
EXTRINSICS="${EXTRINSICS:-*}"
# Manual feature-branch reference runs may carry an explicit focus manifest.
# Scheduled/default-branch and PR-label runs retain the full-suite behavior.
if [[ "${GITHUB_EVENT_NAME:-}" == "workflow_dispatch" &&
      "${GITHUB_REF_NAME:-}" != "${GITHUB_DEFAULT_BRANCH:-main}" &&
      -f "$ROOT_DIR/.maintain/benchmark-focus.json" ]]; then
  PALLET_DIRS="${PALLET_DIRS:-$(python3 -c 'import json,sys; print(" ".join(json.load(open(sys.argv[1]))["pallet_dirs"]))' "$ROOT_DIR/.maintain/benchmark-focus.json")}"
  EXTRINSICS=$(python3 -c 'import json,sys; print(",".join(json.load(open(sys.argv[1]))["extrinsics"]))' "$ROOT_DIR/.maintain/benchmark-focus.json")
  echo "Manual branch reference focus: $PALLET_DIRS / $EXTRINSICS"
fi
# Utility batches are low-amplitude microbenchmarks; median-slopes avoids noisy intercept drift.
UTILITY_OUTPUT_ANALYSIS="${UTILITY_OUTPUT_ANALYSIS:-median-slopes}"


die() { echo "ERROR: $1" >&2; exit 1; }

selective_patch_weights_file() {
    local committed="$1"
    local generated="$2"
    shift 2

    python3 "$SCRIPT_DIR/benchmark_action.py" selective-patch "$committed" "$generated" "$@"
}

# ── Auto-discover pallets ────────────────────────────────────────────────────
declare -A OUTPUTS
while read -r name path; do
  OUTPUTS[$name]="$path"
done < <("$SCRIPT_DIR/discover_pallets.sh")

(( ${#OUTPUTS[@]} > 0 )) || die "no benchmarked pallets found"

# PALLET_DIRS (space-separated directory names under pallets/, set by CI for
# pallet-only diffs) restricts the run to the changed pallets. Unset/empty
# means the full suite.
if [[ -n "${PALLET_DIRS:-}" ]]; then
  declare -A KEEP
  for dir in $PALLET_DIRS; do
    for pallet in "${!OUTPUTS[@]}"; do
      [[ "${OUTPUTS[$pallet]}" == "pallets/$dir/"* ]] && KEEP[$pallet]="${OUTPUTS[$pallet]}"
    done
  done
  if (( ${#KEEP[@]} == 0 )); then
    echo "Changed pallets ($PALLET_DIRS) have no registered benchmarks; nothing to validate."
    exit 0
  fi
  echo "Restricting to changed pallets: ${!KEEP[*]}"
  unset OUTPUTS
  declare -A OUTPUTS
  for pallet in "${!KEEP[@]}"; do OUTPUTS[$pallet]="${KEEP[$pallet]}"; done
fi

mkdir -p "$PATCH_DIR"

# Build if needed
[[ -x "$NODE_BIN" ]] || cargo build --profile production -p node-subtensor --features runtime-benchmarks
cargo build --profile production -p subtensor-weight-tools --bin weight-compare
[[ -x "$NODE_BIN" ]] || die "node binary not found"
[[ -f "$RUNTIME_WASM" ]] || die "runtime WASM not found"
[[ -x "$WEIGHT_CMP" ]] || die "weight-compare not found"

PATCHED=()
SUMMARY=()
FAILED=0

for pallet in "${!OUTPUTS[@]}"; do
  output="${OUTPUTS[$pallet]}"
  committed="$ROOT_DIR/$output"
  tmp=$(mktemp)

  echo ""
  echo "════ $pallet ════"

  analysis_args=()
  if [[ -n "${OUTPUT_ANALYSIS:-}" ]]; then
    analysis_args+=(--output-analysis="$OUTPUT_ANALYSIS")
  elif [[ "$pallet" == "pallet_subtensor_utility" ]]; then
    analysis_args+=(--output-analysis="$UTILITY_OUTPUT_ANALYSIS")
  fi

  if ! "$NODE_BIN" benchmark pallet \
    --runtime="$RUNTIME_WASM" \
    --genesis-builder=runtime \
    --genesis-builder-preset=benchmark \
    --wasm-execution=compiled \
    --pallet="$pallet" \
    --extrinsic="$EXTRINSICS" \
    --steps="$STEPS" \
    --repeat="$REPEAT" \
    --no-storage-info \
    --no-min-squares \
    --no-median-slopes \
    "${analysis_args[@]}" \
    --output="$tmp" \
    --template="$TEMPLATE" 2>&1; then
    SUMMARY+=("$pallet: FAILED"); FAILED=1; rm -f "$tmp"; continue
  fi

  # A filtered reference run measures only requested methods. Merge those
  # methods into a temporary complete file before comparison so unrelated
  # methods are neither reported as removed nor included in the patch.
  if [[ "$EXTRINSICS" != "*" && -f "$committed" ]]; then
    merged=$(mktemp)
    cp "$committed" "$merged"
    measured_methods=()
    while IFS= read -r method; do measured_methods+=("$method"); done < <(
      python3 - "$tmp" <<'PY_METHODS'
import re
import sys
with open(sys.argv[1], encoding="utf-8") as source:
    print("\n".join(dict.fromkeys(re.findall(r"(?m)^\s*fn\s+(\w+)\(", source.read()))))
PY_METHODS
    )
    (( ${#measured_methods[@]} > 0 )) || die "filtered benchmark output has no methods"
    selective_patch_weights_file "$merged" "$tmp" "${measured_methods[@]}"
    mv "$merged" "$tmp"
  fi

  if [[ ! -f "$committed" ]]; then
    cp "$tmp" "$committed"; PATCHED+=("$output"); SUMMARY+=("$pallet: NEW")
  else
    compare_log=$(mktemp)
    if "$WEIGHT_CMP" --old "$committed" --new "$tmp" --threshold "$THRESHOLD" 2>&1 | tee "$compare_log"; then
        rc=0
    else
        rc=${PIPESTATUS[0]}
    fi

    if (( rc == 2 )); then
        drifted_benchmarks=()
        while IFS= read -r benchmark_name; do
            drifted_benchmarks+=("$benchmark_name")
        done < <(python3 "$SCRIPT_DIR/benchmark_action.py" drifted-benchmarks "$compare_log")

        if (( ${#drifted_benchmarks[@]} == 0 )); then
            SUMMARY+=("$pallet: COMPARE FAILED"); FAILED=1
        else
            selective_patch_weights_file "$committed" "$tmp" "${drifted_benchmarks[@]}"
            PATCHED+=("$output")
            SUMMARY+=("$pallet: UPDATED ${#drifted_benchmarks[@]} benchmark(s): ${drifted_benchmarks[*]}")
        fi
    elif (( rc == 0 )); then
        SUMMARY+=("$pallet: OK")
    else
        SUMMARY+=("$pallet: COMPARE FAILED"); FAILED=1
    fi
    rm -f "$compare_log"
  fi
  rm -f "$tmp"
done

echo ""; printf '%s\n' "${SUMMARY[@]}"

# Preserve successful reference measurements even if another pallet fails.
# The run remains red; a partial patch is evidence, not a passing gate.
if (( ${#PATCHED[@]} == 0 )); then
  printf '%s\n' "${SUMMARY[@]}" > "$PATCH_DIR/summary.txt"
  (( FAILED )) && exit 1
  echo "All weights within tolerance."
  exit 0
fi

# Prepare patch
cd "$ROOT_DIR"
git add "${PATCHED[@]}"
{ echo "Head SHA: $(git rev-parse HEAD)"; echo ""; printf '%s\n' "${SUMMARY[@]}"; echo ""; git diff --cached --stat; } > "$PATCH_DIR/summary.txt"
git diff --cached --binary > "$PATCH_DIR/benchmark_patch.diff"
git reset HEAD -- "${PATCHED[@]}" >/dev/null 2>&1 || true
if (( FAILED )); then
  echo "Partial reference patch saved at $PATCH_DIR/benchmark_patch.diff; failed pallets remain unmeasured."
  exit 1
fi
echo "Patch ready at $PATCH_DIR/benchmark_patch.diff — add 'apply-benchmark-patch' label to apply."
exit 2
