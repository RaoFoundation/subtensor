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
THRESHOLD="${THRESHOLD:-40}"
STEPS="${STEPS:-50}"
REPEAT="${REPEAT:-20}"

die() { echo "ERROR: $1" >&2; exit 1; }

# ── Auto-discover benchmark targets ──────────────────────────────────────────
declare -A OUTPUTS
while read -r name path; do
  OUTPUTS[$name]="$path"
done < <("$SCRIPT_DIR/discover_pallets.sh")

(( ${#OUTPUTS[@]} > 0 )) || die "no benchmark targets found"

mkdir -p "$PATCH_DIR"

# Build if needed
[[ -x "$NODE_BIN" ]] || cargo build --profile production -p node-subtensor --features runtime-benchmarks
[[ -x "$WEIGHT_CMP" ]] || cargo build --profile production -p subtensor-weight-tools --bin weight-compare
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

  if ! "$NODE_BIN" benchmark pallet \
    --runtime="$RUNTIME_WASM" \
    --genesis-builder=runtime \
    --genesis-builder-preset=benchmark \
    --wasm-execution=compiled \
    --pallet="$pallet" \
    --extrinsic="*" \
    --steps="$STEPS" \
    --repeat="$REPEAT" \
    --no-storage-info \
    --no-min-squares \
    --no-median-slopes \
    --output="$tmp" \
    --template="$TEMPLATE" 2>&1; then
    SUMMARY+=("$pallet: FAILED"); FAILED=1; rm -f "$tmp"; continue
  fi

  if [[ ! -f "$committed" ]]; then
    cp "$tmp" "$committed"; PATCHED+=("$output"); SUMMARY+=("$pallet: NEW")
  else
    rc=0; "$WEIGHT_CMP" --old "$committed" --new "$tmp" --threshold "$THRESHOLD" || rc=$?
    if (( rc == 2 )); then
      cp "$tmp" "$committed"; PATCHED+=("$output"); SUMMARY+=("$pallet: UPDATED")
    elif (( rc == 0 )); then
      SUMMARY+=("$pallet: OK")
    else
      SUMMARY+=("$pallet: COMPARE FAILED"); FAILED=1
    fi
  fi
  rm -f "$tmp"
done

echo ""; printf '%s\n' "${SUMMARY[@]}"

(( FAILED )) && { printf '%s\n' "${SUMMARY[@]}" > "$PATCH_DIR/summary.txt"; exit 1; }
(( ${#PATCHED[@]} == 0 )) && { echo "All weights within tolerance."; exit 0; }

# Prepare patch
cd "$ROOT_DIR"
git add "${PATCHED[@]}"
{ echo "Head SHA: $(git rev-parse HEAD)"; echo ""; printf '%s\n' "${SUMMARY[@]}"; echo ""; git diff --cached --stat; } > "$PATCH_DIR/summary.txt"
git diff --cached --binary > "$PATCH_DIR/benchmark_patch.diff"
git reset HEAD -- "${PATCHED[@]}" >/dev/null 2>&1 || true
echo "Patch ready at $PATCH_DIR/benchmark_patch.diff — add 'apply-benchmark-patch' label to apply."
exit 2
