#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "usage: $0 OUTPUT_FILE [MAX_WAIT_SECONDS]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
output_file="$1"
max_wait_seconds="${2:-360}"
producer_max_wait_seconds="${PRODUCER_MAX_WAIT_SECONDS:-420}"
producer_artifact_grace_seconds="${PRODUCER_ARTIFACT_GRACE_SECONDS:-30}"
poll_seconds="${SELECTOR_POLL_SECONDS:-15}"

: "${GH_TOKEN:?GH_TOKEN must contain a short-lived Actions token}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GITHUB_REPOSITORY_ID:?GITHUB_REPOSITORY_ID must be set}"
: "${GITHUB_SHA:?GITHUB_SHA must be set}"
: "${GITHUB_PR_HEAD_SHA:?GITHUB_PR_HEAD_SHA must be set}"

[[ "$GITHUB_REPOSITORY_ID" =~ ^[1-9][0-9]*$ ]] || usage
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
[[ "$GITHUB_PR_HEAD_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
[[ "$max_wait_seconds" =~ ^[0-9]+$ ]] || usage
[[ "$producer_max_wait_seconds" =~ ^[0-9]+$ ]] || usage
[[ "$producer_artifact_grace_seconds" =~ ^[0-9]+$ ]] || usage
[[ "$poll_seconds" =~ ^[0-9]+$ ]] || usage

artifact_name="node-subtensor-release-$GITHUB_SHA"
workflow_path=.github/workflows/runtime-checks.yml
started=$(date -u +%s)
producer_completed_seen=
producer_run_id=
producer_job_status=
producer_job_conclusion=
last_producer_state=

write_miss() {
  {
    echo "found=false"
    echo "waited_seconds=$(($(date -u +%s) - started))"
  } >> "$output_file"
}

find_latest_producer_run() {
  local producer_runs
  producer_runs=$(gh api \
    -H 'Accept: application/vnd.github+json' \
    "repos/$GITHUB_REPOSITORY/actions/runs?event=pull_request&head_sha=$GITHUB_PR_HEAD_SHA&per_page=100" \
    2>/dev/null || true)
  jq -er \
    --arg repository_id "$GITHUB_REPOSITORY_ID" \
    --arg sha "$GITHUB_PR_HEAD_SHA" \
    --arg workflow_path "$workflow_path" '
      [.workflow_runs[]
        | select(.head_sha == $sha)
        | select((.head_repository.id | tostring) == $repository_id)
        | select(.path == $workflow_path)
        | select(.event == "pull_request")
        | select((.id | type) == "number" and .id > 0)]
      | sort_by(.created_at)
      | reverse
      | (.[0].id // empty)
    ' <<< "$producer_runs" 2>/dev/null || true
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

  if [[ -z "$producer_run_id" ]]; then
    producer_run_id=$(find_latest_producer_run)
  fi

  # Once discovered, latch the exact release-producing job. Transient API
  # failures must not make us forget a producer and cross back to the shorter
  # discovery timeout.
  if [[ -n "$producer_run_id" ]]; then
    producer_jobs=$(gh api \
      -H 'Accept: application/vnd.github+json' \
      "repos/$GITHUB_REPOSITORY/actions/runs/$producer_run_id/jobs?filter=latest&per_page=100" \
      2>/dev/null || true)
    producer_job=$(
      jq -cer '
        [.jobs[] | select(.name == "build release node")]
        | sort_by(.started_at // .created_at)
        | reverse
        | .[0]
      ' <<< "$producer_jobs" 2>/dev/null || true
    )
    current_status=$(jq -r '.status // empty' <<< "$producer_job" 2>/dev/null || true)
    if [[ -n "$current_status" ]]; then
      producer_job_status=$current_status
      producer_job_conclusion=$(jq -r '.conclusion // empty' <<< "$producer_job" 2>/dev/null || true)
    fi
  fi

  producer_state=not-found
  if [[ -n "$producer_run_id" ]]; then
    producer_state=${producer_job_status:-job-pending}
    [[ -z "$producer_job_conclusion" ]] || producer_state="$producer_state/$producer_job_conclusion"
  fi
  if [[ "$producer_state" != "$last_producer_state" ]]; then
    echo "Runtime Checks release producer: $producer_state"
    last_producer_state=$producer_state
  fi

  elapsed=$(($(date -u +%s) - started))
  if [[ "$producer_job_status" == completed && "$producer_job_conclusion" == success ]]; then
    [[ -n "$producer_completed_seen" ]] || producer_completed_seen=$(date -u +%s)
    completed_elapsed=$(($(date -u +%s) - producer_completed_seen))
    if (( completed_elapsed >= producer_artifact_grace_seconds )); then
      write_miss
      echo "Runtime Checks release producer completed without a verified artifact after ${completed_elapsed}s of API propagation grace; using the local build fallback."
      exit 0
    fi
  elif [[ "$producer_job_status" == completed ]]; then
    replacement_run_id=$(find_latest_producer_run)
    if [[ -n "$replacement_run_id" && "$replacement_run_id" != "$producer_run_id" ]]; then
      echo "Runtime Checks replaced producer run $producer_run_id with $replacement_run_id."
      producer_run_id=$replacement_run_id
      producer_job_status=
      producer_job_conclusion=
      producer_completed_seen=
      continue
    fi
    write_miss
    echo "Runtime Checks release producer completed (${producer_job_conclusion:-unknown}); using the local build fallback."
    exit 0
  elif [[ -n "$producer_run_id" ]]; then
    producer_completed_seen=
    if (( elapsed >= producer_max_wait_seconds )); then
      write_miss
      echo "Runtime Checks release producer was still ${producer_job_status:-pending} after ${elapsed}s; using the local build fallback."
      exit 0
    fi
  elif (( elapsed >= max_wait_seconds )); then
    write_miss
    echo "No exact-commit Runtime Checks producer appeared after ${elapsed}s; using the local build fallback."
    exit 0
  fi
  sleep "$poll_seconds"
done
