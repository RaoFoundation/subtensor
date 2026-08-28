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
PID_FILE="${LOCALNET_PID_FILE:-/tmp/subtensor-localnet.pids}"

terminate_pids() {
  local pids=("$@")
  local any_running pid

  [ "${#pids[@]}" -gt 0 ] || return 0

  for pid in "${pids[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
  done

  # Give node databases time to close cleanly before forcing termination.
  for _ in {1..30}; do
    any_running=0
    for pid in "${pids[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        any_running=1
        break
      fi
    done
    [ "$any_running" -eq 1 ] || break
    sleep 1
  done

  for pid in "${pids[@]}"; do
    kill -KILL "$pid" 2>/dev/null || true
    # Only children of this shell can be reaped; stale recorded PIDs simply
    # make wait return non-zero, which is intentionally ignored.
    wait "$pid" 2>/dev/null || true
  done
}

stop_recorded_nodes() {
  [ -f "$PID_FILE" ] || return 0

  local recorded_pids=()
  while IFS= read -r pid; do
    case "$pid" in
      ''|*[!0-9]*) continue ;;
    esac

    # PID files can survive a container restart through a /tmp volume. Only
    # signal a reused PID when it is still one of our node processes.
    if kill -0 "$pid" 2>/dev/null \
      && ps -p "$pid" -o comm= 2>/dev/null | grep -q 'node-subtensor'; then
      recorded_pids+=("$pid")
    fi
  done <"$PID_FILE"

  if [ "${#recorded_pids[@]}" -eq 0 ]; then
    rm -f "$PID_FILE"
    return 0
  fi

  # Never purge a database while a node from the previous run still has it
  # open. Terminate only the validated PIDs before touching chain state.
  terminate_pids "${recorded_pids[@]}"
  rm -f "$PID_FILE"
}

if [ "$BUILD_ONLY" -eq 0 ]; then
  stop_recorded_nodes
fi

if [ ! -d "$SPEC_PATH" ]; then
  echo "*** Creating directory ${SPEC_PATH}..."
  mkdir -p "$SPEC_PATH"
fi

if [[ "$BUILD_BINARY" == "1" ]]; then
  echo "*** Building substrate binary..."

  BUILD_CMD=(
    cargo build
    --locked
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

  NODE_PIDS=()
  SHUTDOWN_STARTED=0

  shutdown_nodes() {
    [ "$SHUTDOWN_STARTED" -eq 0 ] || return 0
    SHUTDOWN_STARTED=1
    trap - EXIT INT TERM

    terminate_pids "${NODE_PIDS[@]}"
    rm -f "$PID_FILE"
  }

  # Called indirectly by the signal trap below.
  # shellcheck disable=SC2329
  handle_signal() {
    shutdown_nodes
    exit 143
  }

  wait_for_node_exit() {
    local active_pids pid

    # Bash 3.2 (the system Bash on macOS) has no `wait -n`. The job table is
    # portable across every supported Bash version and lets us identify and
    # reap whichever authority exits first.
    while true; do
      # `jobs -p` retains completed jobs in some non-interactive shells. Union
      # running and stopped jobs instead, so only truly exited children vanish.
      active_pids="$(jobs -pr; jobs -ps)"
      for pid in "${NODE_PIDS[@]}"; do
        if ! grep -qx "$pid" <<<"$active_pids"; then
          wait "$pid"
          return $?
        fi
      done
      sleep 1
    done
  }

  # Archive mode keeps historical state queryable. Frontier's eth_getLogs /
  # eth_getTransactionReceipt read receipts from per-block state, so with the
  # default 256-block state pruning EVM logs silently disappear after ~4 min,
  # which breaks anything indexing history (e.g. Hyperlane agents).
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
    --state-pruning archive
    --blocks-pruning archive
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
    --state-pruning archive
    --blocks-pruning archive
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
    --state-pruning archive
    --blocks-pruning archive
  )

  # Provide RUN_IN_DOCKER local environment variable if run script in the docker image
  if [ "${RUN_IN_DOCKER:-}" == "1" ]; then
    one_start+=(--unsafe-rpc-external)
    two_start+=(--unsafe-rpc-external)
    three_start+=(--unsafe-rpc-external)
  fi

  trap shutdown_nodes EXIT
  trap handle_signal INT TERM

  "${one_start[@]}" 2>&1 &
  NODE_PIDS+=("$!")
  "${two_start[@]}" 2>&1 &
  NODE_PIDS+=("$!")
  "${three_start[@]}" 2>&1 &
  NODE_PIDS+=("$!")
  printf '%s\n' "${NODE_PIDS[@]}" >"$PID_FILE"

  set +e
  wait_for_node_exit
  node_status=$?
  set -e

  # A node exiting by itself, even with status zero, leaves a broken partial
  # network. Stop its peers and make that visible to Docker/CI.
  [ "$node_status" -ne 0 ] || node_status=1
  shutdown_nodes
  exit "$node_status"
fi
