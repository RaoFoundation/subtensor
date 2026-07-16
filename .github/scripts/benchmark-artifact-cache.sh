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
cold_output="$benchmark_dir/cold-output"
warm_output="$benchmark_dir/warm-output"
rm -rf "$benchmark_dir"
mkdir -p "$benchmark_dir"
: > "$direct_output"
: > "$cold_output"
: > "$warm_output"

FIREACTIONS_ARTIFACT_CACHE_DISABLE=true \
  "$script_dir/download-artifact.sh" "$ARTIFACT_ID" mainnet-snapshot \
    "$ARTIFACT_DIGEST" "$ARTIFACT_SIZE" "$benchmark_dir/direct" "$direct_output"
"$script_dir/download-artifact.sh" "$ARTIFACT_ID" mainnet-snapshot \
  "$ARTIFACT_DIGEST" "$ARTIFACT_SIZE" "$benchmark_dir/local-cold" "$cold_output"
"$script_dir/download-artifact.sh" "$ARTIFACT_ID" mainnet-snapshot \
  "$ARTIFACT_DIGEST" "$ARTIFACT_SIZE" "$benchmark_dir/local-warm" "$warm_output"

direct_sha=$(sha256sum "$benchmark_dir/direct/mainnet-snapshot.tar.gz" | awk '{print $1}')
cold_sha=$(sha256sum "$benchmark_dir/local-cold/mainnet-snapshot.tar.gz" | awk '{print $1}')
warm_sha=$(sha256sum "$benchmark_dir/local-warm/mainnet-snapshot.tar.gz" | awk '{print $1}')
[[ "$direct_sha" == "$cold_sha" && "$direct_sha" == "$warm_sha" ]]

direct_seconds=$(sed -n 's/^seconds=//p' "$direct_output")
cold_seconds=$(sed -n 's/^seconds=//p' "$cold_output")
warm_seconds=$(sed -n 's/^seconds=//p' "$warm_output")
warm_source=$(sed -n 's/^source=//p' "$warm_output")
[[ "$warm_source" == local-hit ]]

{
  echo "### Mainnet snapshot artifact cache"
  echo "- Artifact: $ARTIFACT_ID ($ARTIFACT_SIZE bytes)"
  echo "- Direct GitHub: ${direct_seconds}s"
  echo "- Local cold fill: ${cold_seconds}s"
  echo "- Local warm hit: ${warm_seconds}s"
  echo "- Extracted payload SHA-256: $warm_sha"
} >> "$GITHUB_STEP_SUMMARY"
