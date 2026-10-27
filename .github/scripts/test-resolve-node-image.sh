#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
resolver="$script_dir/resolve-node-image.sh"
image=ghcr.io/raofoundation/subtensor
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

run_case() {
  local name="$1"
  local input_tag="$2"
  local release_tag="$3"
  local expected_tags="$4"
  local expected_mainnet="$5"
  local output="$tmp/$name"

  GITHUB_REPOSITORY=RaoFoundation/subtensor \
    INPUT_TAG="$input_tag" \
    RELEASE_TAG="$release_tag" \
    "$resolver" "$output"

  grep -qxF "tags=$expected_tags" "$output" || {
    echo "$name: unexpected tags" >&2
    cat "$output" >&2
    exit 1
  }
  grep -qxF "mainnet_release=$expected_mainnet" "$output"
}

expect_failure() {
  local name="$1"
  local input_tag="$2"
  local release_tag="$3"

  if GITHUB_REPOSITORY=RaoFoundation/subtensor \
      INPUT_TAG="$input_tag" \
      RELEASE_TAG="$release_tag" \
      "$resolver" "$tmp/$name" >/dev/null 2>&1; then
    echo "$name: unexpectedly succeeded" >&2
    exit 1
  fi
}

# Merged but undeployed code never becomes :latest.
run_case main main "" "$image:main" false
run_case testnet testnet "" "$image:testnet" false
run_case feature feature/example "" "$image:feature-example" false
run_case manual-release v470 "" "$image:v470" false
# Only the verified mainnet mirror publishes the release tag and :latest.
run_case mainnet mainnet v470 "$image:mainnet,$image:v470,$image:latest" true

expect_failure mainnet-without-release mainnet ""
expect_failure mainnet-malformed-release mainnet "470"
expect_failure release-tag-off-mainnet main v470

echo "node image tag policy checks passed"
