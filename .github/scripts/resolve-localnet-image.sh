#!/usr/bin/env bash

set -euo pipefail

output_file="${1:-${GITHUB_OUTPUT:-}}"
: "${output_file:?pass an output file or set GITHUB_OUTPUT}"
: "${EVENT_NAME:?EVENT_NAME is required}"
: "${REF_NAME:?REF_NAME is required}"
: "${EVENT_SHA:?EVENT_SHA is required}"

branch_or_tag="${BRANCH_OR_TAG:-}"
pr_number="${PR_NUMBER:-}"
pr_head_sha="${PR_HEAD_SHA:-}"
pr_head_ref="${PR_HEAD_REF:-}"

sanitize_tag() {
  local value="$1"
  value="${value//[^a-zA-Z0-9._-]/-}"
  if [[ ! "$value" =~ ^[a-zA-Z0-9_] ]]; then
    value="ref-${value}"
  fi
  printf '%.128s' "$value"
}

if [[ -n "$pr_number" ]]; then
  [[ "$pr_number" =~ ^[0-9]+$ ]] || {
    echo "PR_NUMBER must contain digits only: $pr_number" >&2
    exit 1
  }
  tag="pr-${pr_number}"
  ref="${pr_head_sha:-${pr_head_ref:-${branch_or_tag:-main}}}"
  latest_tag=false
else
  source_tag="${branch_or_tag:-$REF_NAME}"
  tag="$(sanitize_tag "$source_tag")"
  ref="${branch_or_tag:-$EVENT_SHA}"
  if [[ "$tag" == main ]]; then
    latest_tag=true
  else
    latest_tag=false
  fi
fi

{
  echo "tag=$tag"
  echo "ref=$ref"
  echo "latest_tag=$latest_tag"
} >>"$output_file"
