#!/usr/bin/env bash
set -euo pipefail

timeout_seconds="${CLONE_STOP_TIMEOUT_SECONDS:-60}"
[[ "$timeout_seconds" =~ ^[0-9]+$ ]] || {
  echo "invalid CLONE_STOP_TIMEOUT_SECONDS: $timeout_seconds" >&2
  exit 2
}

patterns=(
  "node-subtensor.*--base-path clones/mainnet-clone"
  "node-subtensor.*--base-path \.\./clones/mainnet-clone"
)

clone_running() {
  local pattern
  for pattern in "${patterns[@]}"; do
    pgrep -f "$pattern" > /dev/null && return 0
  done
  return 1
}

for pattern in "${patterns[@]}"; do
  pkill -f "$pattern" || true
done

deadline=$(($(date +%s) + timeout_seconds))
while clone_running && (( $(date +%s) < deadline )); do
  sleep 1
done

if clone_running; then
  echo "Clone node did not stop within ${timeout_seconds}s." >&2
  for pattern in "${patterns[@]}"; do
    pgrep -af "$pattern" >&2 || true
  done
  exit 1
fi
