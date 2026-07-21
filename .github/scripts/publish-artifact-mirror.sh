#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 5 || ! "$1" =~ ^[1-9][0-9]*$ ]]; then
  echo "usage: $0 ARTIFACT_ID ARTIFACT_NAME SHA256_DIGEST PRODUCER_SHA WORKFLOW_PATH" >&2
  exit 2
fi

artifact_id="$1"
artifact_name="$2"
digest="$3"
producer_sha="$4"
workflow_path="$5"

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GH_TOKEN:?GH_TOKEN must contain an Actions token}"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
archive="$tmp/artifact.zip"

for attempt in 1 2 3 4 5; do
  if gh api \
    -H 'Accept: application/vnd.github+json' \
    "repos/$GITHUB_REPOSITORY/actions/artifacts/$artifact_id/zip" \
    > "$archive"; then
    break
  fi
  rm -f "$archive"
  sleep $((attempt * 5))
done
[[ -s "$archive" ]]

"$(dirname "$0")/r2-artifact-mirror.py" \
  "$archive" \
  "$artifact_id" \
  "$artifact_name" \
  "$digest" \
  "$producer_sha" \
  "$workflow_path"
