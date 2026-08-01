#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
JS_TESTS="$REPO_ROOT/clones/js-tests"
NODE_LOG="$REPO_ROOT/clone-node.log"
source "$SCRIPT_DIR/clone-process-supervision.sh"

[[ $# -eq 1 ]] || { echo "usage: run-clone-epoch-soak.sh EPOCH_CYCLES" >&2; exit 2; }
epoch_cycles=$1
[[ "$epoch_cycles" =~ ^[123]$ ]] || { echo "epoch cycles must be 1, 2, or 3" >&2; exit 2; }
: "${SOAK_DEADLINE_EPOCH_MS:?SOAK_DEADLINE_EPOCH_MS is required}"
[[ "$SOAK_DEADLINE_EPOCH_MS" =~ ^[1-9][0-9]*$ ]] || {
  echo "SOAK_DEADLINE_EPOCH_MS must be a positive integer" >&2
  exit 2
}

monitor_report="$JS_TESTS/temp/clone-block-performance-soak.json"
epoch_report="$JS_TESTS/temp/clone-epoch-coverage.json"
monitor_pid=
coverage_pid=

cleanup() {
  local status=$?
  if [[ -n "$coverage_pid" ]]; then
    terminate_process_tree "$coverage_pid"
    wait "$coverage_pid" 2>/dev/null || true
  fi
  if [[ -n "$monitor_pid" ]]; then
    kill -TERM "$monitor_pid" 2>/dev/null || true
    wait "$monitor_pid" 2>/dev/null || true
  fi
  "$SCRIPT_DIR/stop-local-clone.sh" || true
  exit "$status"
}
trap cleanup EXIT

mkdir -p "$JS_TESTS/temp"
rm -f "$monitor_report" "$epoch_report"

"$SCRIPT_DIR/start-local-clone-and-wait.sh" accelerated
(
  cd "$JS_TESTS"
  npm run runtime:update:alice || { sleep 15; npm run runtime:update:alice; }
)

[[ -f "$NODE_LOG" ]] || : > "$NODE_LOG"
start_offset=$(wc -c < "$NODE_LOG" | tr -d '[:space:]')
"$SCRIPT_DIR/run-clone-block-monitor.sh" collect soak "$start_offset" &
monitor_pid=$!

(
  cd "$JS_TESTS"
  npm run soak:epochs -- \
    --epoch-cycles "$epoch_cycles" \
    --deadline-epoch-ms "$SOAK_DEADLINE_EPOCH_MS" \
    --report "$epoch_report"
) &
coverage_pid=$!

status=0
supervise_monitor_and_workload "$monitor_pid" "$coverage_pid" "epoch coverage" || status=$?
coverage_pid=
monitor_pid=
exit "$status"
