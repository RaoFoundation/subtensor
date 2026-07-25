#!/usr/bin/env bash
#
# Check or ensure that a release-tagged Docker workflow completed successfully
# for an exact source commit. `gh workflow run` only confirms that GitHub
# accepted a dispatch request; the child workflow can still fail before any job
# starts. Ensure mode follows the child run to completion and retries transient
# startup/discovery failures.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  release-docker-publication.sh status WORKFLOW REF EXPECTED_SHA
  release-docker-publication.sh ensure WORKFLOW REF INPUT_NAME INPUT_VALUE EXPECTED_SHA
EOF
  exit 2
}

[[ $# -ge 1 ]] || usage
mode="$1"
shift

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"
: "${GH_TOKEN:?GH_TOKEN required}"

validate_common() {
  local workflow="$1"
  local ref="$2"
  local expected_sha="$3"

  [[ "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] \
    || { echo "invalid GitHub repository: $GITHUB_REPOSITORY" >&2; exit 2; }
  [[ "$workflow" =~ ^[A-Za-z0-9_.-]+\.ya?ml$ ]] \
    || { echo "invalid workflow filename: $workflow" >&2; exit 2; }
  [[ "$ref" =~ ^[A-Za-z0-9._/-]+$ ]] \
    || { echo "invalid workflow ref: $ref" >&2; exit 2; }
  [[ "$expected_sha" =~ ^[0-9a-f]{40}$ ]] \
    || { echo "expected SHA must be a lowercase 40-byte Git SHA" >&2; exit 2; }
}

workflow_runs() {
  local workflow="$1"
  local ref="$2"

  gh api --method GET \
    "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow/runs" \
    -f branch="$ref" \
    -F per_page=100
}

successful_run_id() {
  local workflow="$1"
  local ref="$2"
  local expected_sha="$3"

  workflow_runs "$workflow" "$ref" \
    | jq -er --arg sha "$expected_sha" '
        [
          .workflow_runs[]
          | select(
              (.event == "workflow_dispatch" or .event == "release") and
              .head_sha == $sha and
              .status == "completed" and
              .conclusion == "success"
            )
        ]
        | if length == 0 then empty else max_by(.id).id end
      ' 2>/dev/null
}

latest_matching_run_id() {
  local workflow="$1"
  local ref="$2"
  local expected_sha="$3"
  local after_id="$4"

  workflow_runs "$workflow" "$ref" \
    | jq -er --arg sha "$expected_sha" --argjson after_id "$after_id" '
        [
          .workflow_runs[]
          | select(
              .event == "workflow_dispatch" and
              .head_sha == $sha and
              .id > $after_id
            )
        ]
        | if length == 0 then empty else max_by(.id).id end
      ' 2>/dev/null
}

case "$mode" in
  status)
    [[ $# -eq 3 ]] || usage
    workflow="$1"
    ref="$2"
    expected_sha="$3"
    validate_common "$workflow" "$ref" "$expected_sha"

    if run_id=$(successful_run_id "$workflow" "$ref" "$expected_sha"); then
      echo "$workflow already published $ref from $expected_sha in run $run_id"
      exit 0
    fi
    echo "$workflow has no successful $ref publication for $expected_sha" >&2
    exit 1
    ;;

  ensure)
    [[ $# -eq 5 ]] || usage
    workflow="$1"
    ref="$2"
    input_name="$3"
    input_value="$4"
    expected_sha="$5"
    validate_common "$workflow" "$ref" "$expected_sha"
    [[ "$input_name" =~ ^[A-Za-z0-9_-]+$ ]] \
      || { echo "invalid workflow input name: $input_name" >&2; exit 2; }
    [[ "$input_value" =~ ^[A-Za-z0-9._/-]+$ ]] \
      || { echo "invalid workflow input value: $input_value" >&2; exit 2; }
    ;;

  *)
    usage
    ;;
esac

if run_id=$(successful_run_id "$workflow" "$ref" "$expected_sha"); then
  echo "$workflow already published $ref from $expected_sha in run $run_id"
  exit 0
fi

max_attempts="${PUBLICATION_MAX_ATTEMPTS:-3}"
discovery_polls="${PUBLICATION_DISCOVERY_POLLS:-24}"
completion_polls="${PUBLICATION_COMPLETION_POLLS:-720}"
poll_interval="${PUBLICATION_POLL_INTERVAL_SECONDS:-15}"

[[ "$max_attempts" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_MAX_ATTEMPTS must be positive" >&2; exit 2; }
[[ "$discovery_polls" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_DISCOVERY_POLLS must be positive" >&2; exit 2; }
[[ "$completion_polls" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_COMPLETION_POLLS must be positive" >&2; exit 2; }
[[ "$poll_interval" =~ ^[0-9]+$ ]] \
  || { echo "PUBLICATION_POLL_INTERVAL_SECONDS must be non-negative" >&2; exit 2; }

attempt=1
while (( attempt <= max_attempts )); do
  before_id=$(workflow_runs "$workflow" "$ref" \
    | jq -er --arg sha "$expected_sha" '
        [
          .workflow_runs[]
          | select(.event == "workflow_dispatch" and .head_sha == $sha)
          | .id
        ]
        | max // 0
      ')

  echo "Dispatching $workflow for $ref (attempt $attempt/$max_attempts)"
  gh workflow run "$workflow" \
    --repo "$GITHUB_REPOSITORY" \
    --ref "$ref" \
    -f "$input_name=$input_value"

  run_id=""
  poll=1
  while (( poll <= discovery_polls )); do
    if run_id=$(latest_matching_run_id \
        "$workflow" "$ref" "$expected_sha" "$before_id"); then
      break
    fi
    sleep "$poll_interval"
    poll=$(( poll + 1 ))
  done

  if [[ -z "$run_id" ]]; then
    echo "::warning::$workflow dispatch did not produce a discoverable run; retrying"
    attempt=$(( attempt + 1 ))
    continue
  fi

  echo "Following $workflow run $run_id"
  poll=1
  conclusion=""
  while (( poll <= completion_polls )); do
    run_json=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run_id")
    run_event=$(jq -er '.event' <<<"$run_json")
    run_path=$(jq -er '.path' <<<"$run_json")
    run_sha=$(jq -er '.head_sha' <<<"$run_json")
    [[ "$run_event" == "workflow_dispatch" ]] \
      || { echo "run $run_id has unexpected event: $run_event" >&2; exit 1; }
    [[ "$run_path" == ".github/workflows/$workflow" ]] \
      || { echo "run $run_id has unexpected workflow: $run_path" >&2; exit 1; }
    [[ "$run_sha" == "$expected_sha" ]] \
      || { echo "run $run_id source $run_sha != $expected_sha" >&2; exit 1; }

    if [[ "$(jq -er '.status' <<<"$run_json")" == "completed" ]]; then
      conclusion=$(jq -er '.conclusion' <<<"$run_json")
      break
    fi
    sleep "$poll_interval"
    poll=$(( poll + 1 ))
  done

  if [[ -z "$conclusion" ]]; then
    echo "$workflow run $run_id did not complete within the polling window" >&2
    exit 1
  fi

  case "$conclusion" in
    success)
      echo "$workflow successfully published $ref from $expected_sha in run $run_id"
      exit 0
      ;;
    startup_failure)
      echo "::warning::$workflow run $run_id failed before jobs started; retrying"
      attempt=$(( attempt + 1 ))
      ;;
    *)
      echo "$workflow run $run_id concluded with $conclusion; not retrying a build failure" >&2
      exit 1
      ;;
  esac
done

echo "$workflow did not publish $ref after $max_attempts attempts" >&2
exit 1
