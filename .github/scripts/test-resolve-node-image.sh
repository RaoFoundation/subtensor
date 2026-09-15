#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
resolver="$script_dir/resolve-node-image.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

run_case() {
  local name="$1"
  local input_tag="$2"
  local source_ref="$3"
  local expected_tag="$4"
  local expected_latest="$5"
  local output="$tmp/$name"

  GITHUB_REPOSITORY=RaoFoundation/subtensor \
    INPUT_TAG="$input_tag" \
    SOURCE_REF="$source_ref" \
    "$resolver" "$output"

  grep -qxF "tag=$expected_tag" "$output"
  grep -qxF "latest_tag=$expected_latest" "$output"
  grep -qxF "image=ghcr.io/raofoundation/subtensor" "$output"
}

run_case main main refs/heads/main main true
run_case stale-main main refs/tags/v448 main false
run_case testnet testnet refs/heads/testnet testnet false
run_case release v448 refs/tags/v448 v448 false
run_case feature feature/example refs/heads/feature/example feature-example false

workflow="$script_dir/../workflows/docker.yml"
grep -qF 'branches: [main, devnet, testnet]' "$workflow"
grep -qF 'run: ./.github/scripts/resolve-node-image.sh' "$workflow"
grep -qF "env.latest_tag == 'true'" "$workflow"
grep -qF "cancel-in-progress: \${{ github.ref != 'refs/heads/main' }}" "$workflow"
grep -qF 'current_main=$(gh api "repos/$GITHUB_REPOSITORY/git/ref/heads/main"' "$workflow"

publish_job=$(sed -n '/^  publish:/,$p' "$workflow")
checkout_line=$(grep -nF 'ref: ${{ needs.setup.outputs.sha }}' <<<"$publish_job" | head -n 1 | cut -d: -f1)
resolver_line=$(grep -nF 'run: ./.github/scripts/resolve-node-image.sh' <<<"$publish_job" | cut -d: -f1)
[[ "$checkout_line" -lt "$resolver_line" ]] || {
  echo "publish job must check out the pinned source before running its resolver" >&2
  exit 1
}

head_check_line=$(grep -nF 'name: Verify current main revision' <<<"$publish_job" | cut -d: -f1)
push_line=$(grep -nF 'name: Build and push' <<<"$publish_job" | cut -d: -f1)
[[ "$head_check_line" -lt "$push_line" ]] || {
  echo "publish job must reject stale main revisions before pushing" >&2
  exit 1
}

echo "node image tag policy checks passed"
