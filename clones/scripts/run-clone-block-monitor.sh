#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
JS_TESTS="$REPO_ROOT/clones/js-tests"

[[ $# -eq 3 ]] || {
  echo "usage: run-clone-block-monitor.sh fail-fast|collect REPORT_LABEL START_OFFSET" >&2
  exit 2
}
policy=$1
report_label=$2
start_offset=$3
[[ "$policy" == fail-fast || "$policy" == collect ]] || {
  echo "monitor policy must be fail-fast or collect" >&2
  exit 2
}
[[ "$report_label" =~ ^[A-Za-z0-9_-]+$ ]] || {
  echo "report label may contain only letters, digits, underscores, and hyphens" >&2
  exit 2
}
[[ "$start_offset" =~ ^[0-9]+$ ]] || {
  echo "start offset must be a non-negative integer" >&2
  exit 2
}

mkdir -p "$JS_TESTS/temp"
cd "$JS_TESTS"
exec node --import tsx scripts/monitor-clone-blocks.ts \
  --policy "$policy" \
  --node-log "$REPO_ROOT/clone-node.log" \
  --start-offset "$start_offset" \
  --report "$JS_TESTS/temp/clone-block-performance-${report_label}.json" \
  --log-name "clone-block-performance-${report_label}.log"
