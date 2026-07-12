#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  snapshot-artifact.sh mode EVENT_NAME LABELS_JSON MANUAL_FRESH OUTPUT_FILE
  snapshot-artifact.sh select ARTIFACT_NAME BRANCH REPOSITORY_ID MAX_AGE_HOURS OUTPUT_FILE [required|optional]
  snapshot-artifact.sh download ARTIFACT_ID DIGEST DESTINATION
  snapshot-artifact.sh validate MANIFEST_FILE SNAPSHOT_FILE NETWORK GENESIS_HASH CLI_VERSION

For tests, set ARTIFACTS_JSON_FILE instead of calling the GitHub API,
ARTIFACT_ZIP_FILE instead of downloading an artifact, and NOW_EPOCH to
override the current time.
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
  [[ $# -eq 6 ]] || usage
  local artifact_name="$1"
  local branch="$2"
  local repository_id="$3"
  local max_age_hours="$4"
  local output_file="$5"
  local requirement="$6"
  local payload now candidate created_epoch age_seconds age_hours

  [[ "$repository_id" =~ ^[0-9]+$ ]] || { echo "invalid repository id: $repository_id" >&2; exit 2; }
  [[ "$max_age_hours" =~ ^[0-9]+$ ]] || { echo "invalid maximum age: $max_age_hours" >&2; exit 2; }
  [[ "$requirement" == required || "$requirement" == optional ]] || usage

  if [[ -n "${ARTIFACTS_JSON_FILE:-}" ]]; then
    payload=$(<"$ARTIFACTS_JSON_FILE")
  else
    : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
    payload=$(gh api \
      "repos/$GITHUB_REPOSITORY/actions/artifacts?name=$artifact_name&per_page=100")
  fi

  now="${NOW_EPOCH:-$(date -u +%s)}"
  [[ "$now" =~ ^[0-9]+$ ]] || { echo "invalid NOW_EPOCH: $now" >&2; exit 2; }

  candidate=$(jq -cer \
    --arg name "$artifact_name" \
    --arg branch "$branch" \
    --argjson repository_id "$repository_id" \
    --argjson now "$now" \
    --argjson max_age_seconds "$((max_age_hours * 3600))" '
      [
        .artifacts[]
        | select(.name == $name)
        | select(.expired == false)
        | select(.workflow_run.head_branch == $branch)
        | select(.workflow_run.repository_id == $repository_id)
        | select(.workflow_run.head_repository_id == $repository_id)
        | .created_epoch = (.created_at | fromdateiso8601)
        | select(.created_epoch <= $now)
        | select(($now - .created_epoch) <= $max_age_seconds)
      ]
      | sort_by(.created_epoch)
      | last // empty
    ' <<<"$payload" 2>/dev/null || true)

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
  set_output "$output_file" created-at "$(jq -er '.created_at' <<<"$candidate")"
  set_output "$output_file" age-hours "$age_hours"
  set_output "$output_file" size-bytes "$(jq -er '.size_in_bytes' <<<"$candidate")"
  set_output "$output_file" digest "$(jq -er '.digest // ""' <<<"$candidate")"

  if ((age_seconds > 36 * 3600)); then
    echo "::warning::$artifact_name is ${age_hours}h old; refresh is expected daily and this artifact becomes unusable after ${max_age_hours}h."
  fi
  echo "Selected $artifact_name artifact $(jq -er '.id' <<<"$candidate") from run $(jq -er '.workflow_run.id' <<<"$candidate") (${age_hours}h old)."
}

download_artifact() {
  [[ $# -eq 3 ]] || usage
  local artifact_id="$1"
  local digest="$2"
  local destination="$3"
  local archive actual_digest size_bytes concurrency parts_dir api_url download_url retry_delay
  local chunk_size download_started download_seconds assembly_started assembly_seconds

  [[ "$artifact_id" =~ ^[0-9]+$ ]] || { echo "invalid artifact id: $artifact_id" >&2; exit 2; }
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "invalid artifact digest: $digest" >&2; exit 2; }

  archive=$(mktemp)
  parts_dir=$(mktemp -d)
  trap 'rm -f "$archive"; rm -rf "$parts_dir"' EXIT
  if [[ -n "${ARTIFACT_ZIP_FILE:-}" ]]; then
    cp "$ARTIFACT_ZIP_FILE" "$archive"
  else
    : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
    : "${GH_TOKEN:?GH_TOKEN must be set}"
    concurrency="${ARTIFACT_DOWNLOAD_CONCURRENCY:-1}"
    [[ "$concurrency" =~ ^[1-9][0-9]*$ ]] && ((concurrency <= 64)) || {
      echo "invalid ARTIFACT_DOWNLOAD_CONCURRENCY: $concurrency" >&2
      exit 2
    }
    retry_delay="${ARTIFACT_RETRY_DELAY_SECONDS:-1}"
    [[ "$retry_delay" =~ ^[0-9]+$ ]] && ((retry_delay <= 60)) || {
      echo "invalid ARTIFACT_RETRY_DELAY_SECONDS: $retry_delay" >&2
      exit 2
    }
    size_bytes=$(gh api \
      -H 'Accept: application/vnd.github+json' \
      "repos/$GITHUB_REPOSITORY/actions/artifacts/$artifact_id" \
      --jq '.size_in_bytes')
    [[ "$size_bytes" =~ ^[1-9][0-9]*$ ]] || { echo "invalid artifact size: $size_bytes" >&2; exit 1; }
    api_url="https://api.github.com/repos/$GITHUB_REPOSITORY/actions/artifacts/$artifact_id/zip"

    get_download_url() {
      local headers status url
      headers=$(mktemp)
      status=$(curl --disable --silent --show-error \
        --connect-timeout 15 \
        --max-time 30 \
        --dump-header "$headers" \
        --output /dev/null \
        --max-redirs 0 \
        --header 'Accept: application/vnd.github+json' \
        --header "Authorization: Bearer $GH_TOKEN" \
        --header 'X-GitHub-Api-Version: 2022-11-28' \
        --write-out '%{http_code}' \
        "$api_url")
      if [[ "$status" != 302 ]]; then
        echo "artifact redirect returned HTTP $status" >&2
        rm -f "$headers"
        return 1
      fi
      url=$(awk 'tolower($0) ~ /^location:/ { sub(/^[^:]+:[[:space:]]*/, ""); sub(/\r$/, ""); print; exit }' "$headers")
      rm -f "$headers"
      [[ "$url" == https://* ]] || {
        echo "artifact redirect did not contain an HTTPS URL" >&2
        return 1
      }
      printf '%s' "$url"
    }

    download_range() {
      local start="$1" end="$2" part="$3" expected_size attempt url actual_size
      expected_size=$((end - start + 1))
      url="$download_url"
      for attempt in 1 2 3; do
        if ((attempt > 1)); then
          url=$(get_download_url) || continue
        fi
        if curl --disable --fail --silent --show-error \
          --connect-timeout 30 \
          --max-time 900 \
          --range "$start-$end" \
          --output "$part" \
          "$url"; then
          actual_size=$(file_size "$part")
          if [[ "$actual_size" == "$expected_size" ]]; then
            return 0
          fi
          echo "range $start-$end size mismatch: expected $expected_size, got $actual_size" >&2
        fi
        rm -f "$part"
        sleep "$((attempt * retry_delay))"
      done
      return 1
    }

    download_url=$(get_download_url)
    chunk_size=$(((size_bytes + concurrency - 1) / concurrency))
    download_started=$(date +%s)
    local pids=() parts=() failed=0 worker start end part pid index
    for ((worker = 0; worker < concurrency; worker++)); do
      start=$((worker * chunk_size))
      ((start < size_bytes)) || break
      end=$((start + chunk_size - 1))
      ((end < size_bytes)) || end=$((size_bytes - 1))
      printf -v part '%s/part-%03d' "$parts_dir" "$worker"
      parts+=("$part")
      download_range "$start" "$end" "$part" &
      pids+=("$!")
    done
    for pid in "${pids[@]}"; do
      wait "$pid" || failed=1
    done
    ((failed == 0)) || { echo "one or more artifact ranges failed" >&2; exit 1; }
    download_seconds=$(($(date +%s) - download_started))

    assembly_started=$(date +%s)
    for index in "${!parts[@]}"; do
      command cat "${parts[$index]}" >> "$archive"
    done
    assembly_seconds=$(($(date +%s) - assembly_started))
    [[ "$(file_size "$archive")" == "$size_bytes" ]] || { echo "artifact size mismatch" >&2; exit 1; }
    echo "Downloaded artifact $artifact_id in ${download_seconds}s with $concurrency ranges; assembled in ${assembly_seconds}s."
  fi

  actual_digest="sha256:$(sha256sum "$archive" | awk '{print $1}')"
  [[ "$actual_digest" == "$digest" ]] || {
    echo "artifact archive checksum mismatch: expected $digest, got $actual_digest" >&2
    exit 1
  }
  mkdir -p "$destination"
  unzip -q "$archive" -d "$destination"
  rm -f "$archive"
  rm -rf "$parts_dir"
  trap - EXIT
  echo "Downloaded and verified artifact $artifact_id."
}

validate_manifest() {
  [[ $# -eq 5 ]] || usage
  local manifest_file="$1"
  local snapshot_file="$2"
  local network="$3"
  local genesis_hash="$4"
  local cli_version="$5"
  local expected_file expected_size expected_sha actual_size actual_sha

  [[ -f "$manifest_file" ]] || { echo "missing snapshot manifest: $manifest_file" >&2; exit 1; }
  [[ -f "$snapshot_file" ]] || { echo "missing snapshot file: $snapshot_file" >&2; exit 1; }

  jq -e \
    --arg network "$network" \
    --arg genesis "$genesis_hash" \
    --arg cli "$cli_version" '
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
      (.producer_sha | test("^[0-9a-f]{40}$")) and
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
  download) download_artifact "$@" ;;
  validate) validate_manifest "$@" ;;
  *) usage ;;
esac
