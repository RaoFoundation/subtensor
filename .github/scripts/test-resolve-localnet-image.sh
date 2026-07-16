#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
resolver="$repo_root/.github/scripts/resolve-localnet-image.sh"

assert_case() {
  local description="$1"
  local expected_tag="$2"
  local expected_ref="$3"
  local expected_latest="$4"
  shift 4

  local output
  output="$(mktemp)"
  env -i PATH="$PATH" HOME="${HOME:-/tmp}" "$@" "$resolver" "$output"

  grep -qxF "tag=$expected_tag" "$output" || {
    echo "$description: unexpected tag" >&2
    sed -n '1,20p' "$output" >&2
    exit 1
  }
  grep -qxF "ref=$expected_ref" "$output" || {
    echo "$description: unexpected ref" >&2
    sed -n '1,20p' "$output" >&2
    exit 1
  }
  grep -qxF "latest_tag=$expected_latest" "$output" || {
    echo "$description: unexpected latest policy" >&2
    sed -n '1,20p' "$output" >&2
    exit 1
  }
  rm -f "$output"
}

common=(EVENT_SHA=0123456789abcdef REF_NAME=main)

assert_case "main push" main 0123456789abcdef true \
  env EVENT_NAME=push "${common[@]}"
assert_case "devnet push" devnet 0123456789abcdef false \
  env EVENT_NAME=push EVENT_SHA=0123456789abcdef REF_NAME=devnet
assert_case "testnet push" testnet 0123456789abcdef false \
  env EVENT_NAME=push EVENT_SHA=0123456789abcdef REF_NAME=testnet
assert_case "release" v431 0123456789abcdef false \
  env EVENT_NAME=release EVENT_SHA=0123456789abcdef REF_NAME=v431
assert_case "manual feature ref" feat-docker feat/docker false \
  env EVENT_NAME=workflow_dispatch "${common[@]}" BRANCH_OR_TAG=feat/docker
assert_case "manual invalid-leading ref" ref--candidate-x -candidate/x false \
  env EVENT_NAME=workflow_dispatch "${common[@]}" BRANCH_OR_TAG=-candidate/x
assert_case "manual main" main main true \
  env EVENT_NAME=workflow_dispatch "${common[@]}" BRANCH_OR_TAG=main
assert_case "PR build" pr-2898 fedcba9876543210 false \
  env EVENT_NAME=workflow_dispatch "${common[@]}" PR_NUMBER=2898 \
    PR_HEAD_SHA=fedcba9876543210 PR_HEAD_REF=feature/example

if env -i PATH="$PATH" HOME="${HOME:-/tmp}" EVENT_NAME=workflow_dispatch \
  EVENT_SHA=0123456789abcdef REF_NAME=main PR_NUMBER=not-a-number \
  "$resolver" "$(mktemp)" >/dev/null 2>&1; then
  echo "invalid PR number unexpectedly succeeded" >&2
  exit 1
fi

echo "localnet image tag policy tests passed"
