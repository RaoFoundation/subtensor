#!/usr/bin/env zsh
set -euo pipefail

# Generate weights.rs files for all (or a single) benchmark target using the standard
# frame-benchmarking-cli --output / --template approach.
#
# Targets are auto-discovered: pallets with both benchmarking.rs and
# weights.rs are included, plus runtime-owned targets listed by
# scripts/discover_pallets.sh. If a target is missing from define_benchmarks!
# in runtime/src/lib.rs, the benchmark CLI will error — no silent failures.
#
# Usage:
#   ./scripts/benchmark_all.sh                    # build + generate all
#   ./scripts/benchmark_all.sh pallet_subtensor   # build + generate one target
#   ./scripts/benchmark_all.sh governance         # build + generate governance weights
#   SKIP_BUILD=1 ./scripts/benchmark_all.sh       # skip cargo build

SCRIPT_DIR="$(cd "$(dirname "${0}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

export PATH="$HOME/.cargo/bin:$PATH"

RUNTIME_WASM="$ROOT_DIR/target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm"
NODE_BIN="$ROOT_DIR/target/production/node-subtensor"
TEMPLATE="$ROOT_DIR/.maintain/frame-weight-template.hbs"

STEPS="${STEPS:-50}"
REPEAT="${REPEAT:-20}"

die() { echo "ERROR: $1" >&2; exit 1; }

# ── Auto-discover benchmark targets ──────────────────────────────────────────
typeset -A PALLET_OUTPUTS
while read -r name output_path; do
  PALLET_OUTPUTS[$name]="$output_path"
done < <("$SCRIPT_DIR/discover_pallets.sh")

(( ${#PALLET_OUTPUTS} > 0 )) || die "no benchmark targets found"

# ── Build ────────────────────────────────────────────────────────────────────
if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  echo "Building node-subtensor with runtime-benchmarks..."
  cargo build --profile production -p node-subtensor --features runtime-benchmarks
fi

[[ -x "$NODE_BIN" ]] || die "node binary not found at $NODE_BIN"
[[ -f "$RUNTIME_WASM" ]] || die "runtime WASM not found at $RUNTIME_WASM"
[[ -f "$TEMPLATE" ]] || die "weight template not found at $TEMPLATE"

# ── Determine which targets to benchmark ─────────────────────────────────────
if [[ $# -gt 0 ]]; then
  PALLETS=("$@")
  for p in "${PALLETS[@]}"; do
    [[ -n "${PALLET_OUTPUTS[$p]:-}" ]] || die "unknown benchmark target: $p (available: ${(k)PALLET_OUTPUTS})"
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
