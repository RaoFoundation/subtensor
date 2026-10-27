#!/usr/bin/env bash
# Find the node artifact a trusted push to main uploaded for BASE_NODE_SHA.
# Only runs of WORKFLOW_PATH triggered by a push to main in this repository
# qualify, and download-artifact.sh later verifies the archive digest and size.
# Writes found=true|false plus artifact_id, digest, and size to OUTPUT_FILE.
# A miss is not an error: the caller then builds the node.

set -euo pipefail

usage() {
  echo "usage: $0 WORKFLOW_PATH ARTIFACT_NAME OUTPUT_FILE" >&2
  exit 2
}

[[ $# -eq 3 ]] || usage
workflow_path="$1"
artifact_name="$2"
output_file="$3"

: "${GH_TOKEN:?GH_TOKEN must contain a short-lived Actions token}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GITHUB_REPOSITORY_ID:?GITHUB_REPOSITORY_ID must be set}"
: "${BASE_NODE_SHA:?BASE_NODE_SHA must name the base commit}"

[[ "$workflow_path" =~ ^\.github/workflows/[A-Za-z0-9._-]+\.yml$ ]] || usage
[[ "$artifact_name" =~ ^[A-Za-z0-9._-]+$ ]] || usage
[[ "$GITHUB_REPOSITORY_ID" =~ ^[1-9][0-9]*$ ]] || usage
[[ "$BASE_NODE_SHA" =~ ^[0-9a-f]{40}$ ]] || usage

miss() {
  echo "no $artifact_name from a main push of $BASE_NODE_SHA: $1"
  echo "found=false" >> "$output_file"
  exit 0
}

runs=$(gh api -H 'Accept: application/vnd.github+json' \
  "repos/$GITHUB_REPOSITORY/actions/workflows/${workflow_path##*/}/runs?event=push&branch=main&head_sha=$BASE_NODE_SHA&per_page=20" \
  2>/dev/null) || miss "run lookup failed"

run_ids=$(jq -r \
  --arg sha "$BASE_NODE_SHA" \
  --arg repository_id "$GITHUB_REPOSITORY_ID" \
  --arg workflow_path "$workflow_path" '
    [.workflow_runs[]
      | select(.head_sha == $sha)
      | select(.event == "push" and .head_branch == "main")
      | select((.head_repository.id | tostring) == $repository_id)
      | select(.path == $workflow_path)]
    | sort_by(.run_attempt, .id) | reverse | .[].id
  ' <<< "$runs") || miss "run response invalid"
[[ -n "$run_ids" ]] || miss "no qualifying run"

for run_id in $run_ids; do
  artifacts=$(gh api -H 'Accept: application/vnd.github+json' \
    "repos/$GITHUB_REPOSITORY/actions/runs/$run_id/artifacts?name=$artifact_name&per_page=10" \
    2>/dev/null) || continue
  selected=$(jq -c \
    --arg name "$artifact_name" \
    --arg sha "$BASE_NODE_SHA" \
    --arg repository_id "$GITHUB_REPOSITORY_ID" \
    --argjson run_id "$run_id" '
      [.artifacts[]
        | select(.name == $name and .expired == false)
        | select(.id > 0 and .size_in_bytes > 0)
        | select((.digest // "") | test("^sha256:[0-9a-f]{64}$"))
        | select(.workflow_run.id == $run_id and .workflow_run.head_sha == $sha)
        | select((.workflow_run.head_repository_id | tostring) == $repository_id)]
      | sort_by(.id) | last // empty
    ' <<< "$artifacts") || continue
  [[ -n "$selected" ]] || continue
  {
    echo "found=true"
    echo "artifact_id=$(jq -r .id <<< "$selected")"
    echo "digest=$(jq -r .digest <<< "$selected")"
    echo "size=$(jq -r .size_in_bytes <<< "$selected")"
  } >> "$output_file"
  echo "found $artifact_name from run $run_id for $BASE_NODE_SHA"
  exit 0
done

miss "no unexpired artifact"
