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
: "${SOAK_CHECKPOINT:?SOAK_CHECKPOINT is required}"
[[ "$SOAK_DEADLINE_EPOCH_MS" =~ ^[1-9][0-9]*$ ]] || {
  echo "SOAK_DEADLINE_EPOCH_MS must be a positive integer" >&2
  exit 2
}
[[ -f "$SOAK_CHECKPOINT" ]] || { echo "missing soak checkpoint: $SOAK_CHECKPOINT" >&2; exit 1; }

minimum_post_upgrade_blocks=${MINIMUM_POST_UPGRADE_BLOCKS:-7200}
[[ "$minimum_post_upgrade_blocks" =~ ^[1-9][0-9]*$ ]] || {
  echo "MINIMUM_POST_UPGRADE_BLOCKS must be a positive integer" >&2
  exit 2
}

baseline_monitor_report="$JS_TESTS/temp/clone-block-performance-baseline.json"
baseline_epoch_report="$JS_TESTS/temp/clone-epoch-coverage-baseline.json"
candidate_monitor_report="$JS_TESTS/temp/clone-block-performance-soak.json"
candidate_epoch_report="$JS_TESTS/temp/clone-epoch-coverage.json"
activation_report="$JS_TESTS/temp/runtime-upgrade-soak.json"
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

start_monitor() {
  local policy=$1 label=$2 activation=${3:-} baseline=${4:-}
  local start_offset ready_file diagnostic_file

  [[ -f "$NODE_LOG" ]] || : > "$NODE_LOG"
  start_offset=$(wc -c < "$NODE_LOG" | tr -d '[:space:]')
  ready_file="$JS_TESTS/temp/clone-block-monitor-$label.ready.json"
  diagnostic_file="$JS_TESTS/temp/clone-block-diagnostics-$label.log"
  rm -f "$ready_file" "$diagnostic_file"
  CLONE_MONITOR_ACTIVATION_REPORT="$activation" \
    CLONE_MONITOR_READY_FILE="$ready_file" \
    CLONE_MONITOR_BASELINE_REPORT="$baseline" \
    CLONE_MONITOR_DIAGNOSTIC_FILE="$diagnostic_file" \
    "$SCRIPT_DIR/run-clone-block-monitor.sh" "$policy" "$label" "$start_offset" &
  monitor_pid=$!
  wait_for_monitor_ready "$monitor_pid" "$ready_file"
}

run_epoch_coverage() {
  local release_gate=$1 upgrade_block=$2 minimum_blocks=$3 report=$4 log_name=$5
  local status=0

  (
    cd "$JS_TESTS"
    npm run soak:epochs -- \
      --epoch-cycles "$epoch_cycles" \
      --release-gate "$release_gate" \
      --upgrade-block "$upgrade_block" \
      --minimum-post-upgrade-blocks "$minimum_blocks" \
      --deadline-epoch-ms "$SOAK_DEADLINE_EPOCH_MS" \
      --report "$report" \
      --log-name "$log_name"
  ) &
  coverage_pid=$!

  supervise_monitor_and_workload "$monitor_pid" "$coverage_pid" "$release_gate epoch coverage" || status=$?
  coverage_pid=
  monitor_pid=
  return "$status"
}

upgrade_runtime() {
  rm -f "$activation_report"
  (
    cd "$JS_TESTS"
    npm run runtime:update:alice -- --report "$activation_report" || {
      sleep 15
      npm run runtime:update:alice -- --report "$activation_report"
    }
  )
}

mkdir -p "$JS_TESTS/temp"
rm -f \
  "$baseline_monitor_report" "$baseline_epoch_report" \
  "$candidate_monitor_report" "$candidate_epoch_report" "$activation_report"

echo "Running same-snapshot pre-upgrade latency baseline (report-only thresholds)."
"$SCRIPT_DIR/start-local-clone-and-wait.sh" accelerated
start_monitor baseline baseline
run_epoch_coverage none 0 0 "$baseline_epoch_report" clone-epoch-baseline.log

"$SCRIPT_DIR/stop-local-clone.sh"
"$SCRIPT_DIR/local-clone-checkpoint.sh" restore "$SOAK_CHECKPOINT"

echo "Running release-v438 post-upgrade epoch soak."
"$SCRIPT_DIR/start-local-clone-and-wait.sh" accelerated
start_monitor collect soak "$activation_report" "$baseline_monitor_report"
upgrade_runtime
upgrade_block=$(jq -er '.upgradeBlock | numbers' "$activation_report")
[[ "$upgrade_block" =~ ^[0-9]+$ ]] || { echo "invalid upgrade block: $upgrade_block" >&2; exit 1; }
run_epoch_coverage \
  beta-basket-v2 "$upgrade_block" "$minimum_post_upgrade_blocks" \
  "$candidate_epoch_report" clone-epoch-soak.log
