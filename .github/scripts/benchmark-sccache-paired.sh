#!/usr/bin/env bash

set -euo pipefail

: "${RUNNER_TEMP:?RUNNER_TEMP must be set}"
: "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY must be set}"
: "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID must be set}"
: "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY must be set}"

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
config="$RUNNER_TEMP/sccache-local-contract.json"
output="$RUNNER_TEMP/sccache-local-contract.output"
times="$RUNNER_TEMP/sccache-paired-times"
trap 'rm -f "$config" "$output"' EXIT
: > "$output"

SCCACHE_GHA_FALLBACK=false SCCACHE_LOCAL_TIER_MODE=auto \
  "$script_dir/sccache-configure.sh" prepare reader "$config" "$output"
python3 -c '
import json, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
local = data.get("local")
if not isinstance(local, dict) or local.get("endpoint") != "http://192.168.128.1:8092":
    raise SystemExit("validated host-local tier is unavailable")
' "$config"

start_backend() {
  local mode="$1"
  sccache --stop-server >/dev/null 2>&1 || true
  if [[ "$mode" == local ]]; then
    export SCCACHE_MULTILEVEL_CHAIN=webdav,s3
    export SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY=ignore
    export SCCACHE_WEBDAV_ENDPOINT=http://192.168.128.1:8092
    export SCCACHE_WEBDAV_KEY_PREFIX=""
    export SCCACHE_WEBDAV_USERNAME="$AWS_ACCESS_KEY_ID"
    export SCCACHE_WEBDAV_PASSWORD="$AWS_SECRET_ACCESS_KEY"
  else
    unset SCCACHE_MULTILEVEL_CHAIN SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY
    unset SCCACHE_WEBDAV_ENDPOINT SCCACHE_WEBDAV_KEY_PREFIX
    unset SCCACHE_WEBDAV_USERNAME SCCACHE_WEBDAV_PASSWORD
  fi
  sccache --start-server >/dev/null
}

measure() {
  local label="$1"
  local mode="$2"
  local started seconds

  cargo clean
  start_backend "$mode"
  sccache --zero-stats >/dev/null
  started=$(date -u +%s)
  cargo check --locked -p node-subtensor-runtime
  seconds=$(($(date -u +%s) - started))
  if ! sccache --show-adv-stats 2>&1 | tee "$RUNNER_TEMP/sccache-$label.txt"; then
    sccache --show-stats | tee "$RUNNER_TEMP/sccache-$label.txt"
  fi
  echo "$label=$seconds" >> "$times"
}

: > "$times"
# Discard the first compile so registry extraction, filesystem page cache, and
# daemon startup do not get attributed to either backend.
measure warmup origin
measure local_1 local
measure origin_1 origin
measure origin_2 origin
measure local_2 local

source "$times"
origin_average=$(( (origin_1 + origin_2) / 2 ))
local_average=$(( (local_1 + local_2) / 2 ))
{
  echo "### Paired sccache benchmark (same VM)"
  echo "- Discarded warmup: origin"
  echo "- Measured order: local, origin, origin, local"
  echo "- Direct R2: ${origin_1}s, ${origin_2}s (mean ${origin_average}s)"
  echo "- Warm host-local: ${local_1}s, ${local_2}s (mean ${local_average}s)"
  for label in warmup local_1 origin_1 origin_2 local_2; do
    echo
    echo "#### $label"
    echo '```text'
    cat "$RUNNER_TEMP/sccache-$label.txt"
    echo '```'
  done
} >> "$GITHUB_STEP_SUMMARY"
