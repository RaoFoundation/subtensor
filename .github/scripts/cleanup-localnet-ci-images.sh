#!/usr/bin/env bash

set -euo pipefail

select_candidates() {
  local versions_file="$1"
  local cutoff="$2"

  jq -c --arg cutoff "$cutoff" '
    .[]
    | select(.created_at < $cutoff)
    | select(
        (.metadata.container.tags | length) == 0
        or all(.metadata.container.tags[]; test("^ci-[0-9a-f]{40}$"))
      )
  ' "$versions_file"
}

cleanup_package() {
  : "${PACKAGE_OWNER:?PACKAGE_OWNER is required}"
  : "${GH_TOKEN:?GH_TOKEN is required}"

  local package_name="subtensor-localnet-ci"
  local retention_days="${RETENTION_DAYS:-30}"
  local temp_dir="${RUNNER_TEMP:-/tmp}"
  local versions candidates cutoff deleted version id tags created_at

  [[ "$retention_days" =~ ^[0-9]+$ ]] || {
    echo "RETENTION_DAYS must be a non-negative integer: $retention_days" >&2
    return 1
  }

  versions="$(mktemp "$temp_dir/localnet-ci-versions.XXXXXX")"
  candidates="$(mktemp "$temp_dir/localnet-ci-candidates.XXXXXX")"
  trap "rm -f '$versions' '$candidates'" EXIT
  cutoff="$(date -u -d "$retention_days days ago" +%Y-%m-%dT%H:%M:%SZ)"

  gh api --paginate \
    "/orgs/$PACKAGE_OWNER/packages/container/$package_name/versions?per_page=100" \
    | jq -s 'add // []' >"$versions"
  select_candidates "$versions" "$cutoff" >"$candidates"

  deleted=0
  while IFS= read -r version; do
    id="$(jq -r '.id' <<<"$version")"
    tags="$(jq -r '.metadata.container.tags | join(",")' <<<"$version")"
    created_at="$(jq -r '.created_at' <<<"$version")"
    echo "Deleting version $id (tags=${tags:-untagged}, created=$created_at)"
    gh api --method DELETE \
      "/orgs/$PACKAGE_OWNER/packages/container/$package_name/versions/$id"
    deleted=$((deleted + 1))
  done <"$candidates"

  echo "Deleted $deleted expired $package_name image version(s)."
}

case "${1:-cleanup}" in
  select)
    [[ $# -eq 3 ]] || {
      echo "usage: $0 select VERSIONS_FILE CUTOFF" >&2
      exit 2
    }
    select_candidates "$2" "$3"
    ;;
  cleanup)
    [[ $# -eq 0 || $# -eq 1 ]] || {
      echo "usage: $0 [cleanup]" >&2
      exit 2
    }
    cleanup_package
    ;;
  *)
    echo "usage: $0 [cleanup] | select VERSIONS_FILE CUTOFF" >&2
    exit 2
    ;;
esac
