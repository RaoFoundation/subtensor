#!/usr/bin/env bash
set -euo pipefail

log_file="${1:-clone-node.log}"

nohup ./clones/scripts/start-local-clone.sh --sealing 250 > "$log_file" 2>&1 &

ready=false
for _ in $(seq 1 450); do
  if curl -sf -H "Content-Type: application/json" \
    -d '{"id":1,"jsonrpc":"2.0","method":"system_health","params":[]}' \
    http://127.0.0.1:9944 > /dev/null; then
    ready=true
    break
  fi
  sleep 2
done

if [ "$ready" != true ]; then
  echo "Clone node failed to start:"
  cat "$log_file"
  exit 1
fi

height() {
  local hex
  hex=$(curl -fsS -H "Content-Type: application/json" \
    -d '{"id":1,"jsonrpc":"2.0","method":"chain_getHeader","params":[]}' \
    http://127.0.0.1:9944 | jq -er '.result.number')
  echo $((16#${hex#0x}))
}

first=$(height)
deadline=$(($(date -u +%s) + 60))
while [ "$(date -u +%s)" -lt "$deadline" ]; do
  current=$(height)
  if [ "$current" -ge "$((first + 20))" ]; then
    echo "Accelerated clone advanced from block $first to $current."
    exit 0
  fi
  sleep 1
done

tail -n 200 "$log_file"
exit 1
