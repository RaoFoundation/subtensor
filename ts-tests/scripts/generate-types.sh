#!/usr/bin/env bash
#
# (Re)generate polkadot-api type descriptors from a chain spec runtime or,
# when no chain spec is supplied, from a temporary development node.
# Checks that the node binary exists before running either path.
# Generates types only if they are missing or empty.
#
# Usage:
#   ./generate-types.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/.."

BASE_DIR="./tmp"
mkdir -p "$BASE_DIR"

BINARY="${BINARY_PATH:-../target/release/node-subtensor}"
NODE_LOG="${BASE_DIR}/node.log"
RUNTIME_WASM="${BASE_DIR}/runtime.compact.compressed.wasm"

verify_generated_types() {
  if [ ! -s "./.papi/metadata/subtensor.scale" ] || \
    [ ! -s "./.papi/descriptors/dist/index.mjs" ] || \
    [ ! -e "./node_modules/@polkadot-api/descriptors" ]; then
    echo "ERROR: polkadot-api did not finish installing the generated descriptors"
    exit 1
  fi
}

if [ ! -f "$BINARY" ]; then
  echo "ERROR: Node binary not found at $BINARY"
  echo "Please build it first, e.g.: cargo build --release -p node-subtensor"
  exit 1
fi

DESCRIPTORS_DIR="./.papi/descriptors"
GENERATE_TYPES=false
if [ ! -d "$DESCRIPTORS_DIR" ] || [ -z "$(ls -A "$DESCRIPTORS_DIR" 2>/dev/null)" ]; then
  echo "==> Type descriptors not found or empty, will generate..."
  GENERATE_TYPES=true
else
  echo "==> Type descriptors already exist, skipping generation."
fi

if [ "$GENERATE_TYPES" = true ]; then
  if [ -n "${PAPI_CHAIN_SPEC_PATH:-}" ]; then
    if [ ! -f "$PAPI_CHAIN_SPEC_PATH" ]; then
      echo "ERROR: Chain spec not found at $PAPI_CHAIN_SPEC_PATH"
      exit 1
    fi

    echo "==> Extracting metadata from the chain spec runtime..."
    node ./scripts/extract-runtime-wasm.mjs "$PAPI_CHAIN_SPEC_PATH" "$RUNTIME_WASM"
    pnpm exec polkadot-api add subtensor --wasm "$RUNTIME_WASM" --skip-codegen
    pnpm exec polkadot-api
    verify_generated_types
    echo "==> Done generating types from the chain spec runtime."
    exit 0
  fi

  echo "==> Starting dev node (logs at $NODE_LOG)..."
  "$BINARY" --one --dev &>"$NODE_LOG" &
  NODE_PID=$!
  cleanup_node() {
    status=$?
    trap - EXIT
    kill "$NODE_PID" 2>/dev/null || true
    wait "$NODE_PID" 2>/dev/null || true
    exit "$status"
  }
  trap cleanup_node EXIT

  TIMEOUT=60
  ELAPSED=0
  echo "==> Waiting for node to be ready (timeout: ${TIMEOUT}s)..."
  until curl -sf -o /dev/null \
    -H "Content-Type: application/json" \
    -d '{"id":1,"jsonrpc":"2.0","method":"system_health","params":[]}' \
    http://localhost:9944; do
    sleep 1
    ELAPSED=$((ELAPSED + 1))
    if [ "$ELAPSED" -ge "$TIMEOUT" ]; then
      echo "ERROR: Node failed to start within ${TIMEOUT}s. Check $NODE_LOG"
      exit 1
    fi
  done

  echo "==> Generating papi types..."
  pnpm generate-types
  verify_generated_types

  echo "==> Done generating types."
  exit 0
else
  echo "==> Types are up-to-date, nothing to do."
fi
