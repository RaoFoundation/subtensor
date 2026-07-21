#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "usage: $0 OUTPUT_FILE [MAX_WAIT_SECONDS]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
output_file="$1"
max_wait_seconds="${2:-360}"

: "${GH_TOKEN:?GH_TOKEN must contain a short-lived Actions token}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GITHUB_REPOSITORY_ID:?GITHUB_REPOSITORY_ID must be set}"
: "${GITHUB_SHA:?GITHUB_SHA must be set}"
: "${GITHUB_PR_HEAD_SHA:?GITHUB_PR_HEAD_SHA must be set}"

[[ "$GITHUB_REPOSITORY_ID" =~ ^[1-9][0-9]*$ ]] || usage
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
[[ "$GITHUB_PR_HEAD_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
[[ "$max_wait_seconds" =~ ^[0-9]+$ ]] || usage

artifact_name="node-subtensor-release-$GITHUB_SHA"
workflow_path=.github/workflows/runtime-checks.yml
started=$(date -u +%s)

write_miss() {
  {
    echo "found=false"
    echo "waited_seconds=$(($(date -u +%s) - started))"
  } >> "$output_file"
}

while true; do
  artifacts=$(gh api \
    -H 'Accept: application/vnd.github+json' \
    "repos/$GITHUB_REPOSITORY/actions/artifacts?name=$artifact_name&per_page=100" \
    2>/dev/null || true)

  if jq -e . >/dev/null 2>&1 <<< "$artifacts"; then
    while IFS= read -r candidate; do
      [[ -n "$candidate" ]] || continue
      artifact_id=$(jq -er '.id' <<< "$candidate")
      run_id=$(jq -er '.workflow_run.id' <<< "$candidate")

      run=$(gh api \
        -H 'Accept: application/vnd.github+json' \
        "repos/$GITHUB_REPOSITORY/actions/runs/$run_id" \
        2>/dev/null || true)
      if ! jq -e \
        --arg repository_id "$GITHUB_REPOSITORY_ID" \
        --arg sha "$GITHUB_PR_HEAD_SHA" \
        --arg workflow_path "$workflow_path" \
        --arg run_id "$run_id" '
          (.id | tostring) == $run_id and
          .head_sha == $sha and
          (.head_repository.id | tostring) == $repository_id and
          .path == $workflow_path and
          .event == "pull_request"
        ' >/dev/null 2>&1 <<< "$run"; then
        continue
      fi

      digest=$(jq -er '.digest' <<< "$candidate")
      size=$(jq -er '.size_in_bytes' <<< "$candidate")
      {
        echo "found=true"
        echo "artifact_id=$artifact_id"
        echo "digest=$digest"
        echo "size=$size"
        echo "run_id=$run_id"
        echo "waited_seconds=$(($(date -u +%s) - started))"
      } >> "$output_file"
      echo "Selected exact-merge release artifact $artifact_id from Runtime Checks run $run_id."
      exit 0
    done < <(
      jq -cer \
        --arg name "$artifact_name" \
        --arg repository_id "$GITHUB_REPOSITORY_ID" \
        --arg head_sha "$GITHUB_PR_HEAD_SHA" '
          [.artifacts[]
            | select(.name == $name)
            | select(.expired == false)
            | select((.id | type) == "number" and .id > 0)
            | select((.size_in_bytes | type) == "number" and .size_in_bytes > 0)
            | select((.digest | type) == "string" and (.digest | test("^sha256:[0-9a-f]{64}$")))
            | select(.workflow_run.head_sha == $head_sha)
            | select((.workflow_run.head_repository_id | tostring) == $repository_id)]
          | sort_by(.created_at)
          | reverse[]
        ' <<< "$artifacts" 2>/dev/null || true
    )
  fi

  elapsed=$(($(date -u +%s) - started))
  if (( elapsed >= max_wait_seconds )); then
    write_miss
    echo "No exact-commit TypeScript release artifact appeared after ${elapsed}s; using the local build fallback."
    exit 0
  fi
  remaining=$((max_wait_seconds - elapsed))
  (( remaining > 10 )) || sleep "$remaining"
  (( remaining <= 10 )) || sleep 10
done
