#!/bin/bash
#
# Builds node-subtensor with --features fast-runtime, staging the binary at
# target/release-fast/node-subtensor so the prod build at target/release/
# stays untouched (and the upgrade test keeps working against it).
#
# The fast-runtime build uses a dedicated CARGO_TARGET_DIR to avoid
# invalidating the prod build's incremental cache.
#
set -euo pipefail

cd "$(dirname "$0")/.."
TS_TESTS_DIR="$(pwd)"
REPO_ROOT="$(cd .. && pwd)"

OUTPUT_BIN="$REPO_ROOT/target/release-fast/node-subtensor"
FAST_TARGET_DIR="$TS_TESTS_DIR/tmp/cargo-target-fast"
BUILT_BIN="$FAST_TARGET_DIR/release/node-subtensor"

# Skip if the staged binary is newer than every source file we care about.
# The set of paths mirrors what `cargo build -p node-subtensor` actually
# depends on; widen it if a future change moves source under a new prefix.
if [ -x "$OUTPUT_BIN" ]; then
    newer=$(find \
        "$REPO_ROOT/runtime" \
        "$REPO_ROOT/common" \
        "$REPO_ROOT/pallets" \
        "$REPO_ROOT/node" \
        "$REPO_ROOT/primitives" \
        -name '*.rs' -newer "$OUTPUT_BIN" -print -quit 2>/dev/null || true)
    if [ -z "$newer" ]; then
        echo "==> $OUTPUT_BIN up-to-date, skipping fast-runtime build."
        exit 0
    fi
fi

echo "==> Building node-subtensor with --features fast-runtime"
echo "    (CARGO_TARGET_DIR=$FAST_TARGET_DIR; first build is slow)"
(
    cd "$REPO_ROOT"
    CARGO_TARGET_DIR="$FAST_TARGET_DIR" \
        cargo build --release --features fast-runtime -p node-subtensor
)

if [ ! -x "$BUILT_BIN" ]; then
    echo "ERROR: expected binary not found at $BUILT_BIN" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT_BIN")"
cp "$BUILT_BIN" "$OUTPUT_BIN"
echo "==> Wrote $OUTPUT_BIN (fast-runtime)"
