#!/bin/bash

set -euo pipefail

# If binaries are compiled in CI then skip this script
if [ -n "${BUILT_IN_CI:-}" ]; then
  echo "[*] BUILT_IN_CI is set to '$BUILT_IN_CI'. Skipping script..."
  exit 0
fi

# Check if `--no-purge` passed as a parameter
NO_PURGE=0

# Check if `--build-only` passed as parameter
BUILD_ONLY=0

for arg in "$@"; do
  if [ "$arg" = "--no-purge" ]; then
    NO_PURGE=1
  elif [ "$arg" = "--build-only" ]; then
    BUILD_ONLY=1
  fi
done

# Determine the directory this script resides in. This allows invoking it from any location.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"

# The base directory of the subtensor project
BASE_DIR="$SCRIPT_DIR/.."

# Get the value of fast_runtime from the first argument
fast_runtime=${1:-"True"}

# Define the target directory for compilation
if [ "$fast_runtime" == "False" ]; then
  # Block of code to execute if fast_runtime is False
  echo "fast_runtime is Off"
  : "${CHAIN:=local}"
  : "${BUILD_BINARY:=1}"
  : "${FEATURES:="pow-faucet metadata-hash"}"
  BUILD_DIR="$BASE_DIR/target/non-fast-runtime"
else
  # Block of code to execute if fast_runtime is not False
  echo "fast_runtime is On"
  : "${CHAIN:=local}"
  : "${BUILD_BINARY:=1}"
  : "${FEATURES:="pow-faucet metadata-hash fast-runtime"}"
  BUILD_DIR="$BASE_DIR/target/fast-runtime"
fi

# Ensure the build directory exists
mkdir -p "$BUILD_DIR"

# Cross-compiled builds land in a target-triple subdirectory.
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  NODE_BINARY="$BUILD_DIR/$CARGO_BUILD_TARGET/release/node-subtensor"
else
  NODE_BINARY="$BUILD_DIR/release/node-subtensor"
fi

SPEC_PATH="${SCRIPT_DIR}/specs/"
FULL_PATH="$SPEC_PATH$CHAIN.json"

# Kill any existing nodes which may have not exited correctly after a previous run.
pkill -9 'node-subtensor' || true

if [ ! -d "$SPEC_PATH" ]; then
  echo "*** Creating directory ${SPEC_PATH}..."
  mkdir -p "$SPEC_PATH"
fi

if [[ "$BUILD_BINARY" == "1" ]]; then
  echo "*** Building substrate binary..."

  BUILD_CMD=(
    cargo build
    --workspace
    --profile=release
    --features "$FEATURES"
    --manifest-path "$BASE_DIR/Cargo.toml"
  )

  if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    echo "[+] Cross-compiling for target: $CARGO_BUILD_TARGET"
    BUILD_CMD+=(--target "$CARGO_BUILD_TARGET")
  else
    echo "[+] Building for host architecture"
  fi

  CARGO_TARGET_DIR="$BUILD_DIR" "${BUILD_CMD[@]}"
  echo "*** Binary compiled"
fi

echo "*** Building chainspec..."
"$NODE_BINARY" build-spec --disable-default-bootnode --raw --chain "$CHAIN" >"$FULL_PATH"
echo "*** Chainspec built and output to file"

# Generate node keys (exits non-zero when a key already exists — that's fine)
"$NODE_BINARY" key generate-node-key --chain="$FULL_PATH" --base-path /tmp/one || true
"$NODE_BINARY" key generate-node-key --chain="$FULL_PATH" --base-path /tmp/two || true
"$NODE_BINARY" key generate-node-key --chain="$FULL_PATH" --base-path /tmp/three || true

if [ $NO_PURGE -eq 1 ]; then
  echo "*** Purging previous state skipped..."
else
  echo "*** Purging previous state..."
  "$NODE_BINARY" purge-chain -y --base-path /tmp/two --chain="$FULL_PATH" >/dev/null 2>&1
  "$NODE_BINARY" purge-chain -y --base-path /tmp/one --chain="$FULL_PATH" >/dev/null 2>&1
  "$NODE_BINARY" purge-chain -y --base-path /tmp/three --chain="$FULL_PATH" >/dev/null 2>&1
  echo "*** Previous chainstate purged"
fi

if [ $BUILD_ONLY -eq 0 ]; then
  echo "*** Starting localnet nodes..."

  one_start=(
    "$NODE_BINARY"
    --base-path /tmp/one
    --chain="$FULL_PATH"
    --one
    --port 30334
    --rpc-port 9944
    --validator
    --rpc-cors=all
    --allow-private-ipv4
    --discover-local
    --unsafe-force-node-key-generation
  )

  two_start=(
    "$NODE_BINARY"
    --base-path /tmp/two
    --chain="$FULL_PATH"
    --two
    --port 30335
    --rpc-port 9945
    --validator
    --rpc-cors=all
    --allow-private-ipv4
    --discover-local
    --unsafe-force-node-key-generation
  )

  # Insert //Three keys manually (no --three shorthand exists in Substrate)
  "$NODE_BINARY" key insert \
    --base-path /tmp/three \
    --chain="$FULL_PATH" \
    --scheme Sr25519 \
    --suri "//Three" \
    --key-type aura
  "$NODE_BINARY" key insert \
    --base-path /tmp/three \
    --chain="$FULL_PATH" \
    --scheme Ed25519 \
    --suri "//Three" \
    --key-type gran

  three_start=(
    "$NODE_BINARY"
    --base-path /tmp/three
    --chain="$FULL_PATH"
    --name Three
    --port 30336
    --rpc-port 9946
    --validator
    --rpc-cors=all
    --allow-private-ipv4
    --discover-local
    --unsafe-force-node-key-generation
  )

  # Provide RUN_IN_DOCKER local environment variable if run script in the docker image
  if [ "${RUN_IN_DOCKER:-}" == "1" ]; then
    one_start+=(--unsafe-rpc-external)
    two_start+=(--unsafe-rpc-external)
    three_start+=(--unsafe-rpc-external)
  fi

  trap 'pkill -P $$' EXIT SIGINT SIGTERM

  (
    ("${one_start[@]}" 2>&1) &
    ("${two_start[@]}" 2>&1) &
    ("${three_start[@]}" 2>&1)
    wait
  )
fi
