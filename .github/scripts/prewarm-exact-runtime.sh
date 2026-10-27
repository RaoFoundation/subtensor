#!/usr/bin/env bash

set -euo pipefail

: "${RUNNER_TEMP:?RUNNER_TEMP must be set}"
: "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY must be set}"

[[ "${SCCACHE_ENABLED:-false}" == true ]]
[[ "${SCCACHE_BACKEND:-}" == r2 ]]
[[ "${SCCACHE_LOCAL_TIER:-false}" == false ]]

results="$RUNNER_TEMP/exact-prewarm"
: > "$results"

measure() {
  local label="$1"
  local started seconds stats

  cargo clean
  sccache --zero-stats >/dev/null
  started=$(date -u +%s)
  cargo check --locked -p node-subtensor-runtime
  seconds=$(($(date -u +%s) - started))
  stats=$(sccache --show-stats)
  printf '%s\n' "$stats" | tee "$RUNNER_TEMP/sccache-$label.txt"
  printf '%s=%s\n' "${label}_seconds" "$seconds" >> "$results"
  printf '%s=%s\n' "${label}_rust_hits" \
    "$(awk '$1 == "Cache" && $2 == "hits" && $3 == "(Rust)" {count = $4} END {print count + 0}' <<< "$stats")" \
    >> "$results"
  printf '%s=%s\n' "${label}_rust_misses" \
    "$(awk '$1 == "Cache" && $2 == "misses" && $3 == "(Rust)" {count = $4} END {print count + 0}' <<< "$stats")" \
    >> "$results"
}

measure fill
measure verify
source "$results"

[[ "$verify_rust_hits" =~ ^[0-9]+$ && "$verify_rust_misses" =~ ^[0-9]+$ ]]
if (( verify_rust_hits < 500 || verify_rust_misses > 10 )); then
  echo "::error::exact-key verification produced ${verify_rust_hits} Rust hits and ${verify_rust_misses} misses"
  exit 1
fi

{
  echo "### Exact runtime-only R2 prewarm"
  echo "- Fill: ${fill_seconds}s (${fill_rust_hits} Rust hits, ${fill_rust_misses} misses)"
  echo "- Clean verification: ${verify_seconds}s (${verify_rust_hits} Rust hits, ${verify_rust_misses} misses)"
} >> "$GITHUB_STEP_SUMMARY"
