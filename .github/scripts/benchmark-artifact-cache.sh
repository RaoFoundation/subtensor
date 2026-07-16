#!/usr/bin/env bash

set -euo pipefail

: "${RUNNER_TEMP:?RUNNER_TEMP must be set}"
: "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY must be set}"
: "${ARTIFACT_ID:?ARTIFACT_ID must be set}"
: "${ARTIFACT_DIGEST:?ARTIFACT_DIGEST must be set}"
: "${ARTIFACT_SIZE:?ARTIFACT_SIZE must be set}"

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
benchmark_dir="$RUNNER_TEMP/artifact-cache-benchmark"
direct_output="$benchmark_dir/direct-output"
first_output="$benchmark_dir/first-output"
second_output="$benchmark_dir/second-output"
rm -rf "$benchmark_dir"
mkdir -p "$benchmark_dir"
: > "$direct_output"
: > "$first_output"
: > "$second_output"

FIREACTIONS_ARTIFACT_CACHE_DISABLE=true \
  "$script_dir/download-artifact.sh" "$ARTIFACT_ID" mainnet-snapshot \
    "$ARTIFACT_DIGEST" "$ARTIFACT_SIZE" "$benchmark_dir/direct" "$direct_output"
"$script_dir/download-artifact.sh" "$ARTIFACT_ID" mainnet-snapshot \
  "$ARTIFACT_DIGEST" "$ARTIFACT_SIZE" "$benchmark_dir/local-first" "$first_output"
"$script_dir/download-artifact.sh" "$ARTIFACT_ID" mainnet-snapshot \
  "$ARTIFACT_DIGEST" "$ARTIFACT_SIZE" "$benchmark_dir/local-second" "$second_output"

direct_sha=$(sha256sum "$benchmark_dir/direct/mainnet-snapshot.tar.gz" | awk '{print $1}')
first_sha=$(sha256sum "$benchmark_dir/local-first/mainnet-snapshot.tar.gz" | awk '{print $1}')
second_sha=$(sha256sum "$benchmark_dir/local-second/mainnet-snapshot.tar.gz" | awk '{print $1}')
[[ "$direct_sha" == "$first_sha" && "$direct_sha" == "$second_sha" ]]

direct_seconds=$(sed -n 's/^seconds=//p' "$direct_output")
first_seconds=$(sed -n 's/^seconds=//p' "$first_output")
second_seconds=$(sed -n 's/^seconds=//p' "$second_output")
first_source=$(sed -n 's/^source=//p' "$first_output")
second_source=$(sed -n 's/^source=//p' "$second_output")
[[ "$second_source" == local-hit ]]

{
  echo "### Mainnet snapshot artifact cache"
  echo "- Artifact: $ARTIFACT_ID ($ARTIFACT_SIZE bytes)"
  echo "- Direct GitHub: ${direct_seconds}s"
  echo "- Cache-only probe 1: ${first_seconds}s ($first_source)"
  echo "- Cache-only probe 2: ${second_seconds}s ($second_source)"
  echo "- Extracted payload SHA-256: $second_sha"
} >> "$GITHUB_STEP_SUMMARY"
