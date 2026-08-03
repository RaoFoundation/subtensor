#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
JS_TESTS="$REPO_ROOT/clones/js-tests"
source "$SCRIPT_DIR/clone-process-supervision.sh"

usage() {
  echo "usage: run-clone-regression-phase.sh pristine|remaining|combined" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
phase=$1
[[ "$phase" == pristine || "$phase" == remaining || "$phase" == combined ]] || usage

run_sdk_drift=${RUN_SDK_DRIFT:-false}
[[ "$run_sdk_drift" == true || "$run_sdk_drift" == false ]] || {
  echo "RUN_SDK_DRIFT must be true or false" >&2
  exit 2
}

cleanup() {
  local status=$?
  if [[ -n "${workload_pid:-}" ]]; then
    terminate_process_tree "$workload_pid"
    wait "$workload_pid" 2>/dev/null || true
  fi
  if [[ -n "${monitor_pid:-}" ]]; then
    kill -TERM "$monitor_pid" 2>/dev/null || true
    wait "$monitor_pid" 2>/dev/null || true
  fi
  "$SCRIPT_DIR/stop-local-clone.sh" || true
  exit "$status"
}
trap cleanup EXIT

start_clone() {
  "$SCRIPT_DIR/start-local-clone-and-wait.sh" accelerated
}

upgrade_runtime() {
  local activation_report=$1
  rm -f "$activation_report"
  (
    cd "$JS_TESTS"
    npm run runtime:update:alice -- --report "$activation_report" || {
      sleep 15
      npm run runtime:update:alice -- --report "$activation_report"
    }
  )
}

run_regressions() {
  local selected_phase=$1
  (
    cd "$JS_TESTS"
    CLONE_REGRESSION_PHASE="$selected_phase" \
      CLONE_REGRESSION_TIMEOUT_MS=1800000 \
      npm run test:clone-regressions
  )
}

wait_for_readiness() {
  local phase_name=$1
  local timeout_ms=${CLONE_READINESS_TIMEOUT_MS:-2700000}
  [[ "$timeout_ms" =~ ^[1-9][0-9]*$ ]] || {
    echo "CLONE_READINESS_TIMEOUT_MS must be a positive integer" >&2
    return 2
  }
  (
    cd "$JS_TESTS"
    npm run wait:beta-basket-v2-readiness -- \
      --label "$phase_name" \
      --timeout-ms "$timeout_ms" \
      --report "temp/clone-readiness-$phase_name.json"
  )
}

start_block_monitor() {
  local phase_name=$1
  local activation_report=$2
  local node_log="$REPO_ROOT/clone-node.log"
  local start_offset ready_file diagnostic_file

  [[ -f "$node_log" ]] || : > "$node_log"
  start_offset=$(wc -c < "$node_log" | tr -d '[:space:]')
  ready_file="$JS_TESTS/temp/clone-block-monitor-$phase_name.ready.json"
  diagnostic_file="$JS_TESTS/temp/clone-block-diagnostics-$phase_name.log"
  rm -f "$ready_file" "$diagnostic_file"
  CLONE_MONITOR_ACTIVATION_REPORT="$activation_report" \
    CLONE_MONITOR_READY_FILE="$ready_file" \
    CLONE_MONITOR_DIAGNOSTIC_FILE="$diagnostic_file" \
    "$SCRIPT_DIR/run-clone-block-monitor.sh" fail-fast "$phase_name" "$start_offset" &
  monitor_pid=$!
  wait_for_monitor_ready "$monitor_pid" "$ready_file"
}

run_monitored_workload() {
  local phase_name=$1
  shift
  local status=0

  "$@" &
  workload_pid=$!

  supervise_monitor_and_workload "$monitor_pid" "$workload_pid" "$phase_name workload" || status=$?
  workload_pid=
  monitor_pid=
  return "$status"
}

run_pristine() {
  local activation_report="$JS_TESTS/temp/runtime-upgrade-pristine.json"
  start_clone
  start_block_monitor pristine "$activation_report"
  upgrade_runtime "$activation_report"
  run_monitored_workload pristine run_pristine_workload
}

run_pristine_workload() {
  wait_for_readiness pristine
  run_regressions pristine
}

run_sdk_metadata_drift() {
  [[ "$run_sdk_drift" == true ]] || return 0
  if ! command -v uv >/dev/null 2>&1; then
    curl -LsSf https://astral.sh/uv/0.11.28/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
  fi
  (
    cd "$REPO_ROOT/sdk/python"
    uv sync --locked --all-extras --dev
    uv run python -m codegen.check --drift "${WS_ENDPOINT:-ws://127.0.0.1:9944}"
  )
}

run_remaining_workload() {
  wait_for_readiness remaining
  (
    cd "$JS_TESTS"
    npm test
  )
  run_regressions remaining
  run_sdk_metadata_drift
}

run_remaining() {
  local activation_report="$JS_TESTS/temp/runtime-upgrade-remaining.json"
  start_clone
  start_block_monitor remaining "$activation_report"
  upgrade_runtime "$activation_report"
  run_monitored_workload remaining run_remaining_workload
}

case "$phase" in
  pristine)
    run_pristine
    ;;
  remaining)
    run_remaining
    ;;
  combined)
    : "${CLONE_CHECKPOINT:?CLONE_CHECKPOINT is required for combined execution}"
    run_pristine
    "$SCRIPT_DIR/stop-local-clone.sh"
    "$SCRIPT_DIR/local-clone-checkpoint.sh" restore "$CLONE_CHECKPOINT"
    run_remaining
    ;;
esac
