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
# Utility batches are low-amplitude microbenchmarks; median-slopes avoids noisy intercept drift.
UTILITY_OUTPUT_ANALYSIS="${UTILITY_OUTPUT_ANALYSIS:-median-slopes}"


die() { echo "ERROR: $1" >&2; exit 1; }

selective_patch_weights_file() {
    local committed="$1"
    local generated="$2"
    shift 2

    python3 "$SCRIPT_DIR/benchmark_action.py" selective-patch "$committed" "$generated" "$@"
}

# Validate every selector before building. Empty inputs always mean all;
# branch-local manifests do not silently change the requested scope.
[[ $# -eq 0 || ( $# -eq 1 && "$1" == "--check-selection" ) ]] || die "usage: $0 [--check-selection]"
selection=$("$SCRIPT_DIR/discover_pallets.sh" | python3 "$SCRIPT_DIR/benchmark_action.py" select-pallets)
if [[ "${1:-}" == "--check-selection" ]]; then
  printf '%s\n' "$selection"
  exit 0
fi

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT
printf '%s\n' "$selection" > "$work_dir/pallets.tsv"

mkdir -p "$PATCH_DIR/generated"
rm -f "$PATCH_DIR/benchmark_patch.diff" "$PATCH_DIR/summary.txt" "$PATCH_DIR/selection.tsv"
rm -f "$PATCH_DIR/generated/"*.rs "$PATCH_DIR/generated/"*.rs.tmp

# Build if needed
[[ -x "$NODE_BIN" ]] || cargo build --profile production -p node-subtensor --features runtime-benchmarks
cargo build --profile production -p subtensor-weight-tools --bin weight-compare
[[ -x "$NODE_BIN" ]] || die "node binary not found"
[[ -f "$RUNTIME_WASM" ]] || die "runtime WASM not found"
[[ -x "$WEIGHT_CMP" ]] || die "weight-compare not found"

# Listing reads the built runtime's benchmark metadata without measuring.
# Resolve the whole request first: a valid name must not conceal a typo, and
# a list spanning different pallets must only run each pallet's own methods.
"$NODE_BIN" benchmark pallet \
  --runtime="$RUNTIME_WASM" \
  --genesis-builder=runtime \
  --genesis-builder-preset=benchmark \
  --wasm-execution=compiled \
  --pallet="*" --extrinsic="*" \
  --list=all --no-csv-header > "$work_dir/available.csv"
python3 "$SCRIPT_DIR/benchmark_action.py" select-benchmarks \
  "$work_dir/pallets.tsv" "$work_dir/available.csv" > "$work_dir/selection.tsv"
cp "$work_dir/selection.tsv" "$PATCH_DIR/selection.tsv"

declare -A OUTPUTS=() BENCHMARKS=()
PALLETS=()
while IFS=$'\t' read -r pallet output extrinsics; do
  PALLETS+=("$pallet")
  OUTPUTS[$pallet]="$output"
  BENCHMARKS[$pallet]="$extrinsics"
done < "$work_dir/selection.tsv"
(( ${#PALLETS[@]} > 0 )) || die "no matching benchmarks"

echo "Selected benchmark scope:"
for pallet in "${PALLETS[@]}"; do
  echo "  $pallet: ${BENCHMARKS[$pallet]}"
done

PATCHED=()
SUMMARY=()
FAILED=0

# Stable ordering keeps the small hashed-account pallet ahead of Subtensor's
# long-running suite. Save each completed pallet before starting the next one.
HEAD_SHA=$(git -C "$ROOT_DIR" rev-parse HEAD)

save_progress() {
  local status="$1"
  if (( ${#PATCHED[@]} > 0 )); then
    # Use a private index so checkpoints neither include nor reset staged work.
    local patch_index
    patch_index=$(mktemp)
    rm -f "$patch_index"
    GIT_INDEX_FILE="$patch_index" git -C "$ROOT_DIR" read-tree HEAD
    GIT_INDEX_FILE="$patch_index" git -C "$ROOT_DIR" add -- "${PATCHED[@]}"
    GIT_INDEX_FILE="$patch_index" git -C "$ROOT_DIR" diff --cached --binary HEAD -- "${PATCHED[@]}" > "$PATCH_DIR/benchmark_patch.diff.tmp"
    rm -f "$patch_index"
    mv "$PATCH_DIR/benchmark_patch.diff.tmp" "$PATCH_DIR/benchmark_patch.diff"
  else
    rm -f "$PATCH_DIR/benchmark_patch.diff"
  fi
  {
    echo "Head SHA: $HEAD_SHA"
    echo "Status: $status"
    echo "Completed pallets: ${#SUMMARY[@]}/${#PALLETS[@]}"
    echo "Selected scope (completion covers only this selection):"
    for selected_pallet in "${PALLETS[@]}"; do
      echo "  $selected_pallet: ${BENCHMARKS[$selected_pallet]}"
    done
    printf '%s\n' "${SUMMARY[@]}"
  } > "$PATCH_DIR/summary.txt.tmp"
  mv "$PATCH_DIR/summary.txt.tmp" "$PATCH_DIR/summary.txt"
}

save_progress INCOMPLETE
for pallet in "${PALLETS[@]}"; do
  output="${OUTPUTS[$pallet]}"
  committed="$ROOT_DIR/$output"
  extrinsics="${BENCHMARKS[$pallet]}"
  tmp="$work_dir/$pallet.rs"

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
    --extrinsic="$extrinsics" \
    --steps="$STEPS" \
    --repeat="$REPEAT" \
    --no-storage-info \
    --no-min-squares \
    --no-median-slopes \
    "${analysis_args[@]}" \
    --output="$tmp" \
    --template="$TEMPLATE" 2>&1; then
    SUMMARY+=("$pallet: FAILED"); FAILED=1; rm -f "$tmp"
    save_progress INCOMPLETE
    continue
  fi

  # Retain the complete reference output (including per-byte coefficients),
  # even if comparison fails or its drift falls below the patch threshold.
  cp "$tmp" "$PATCH_DIR/generated/$pallet.rs.tmp"
  mv "$PATCH_DIR/generated/$pallet.rs.tmp" "$PATCH_DIR/generated/$pallet.rs"

  # A filtered reference run measures only requested methods. Merge those
  # methods into a temporary complete file before comparison so unrelated
  # methods are neither reported as removed nor included in the patch.
  if [[ "$extrinsics" != "*" ]]; then
    [[ -f "$committed" ]] || die "filtered benchmarks require an existing complete weight file: $output"
    merged="$work_dir/merged.rs"
    cp "$committed" "$merged"
    if ! python3 "$SCRIPT_DIR/benchmark_action.py" merge-measured "$merged" "$tmp" "$extrinsics"; then
      SUMMARY+=("$pallet: INVALID FILTERED OUTPUT"); FAILED=1
      save_progress INCOMPLETE
      continue
    fi
    mv "$merged" "$tmp"
  fi

  if [[ ! -f "$committed" ]]; then
    cp "$tmp" "$committed"; PATCHED+=("$output"); SUMMARY+=("$pallet: NEW")
  else
    compare_log="$work_dir/compare.log"
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
  save_progress INCOMPLETE
done

echo ""; printf '%s\n' "${SUMMARY[@]}"

# Preserve successful reference measurements even if another pallet fails.
# The run remains red; a partial patch is evidence, not a passing gate.
if (( FAILED )); then
  save_progress FAILED
  echo "Completed reference results saved in $PATCH_DIR; failed pallets remain unmeasured."
  exit 1
fi
save_progress COMPLETE
if (( ${#PATCHED[@]} == 0 )); then
  echo "All selected weights within tolerance."
  exit 0
fi
echo "Patch ready at $PATCH_DIR/benchmark_patch.diff (download bench-patch for manual runs; apply-benchmark-patch label for PR-label runs)."
exit 2
