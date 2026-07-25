#!/usr/bin/env bash
#
# Reconcile release Docker publication against GHCR. Registry annotations are
# the terminal state; Actions metadata is used only to avoid dispatching work
# that is already active. The scheduled release watcher retries any missing
# terminal state on its next run.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  release-docker-publication.sh status WORKFLOW RELEASE_TAG EXPECTED_SHA
  release-docker-publication.sh ensure WORKFLOW RELEASE_TAG EXPECTED_SHA
EOF
  exit 2
}

[[ $# -eq 4 ]] || usage
mode="$1"
workflow="$2"
release_tag="$3"
expected_sha="$4"

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"

case "$mode" in
  status) ;;
  ensure) : "${GH_TOKEN:?GH_TOKEN required in ensure mode}" ;;
  *) usage ;;
esac

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
    publication_tags=("$release_tag" latest)
    ;;
  docker-localnet.yml)
    workflow_input=branch-or-tag
    image_repository="${lowercase_repository}-localnet"
    publication_tags=("$release_tag")
    ;;
  *)
    echo "unsupported release Docker workflow: $workflow" >&2
    exit 2
    ;;
esac

registry_token() {
  curl -fsSL --get \
    --data-urlencode "scope=repository:$image_repository:pull" \
    https://ghcr.io/token \
    | jq -er '.token // .access_token'
}

tag_matches_source() {
  local tag="$1"
  local token="$2"
  local response http_code manifest jq_status

  response=$(curl -sS -w $'\n%{http_code}' \
    -H "Authorization: Bearer $token" \
    -H 'Accept: application/vnd.oci.image.index.v1+json' \
    "https://ghcr.io/v2/$image_repository/manifests/$tag") || return 2
  http_code="${response##*$'\n'}"
  manifest="${response%$'\n'*}"
  case "$http_code" in
    200) ;;
    404) return 1 ;;
    *)
      echo "GHCR returned HTTP $http_code for $image_repository:$tag" >&2
      return 2
      ;;
  esac

  if jq -e \
    --arg revision "$expected_sha" \
    --arg version "$release_tag" '
      .annotations["org.opencontainers.image.revision"] == $revision and
      .annotations["org.opencontainers.image.version"] == $version
    ' <<<"$manifest" >/dev/null; then
    return 0
  else
    jq_status=$?
    [[ "$jq_status" -eq 1 ]] && return 1
    echo "GHCR returned malformed metadata for $image_repository:$tag" >&2
    return 2
  fi
}

publication_exists() {
  local token status tag

  token=$(registry_token) || return 2
  for tag in "${publication_tags[@]}"; do
    tag_matches_source "$tag" "$token" || {
      status=$?
      return "$status"
    }
  done
}

publication_status=0
publication_exists || publication_status=$?
if [[ "$publication_status" -eq 0 ]]; then
  echo "$image_repository:$release_tag is published from $expected_sha"
  exit 0
fi
if [[ "$publication_status" -ne 1 ]]; then
  echo "could not determine GHCR publication state" >&2
  exit 2
fi

if [[ "$mode" == status ]]; then
  echo "$image_repository:$release_tag is not published from $expected_sha" >&2
  exit 1
fi

workflow_ref="${PUBLICATION_WORKFLOW_REF:-main}"
[[ "$workflow_ref" =~ ^[A-Za-z0-9._/-]+$ ]] \
  || { echo "invalid workflow ref: $workflow_ref" >&2; exit 2; }
run_title="Release image $release_tag from $expected_sha"

runs_json=$(gh api --method GET \
  "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow/runs" \
  -f branch="$workflow_ref" \
  -f event=workflow_dispatch \
  -F per_page=100)
active_run_id=$(jq -r --arg title "$run_title" '
  [
    .workflow_runs[]
    | select(.display_title == $title and .status != "completed")
  ]
  | if length == 0 then empty else max_by(.id).id end
' <<<"$runs_json")

if [[ -n "$active_run_id" ]]; then
  echo "$workflow run $active_run_id is already publishing $release_tag from $expected_sha"
  exit 0
fi

gh workflow run "$workflow" \
  --repo "$GITHUB_REPOSITORY" \
  --ref "$workflow_ref" \
  -f "$workflow_input=$release_tag" \
  -f "expected-sha=$expected_sha"
echo "Dispatched $workflow for $release_tag from $expected_sha"
