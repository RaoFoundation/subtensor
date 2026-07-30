#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

log_file="clone-node.log"
ready_attempts=450
advance_timeout_seconds=60

usage() {
  cat >&2 <<'EOF'
Usage: start-local-clone-and-wait.sh MODE

Modes:
  accelerated   Seal every 250ms and require 20 blocks of advancement
  normal        Seal every 12s and require 2 blocks of advancement
  manual        Use manual sealing and require RPC health only
  node-default  Preserve the node's default sealing and require RPC health only
EOF
  exit 2
}

[[ $# -eq 1 ]] || usage
mode="$1"

cd "$REPO_ROOT"
case "$mode" in
  accelerated)
    nohup ./clones/scripts/start-local-clone.sh --sealing 250 > "$log_file" 2>&1 &
    ;;
  normal)
    # The exhaustive post-migration audit pins the exact completion block while the
    # 12-second chain keeps advancing. Retain enough historical state for that scan;
    # the restored clone database uses numeric pruning, so increasing its window is valid.
    nohup ./clones/scripts/start-local-clone.sh \
      --sealing 12000 \
      --state-pruning 10000 \
      > "$log_file" 2>&1 &
    ;;
  manual)
    nohup ./clones/scripts/start-local-clone.sh --sealing manual > "$log_file" 2>&1 &
    ;;
  node-default)
    nohup ./clones/scripts/start-local-clone.sh > "$log_file" 2>&1 &
    ;;
  *) usage ;;
esac
node_pid=$!

cleanup_on_failure() {
  local status=$?
  (( status != 0 )) || return
  echo "Clone node startup failed; last log lines:"
  tail -n 200 "$log_file" 2>/dev/null || true
  "$SCRIPT_DIR/stop-local-clone.sh" || true
}
trap cleanup_on_failure EXIT

rpc() {
  local method="$1"
  curl -fsS -H "Content-Type: application/json" \
    -d "{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[]}" \
    http://127.0.0.1:9944
}

ready=false
for ((attempt = 1; attempt <= ready_attempts; attempt++)); do
  if rpc system_health > /dev/null; then
    ready=true
    break
  fi
  kill -0 "$node_pid" 2>/dev/null || break
  sleep 2
done
[[ "$ready" == true ]] || exit 1

if [[ "$mode" != accelerated && "$mode" != normal ]]; then
  echo "Clone node is healthy."
  trap - EXIT
  exit 0
fi

height() {
  local hex
  hex=$(rpc chain_getHeader | jq -er '.result.number')
  echo $((16#${hex#0x}))
}

first=$(height)
deadline=$(($(date +%s) + advance_timeout_seconds))
required_blocks=20
[[ "$mode" == normal ]] && required_blocks=2
while (( $(date +%s) < deadline )); do
  current=$(height)
  if (( current >= first + required_blocks )); then
    echo "Clone advanced from block $first to $current."
    trap - EXIT
    exit 0
  fi
  sleep 1
done

exit 1
