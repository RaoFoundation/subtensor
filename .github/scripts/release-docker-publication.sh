#!/usr/bin/env bash
#
# Check or ensure that a release Docker image exists in GHCR with immutable
# source identity. A successful Actions run is not terminal state: dispatch
# inputs can select a different source than the workflow ref. Publication is
# complete only when the registry tag identifies the expected commit and tag.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  release-docker-publication.sh status WORKFLOW RELEASE_TAG EXPECTED_SHA
  release-docker-publication.sh ensure WORKFLOW RELEASE_TAG EXPECTED_SHA
EOF
  exit 2
}

[[ $# -ge 1 ]] || usage
mode="$1"
shift

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"

validate_common() {
  local workflow="$1"
  local release_tag="$2"
  local expected_sha="$3"
  local lowercase_repository

  [[ "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] \
    || { echo "invalid GitHub repository: $GITHUB_REPOSITORY" >&2; exit 2; }
  [[ "$release_tag" =~ ^v[0-9]+$ ]] \
    || { echo "invalid release tag: $release_tag" >&2; exit 2; }
  [[ "$expected_sha" =~ ^[0-9a-f]{40}$ ]] \
    || { echo "expected SHA must be a lowercase 40-byte Git SHA" >&2; exit 2; }
  lowercase_repository=$(printf '%s' "$GITHUB_REPOSITORY" | tr '[:upper:]' '[:lower:]')

  case "$workflow" in
    docker.yml)
      workflow_input=tag
      image_repository="$lowercase_repository"
      verify_latest=true
      ;;
    docker-localnet.yml)
      workflow_input=branch-or-tag
      image_repository="${lowercase_repository}-localnet"
      verify_latest=false
      ;;
    *)
      echo "unsupported release Docker workflow: $workflow" >&2
      exit 2
      ;;
  esac
}

registry_token() {
  curl -fsSL --get \
    --data-urlencode "scope=repository:$image_repository:pull" \
    https://ghcr.io/token \
    | jq -er '.token // .access_token'
}

registry_manifest() {
  local tag="$1"
  local token="$2"

  curl -fsSL \
    -H "Authorization: Bearer $token" \
    -H 'Accept: application/vnd.oci.image.index.v1+json' \
    "https://ghcr.io/v2/$image_repository/manifests/$tag"
}

tag_matches_source() {
  local tag="$1"
  local token="$2"
  local manifest revision version

  manifest=$(registry_manifest "$tag" "$token") || return 1
  revision=$(jq -er '.annotations["org.opencontainers.image.revision"]' \
    <<<"$manifest") || return 1
  version=$(jq -er '.annotations["org.opencontainers.image.version"]' \
    <<<"$manifest") || return 1
  [[ "$revision" == "$expected_sha" && "$version" == "$release_tag" ]]
}

publication_exists() {
  local token

  token=$(registry_token) || return 1
  tag_matches_source "$release_tag" "$token" || return 1
  if [[ "$verify_latest" == true ]]; then
    tag_matches_source latest "$token" || return 1
  fi
}

workflow_runs() {
  gh api --method GET \
    "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow/runs" \
    -f branch="$workflow_ref" \
    -F per_page=100
}

latest_matching_run_id() {
  local after_id="$1"

  workflow_runs \
    | jq -er \
      --arg title "$run_title" \
      --argjson after_id "$after_id" '
        [
          .workflow_runs[]
          | select(
              .event == "workflow_dispatch" and
              .display_title == $title and
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
    release_tag="$2"
    expected_sha="$3"
    ;;
  ensure)
    [[ $# -eq 3 ]] || usage
    workflow="$1"
    release_tag="$2"
    expected_sha="$3"
    : "${GH_TOKEN:?GH_TOKEN required in ensure mode}"
    ;;
  *)
    usage
    ;;
esac

validate_common "$workflow" "$release_tag" "$expected_sha"

if publication_exists; then
  echo "$image_repository:$release_tag is published from $expected_sha"
  exit 0
fi

if [[ "$mode" == status ]]; then
  echo "$image_repository:$release_tag is not published from $expected_sha" >&2
  exit 1
fi

max_attempts="${PUBLICATION_MAX_ATTEMPTS:-3}"
discovery_polls="${PUBLICATION_DISCOVERY_POLLS:-24}"
# Leave headroom for the caller's 180-minute job timeout so this helper can
# report a useful terminal error instead of being killed mid-poll.
completion_polls="${PUBLICATION_COMPLETION_POLLS:-660}"
publication_polls="${PUBLICATION_REGISTRY_POLLS:-12}"
poll_interval="${PUBLICATION_POLL_INTERVAL_SECONDS:-15}"
workflow_ref="${PUBLICATION_WORKFLOW_REF:-main}"
run_title="Release image $release_tag from $expected_sha"

[[ "$max_attempts" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_MAX_ATTEMPTS must be positive" >&2; exit 2; }
[[ "$discovery_polls" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_DISCOVERY_POLLS must be positive" >&2; exit 2; }
[[ "$completion_polls" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_COMPLETION_POLLS must be positive" >&2; exit 2; }
[[ "$publication_polls" =~ ^[1-9][0-9]*$ ]] \
  || { echo "PUBLICATION_REGISTRY_POLLS must be positive" >&2; exit 2; }
[[ "$poll_interval" =~ ^[0-9]+$ ]] \
  || { echo "PUBLICATION_POLL_INTERVAL_SECONDS must be non-negative" >&2; exit 2; }
[[ "$workflow_ref" =~ ^[A-Za-z0-9._/-]+$ ]] \
  || { echo "invalid workflow ref: $workflow_ref" >&2; exit 2; }

attempt=1
while (( attempt <= max_attempts )); do
  before_id=$(workflow_runs \
    | jq -er \
      --arg title "$run_title" '
        [
          .workflow_runs[]
          | select(
              .event == "workflow_dispatch" and
              .display_title == $title
            )
          | .id
        ]
        | max // 0
      ')

  echo "Dispatching $workflow for $release_tag (attempt $attempt/$max_attempts)"
  gh workflow run "$workflow" \
    --repo "$GITHUB_REPOSITORY" \
    --ref "$workflow_ref" \
    -f "$workflow_input=$release_tag" \
    -f "expected-sha=$expected_sha"

  run_id=""
  poll=1
  while (( poll <= discovery_polls )); do
    if run_id=$(latest_matching_run_id "$before_id"); then
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
    display_title=$(jq -er '.display_title' <<<"$run_json")
    [[ "$run_event" == "workflow_dispatch" ]] \
      || { echo "run $run_id has unexpected event: $run_event" >&2; exit 1; }
    [[ "$run_path" == ".github/workflows/$workflow" ]] \
      || { echo "run $run_id has unexpected workflow: $run_path" >&2; exit 1; }
    [[ "$display_title" == "$run_title" ]] \
      || { echo "run $run_id has unexpected title: $display_title" >&2; exit 1; }

    if [[ "$(jq -er '.status' <<<"$run_json")" == completed ]]; then
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
      for (( poll = 1; poll <= publication_polls; poll++ )); do
        if publication_exists; then
          echo "$image_repository:$release_tag is published from $expected_sha in run $run_id"
          exit 0
        fi
        sleep "$poll_interval"
      done
      echo "$workflow run $run_id succeeded without publishing the expected image identity" >&2
      exit 1
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

echo "$workflow did not publish $release_tag after $max_attempts attempts" >&2
exit 1
