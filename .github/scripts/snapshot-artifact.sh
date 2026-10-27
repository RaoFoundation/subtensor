#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  snapshot-artifact.sh mode EVENT_NAME LABELS_JSON MANUAL_FRESH OUTPUT_FILE
  snapshot-artifact.sh select ARTIFACT_NAME BRANCH REPOSITORY_ID WORKFLOW_PATH MAX_AGE_HOURS OUTPUT_FILE [required|optional]
  snapshot-artifact.sh validate MANIFEST_FILE SNAPSHOT_FILE NETWORK GENESIS_HASH CLI_VERSION PRODUCER_SHA

For tests, set ARTIFACTS_JSON_FILE and WORKFLOW_RUNS_JSON_FILE instead of
calling the GitHub API and NOW_EPOCH to override the current time.
EOF
  exit 2
}

set_output() {
  local output_file="$1"
  local name="$2"
  local value="$3"
  printf '%s=%s\n' "$name" "$value" >> "$output_file"
}

file_size() {
  stat -c '%s' "$1" 2>/dev/null || stat -f '%z' "$1"
}

resolve_mode() {
  [[ $# -eq 4 ]] || usage
  local event_name="$1"
  local labels_json="$2"
  local manual_fresh="$3"
  local output_file="$4"
  local fresh=false

  [[ "$labels_json" != null ]] || labels_json='[]'
  [[ "$manual_fresh" == true || "$manual_fresh" == false ]] || {
    echo "invalid manual fresh-state value: $manual_fresh" >&2
    exit 2
  }
  jq -e 'type == "array" and all(.[]; type == "string")' <<<"$labels_json" >/dev/null || {
    echo "invalid labels JSON: $labels_json" >&2
    exit 2
  }

  if [[ "$event_name" == pull_request ]] &&
    jq -e 'index("fresh-try-runtime-state") != null' <<<"$labels_json" >/dev/null; then
    fresh=true
  elif [[ "$event_name" == workflow_dispatch && "$manual_fresh" == true ]]; then
    fresh=true
  fi

  set_output "$output_file" fresh-state "$fresh"
  if [[ "$fresh" == true ]]; then
    echo "Explicit live-state bypass selected."
  else
    echo "Fail-closed cached-state mode selected."
  fi
}

select_artifact() {
  [[ $# -eq 7 ]] || usage
  local artifact_name="$1"
  local branch="$2"
  local repository_id="$3"
  local workflow_path="$4"
  local max_age_hours="$5"
  local output_file="$6"
  local requirement="$7"
  local payload workflow_runs workflow_file now candidate created_epoch age_seconds age_hours

  [[ "$repository_id" =~ ^[0-9]+$ ]] || { echo "invalid repository id: $repository_id" >&2; exit 2; }
  [[ "$workflow_path" =~ ^\.github/workflows/[A-Za-z0-9._-]+\.ya?ml$ ]] || {
    echo "invalid workflow path: $workflow_path" >&2
    exit 2
  }
  [[ "$max_age_hours" =~ ^[0-9]+$ ]] || { echo "invalid maximum age: $max_age_hours" >&2; exit 2; }
  [[ "$requirement" == required || "$requirement" == optional ]] || usage

  if [[ -n "${ARTIFACTS_JSON_FILE:-}" ]]; then
    payload=$(<"$ARTIFACTS_JSON_FILE")
  else
    : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
    payload=$(gh api \
      "repos/$GITHUB_REPOSITORY/actions/artifacts?name=$artifact_name&per_page=100")
  fi
  if [[ -n "${WORKFLOW_RUNS_JSON_FILE:-}" ]]; then
    workflow_runs=$(<"$WORKFLOW_RUNS_JSON_FILE")
  else
    workflow_file="${workflow_path##*/}"
    workflow_runs=$(gh api --method GET \
      "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow_file/runs" \
      -f branch="$branch" -F per_page=100 -F exclude_pull_requests=true \
      --jq '{workflow_runs: [.workflow_runs[] | {
        id, path, head_branch, head_sha,
        repository: {id: .repository.id},
        head_repository: {id: .head_repository.id}
      }]}')
  fi

  now="${NOW_EPOCH:-$(date -u +%s)}"
  [[ "$now" =~ ^[0-9]+$ ]] || { echo "invalid NOW_EPOCH: $now" >&2; exit 2; }

  candidate=$(jq -ncer \
    --arg name "$artifact_name" \
    --arg branch "$branch" \
    --arg workflow_path "$workflow_path" \
    --argjson repository_id "$repository_id" \
    --argjson now "$now" \
    --argjson max_age_seconds "$((max_age_hours * 3600))" \
    --argjson artifacts "$payload" \
    --argjson workflow_runs "$workflow_runs" '
      ($workflow_runs.workflow_runs
        | map(select(
            .path == $workflow_path and
            .head_branch == $branch and
            .repository.id == $repository_id and
            .head_repository.id == $repository_id and
            (.head_sha | test("^[0-9a-f]{40}$"))
          ))
        | map({key: (.id | tostring), value: .head_sha})
        | from_entries) as $trusted_runs
      |
      [
        $artifacts.artifacts[]
        | select(.name == $name)
        | select(.expired == false)
        | select((.size_in_bytes | type) == "number" and .size_in_bytes > 0)
        | select((.digest | type) == "string" and (.digest | test("^sha256:[0-9a-f]{64}$")))
        | select(.workflow_run.head_branch == $branch)
        | select(.workflow_run.repository_id == $repository_id)
        | select(.workflow_run.head_repository_id == $repository_id)
        | select($trusted_runs[(.workflow_run.id | tostring)] == .workflow_run.head_sha)
        | .created_epoch = (.created_at | fromdateiso8601)
        | select(.created_epoch <= $now)
        | select(($now - .created_epoch) <= $max_age_seconds)
      ]
      | sort_by(.created_epoch)
      | last // empty
    ' 2>/dev/null || true)

  if [[ -z "$candidate" ]]; then
    set_output "$output_file" found false
    if [[ "$requirement" == optional ]]; then
      echo "No usable $artifact_name artifact found; optional restore will be skipped."
      return 0
    fi
    echo "::error::No non-expired $artifact_name artifact from $branch is at most ${max_age_hours}h old. Dispatch Refresh Mainnet Snapshot, or explicitly request fresh live state."
    return 1
  fi

  created_epoch=$(jq -er '.created_epoch' <<<"$candidate")
  age_seconds=$((now - created_epoch))
  age_hours=$((age_seconds / 3600))

  set_output "$output_file" found true
  set_output "$output_file" artifact-id "$(jq -er '.id' <<<"$candidate")"
  set_output "$output_file" run-id "$(jq -er '.workflow_run.id' <<<"$candidate")"
  set_output "$output_file" producer-sha "$(jq -er '.workflow_run.head_sha' <<<"$candidate")"
  set_output "$output_file" artifact-size-bytes "$(jq -er '.size_in_bytes' <<<"$candidate")"
  set_output "$output_file" artifact-digest "$(jq -er '.digest' <<<"$candidate")"
  set_output "$output_file" created-at "$(jq -er '.created_at' <<<"$candidate")"
  set_output "$output_file" age-hours "$age_hours"
  if ((age_seconds > 36 * 3600)); then
    echo "::warning::$artifact_name is ${age_hours}h old; refresh is expected daily and this artifact becomes unusable after ${max_age_hours}h."
  fi
  echo "Selected $artifact_name artifact $(jq -er '.id' <<<"$candidate") from run $(jq -er '.workflow_run.id' <<<"$candidate") (${age_hours}h old)."
}

validate_manifest() {
  [[ $# -eq 6 ]] || usage
  local manifest_file="$1"
  local snapshot_file="$2"
  local network="$3"
  local genesis_hash="$4"
  local cli_version="$5"
  local producer_sha="$6"
  local expected_file expected_size expected_sha actual_size actual_sha

  [[ -f "$manifest_file" ]] || { echo "missing snapshot manifest: $manifest_file" >&2; exit 1; }
  [[ -f "$snapshot_file" ]] || { echo "missing snapshot file: $snapshot_file" >&2; exit 1; }

  jq -e \
    --arg network "$network" \
    --arg genesis "$genesis_hash" \
    --arg cli "$cli_version" \
    --arg producer_sha "$producer_sha" '
      .schema_version == 1 and
      .kind == "try-runtime-state" and
      .network == $network and
      .genesis_hash == $genesis and
      .try_runtime_cli_version == $cli and
      (.finalized_block_hash | test("^0x[0-9a-f]{64}$")) and
      (.finalized_block_number | type == "number") and
      (.source_spec_name | type == "string" and length > 0) and
      (.source_spec_version | type == "number") and
      (.created_at | fromdateiso8601 | type == "number") and
      .producer_sha == $producer_sha and
      (.snapshot_file | type == "string" and length > 0) and
      (.snapshot_size_bytes | type == "number") and
      (.snapshot_sha256 | test("^[0-9a-f]{64}$"))
    ' "$manifest_file" >/dev/null || {
      echo "snapshot manifest contract validation failed: $manifest_file" >&2
      exit 1
    }

  expected_file=$(jq -er '.snapshot_file' "$manifest_file")
  [[ "$expected_file" == "$(basename "$snapshot_file")" ]] || {
    echo "manifest expects $expected_file, downloaded $(basename "$snapshot_file")" >&2
    exit 1
  }

  expected_size=$(jq -er '.snapshot_size_bytes' "$manifest_file")
  actual_size=$(file_size "$snapshot_file")
  [[ "$actual_size" == "$expected_size" ]] || {
    echo "snapshot size mismatch: expected $expected_size, got $actual_size" >&2
    exit 1
  }

  expected_sha=$(jq -er '.snapshot_sha256' "$manifest_file")
  actual_sha=$(sha256sum "$snapshot_file" | awk '{print $1}')
  [[ "$actual_sha" == "$expected_sha" ]] || {
    echo "snapshot checksum mismatch: expected $expected_sha, got $actual_sha" >&2
    exit 1
  }

  echo "Validated $network snapshot at block $(jq -er '.finalized_block_number' "$manifest_file") ($(jq -er '.finalized_block_hash' "$manifest_file"))."
}

command="${1:-}"
shift || true
case "$command" in
  mode) resolve_mode "$@" ;;
  select) select_artifact "$@" ;;
  validate) validate_manifest "$@" ;;
  *) usage ;;
esac
