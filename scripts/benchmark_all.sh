#!/usr/bin/env zsh
set -euo pipefail

# Generate weights.rs files for all (or a single) pallet using the standard
# frame-benchmarking-cli --output / --template approach.
#
# Pallets are auto-discovered: any pallet with both benchmarking.rs and
# weights.rs is included. If a pallet is missing from define_benchmarks!
# in runtime/src/lib.rs, the benchmark CLI will error — no silent failures.
#
# Usage:
#   ./scripts/benchmark_all.sh                    # build + generate all
#   ./scripts/benchmark_all.sh pallet_subtensor   # build + generate one pallet
#   SKIP_BUILD=1 ./scripts/benchmark_all.sh       # skip cargo build

SCRIPT_DIR="$(cd "$(dirname "${0}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

RUNTIME_WASM="$ROOT_DIR/target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm"
NODE_BIN="$ROOT_DIR/target/production/node-subtensor"
TEMPLATE="$ROOT_DIR/.maintain/frame-weight-template.hbs"

STEPS="${STEPS:-50}"
REPEAT="${REPEAT:-20}"

die() { echo "ERROR: $1" >&2; exit 1; }

# ── Auto-discover pallets ────────────────────────────────────────────────────
typeset -A PALLET_OUTPUTS
while read -r name output_path; do
  PALLET_OUTPUTS[$name]="$output_path"
done < <("$SCRIPT_DIR/discover_pallets.sh")

(( ${#PALLET_OUTPUTS} > 0 )) || die "no benchmarked pallets found"

# ── Build ────────────────────────────────────────────────────────────────────
if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  echo "Building node-subtensor with runtime-benchmarks..."
  cargo build --profile production -p node-subtensor --features runtime-benchmarks
fi

[[ -x "$NODE_BIN" ]] || die "node binary not found at $NODE_BIN"
[[ -f "$RUNTIME_WASM" ]] || die "runtime WASM not found at $RUNTIME_WASM"
[[ -f "$TEMPLATE" ]] || die "weight template not found at $TEMPLATE"

# ── Determine which pallets to benchmark ─────────────────────────────────────
if [[ $# -gt 0 ]]; then
  PALLETS=("$@")
  for p in "${PALLETS[@]}"; do
    [[ -n "${PALLET_OUTPUTS[$p]:-}" ]] || die "unknown pallet: $p (available: ${(k)PALLET_OUTPUTS})"
  done
else
  PALLETS=("${(k)PALLET_OUTPUTS[@]}")
fi

# ── Benchmark loop ───────────────────────────────────────────────────────────
for pallet in "${PALLETS[@]}"; do
  output="${PALLET_OUTPUTS[$pallet]}"

  echo ""
  echo "════════════════════════════════════════════════════════"
  echo " Benchmarking $pallet -> $output"
  echo "════════════════════════════════════════════════════════"

  "$NODE_BIN" benchmark pallet \
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
    --output="$ROOT_DIR/$output" \
    --template="$TEMPLATE"

  echo "  -> wrote $output"
done

echo ""
echo "All done."
