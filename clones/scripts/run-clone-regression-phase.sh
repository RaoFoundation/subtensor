#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
JS_TESTS="$REPO_ROOT/clones/js-tests"

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
keep_clone_running=${KEEP_CLONE_RUNNING:-false}
[[ "$keep_clone_running" == true || "$keep_clone_running" == false ]] || {
  echo "KEEP_CLONE_RUNNING must be true or false" >&2
  exit 2
}

cleanup() {
  local status=$?
  if [[ "$keep_clone_running" == true ]]; then
    echo "Leaving local clone running (KEEP_CLONE_RUNNING=true)."
  else
    "$SCRIPT_DIR/stop-local-clone.sh" || true
  fi
  exit "$status"
}
trap cleanup EXIT

start_clone() {
  "$SCRIPT_DIR/start-local-clone-and-wait.sh" "${1:-accelerated}"
}

upgrade_runtime() {
  (
    cd "$JS_TESTS"
    npm run runtime:update:alice || { sleep 15; npm run runtime:update:alice; }
  )
}

migration_probe() {
  (
    cd "$JS_TESTS"
    npx tsx tests/test-mainnet-migration-completion.ts "$1"
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

run_pristine() {
  start_clone normal
  migration_probe before
  upgrade_runtime
  migration_probe upgraded
  migration_probe after
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

run_remaining() {
  start_clone
  upgrade_runtime
  (
    cd "$JS_TESTS"
    npm test
  )
  run_regressions remaining
  run_sdk_metadata_drift
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
