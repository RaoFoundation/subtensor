#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 ARTIFACT_NAME" >&2
  exit 2
fi

artifact_name="$1"
: "${GH_TOKEN:?GH_TOKEN must contain an Actions token}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GITHUB_RUN_ID:?GITHUB_RUN_ID must be set}"
: "${GITHUB_SHA:?GITHUB_SHA must be set}"

case "$artifact_name" in
  mainnet-snapshot|try-runtime-snap-v0.10.1-mainnet|try-runtime-snap-v0.10.1-testnet|try-runtime-snap-v0.10.1-devnet) ;;
  *) echo "artifact is outside the trusted mirror allowlist: $artifact_name" >&2; exit 2 ;;
esac

metadata=$(gh api \
  -H 'Accept: application/vnd.github+json' \
  "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/artifacts?per_page=100")
selection=$(jq -cer --arg name "$artifact_name" '
  [.artifacts[] | select(.name == $name and .expired == false)]
  | if length == 1 then .[0] else error("expected exactly one current-run artifact") end
' <<< "$metadata")
artifact_id=$(jq -er '.id | select(type == "number" and . > 0)' <<< "$selection")
digest=$(jq -er '.digest | select(type == "string")' <<< "$selection")
[[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]

"$(dirname "$0")/publish-artifact-mirror.sh" \
  "$artifact_id" \
  "$artifact_name" \
  "$digest" \
  "$GITHUB_SHA" \
  .github/workflows/refresh-mainnet-snapshot.yml
