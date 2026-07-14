#!/bin/bash
#
# Builds a runtime WASM with spec_version bumped by +1
#
set -euo pipefail

cd "$(dirname "$0")/.."
TS_TESTS_DIR="$(pwd)"
REPO_ROOT="$(cd .. && pwd)"

LIB_RS="$REPO_ROOT/runtime/src/lib.rs"
RUNTIME_TOML="$REPO_ROOT/runtime/Cargo.toml"
OUTPUT_DIR="$TS_TESTS_DIR/tmp"
OUTPUT_WASM="$OUTPUT_DIR/upgraded-runtime.wasm"
UPGRADE_TARGET_DIR="$OUTPUT_DIR/cargo-target"
BUILT_WASM="$UPGRADE_TARGET_DIR/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm"

mkdir -p "$OUTPUT_DIR"

# Skip if existing output is newer than every input source.
if [ -f "$OUTPUT_WASM" ] \
   && [ "$OUTPUT_WASM" -nt "$LIB_RS" ] \
   && [ "$OUTPUT_WASM" -nt "$RUNTIME_TOML" ]; then
    echo "==> Upgraded runtime already up-to-date at $OUTPUT_WASM, skipping build."
    exit 0
fi

# Read current spec_version from source.
CURRENT_VERSION=$(grep -E '^\s*spec_version:' "$LIB_RS" | head -1 | grep -oE '[0-9]+')
if [ -z "$CURRENT_VERSION" ]; then
    echo "ERROR: failed to parse spec_version from $LIB_RS" >&2
    exit 1
fi
NEW_VERSION=$((CURRENT_VERSION + 1))
echo "==> Bumping spec_version: $CURRENT_VERSION -> $NEW_VERSION (transient, will be restored)"

# Backup + always-restore guard.
BACKUP="$LIB_RS.upgrade-build-backup"
cp "$LIB_RS" "$BACKUP"
trap 'mv "$BACKUP" "$LIB_RS"' EXIT

# In-place bump (BSD/macOS sed friendly: -i with empty suffix arg).
sed -i.tmp -E "s/^([[:space:]]*spec_version:[[:space:]]*)[0-9]+,/\1${NEW_VERSION},/" "$LIB_RS"
rm -f "$LIB_RS.tmp"

echo "==> Building runtime crate (CARGO_TARGET_DIR=$UPGRADE_TARGET_DIR)"
echo "    First build is slow (cold deps); subsequent runs are incremental."
(
    cd "$REPO_ROOT"
    CARGO_TARGET_DIR="$UPGRADE_TARGET_DIR" \
        cargo build --profile release -p node-subtensor-runtime
)

if [ ! -f "$BUILT_WASM" ]; then
    echo "ERROR: expected WASM not found at $BUILT_WASM" >&2
    exit 1
fi

cp "$BUILT_WASM" "$OUTPUT_WASM"
echo "==> Wrote $OUTPUT_WASM (spec_version=$NEW_VERSION)"
