#!/usr/bin/env bash
# Resolves the immutable release tag for an image built from the mainnet
# mirror. The mirror moves only after watch-mainnet-release.yml verifies the
# finalized on-chain runtime. Before anything is published as a release image,
# re-check that the built commit is still the mirror head and carries the
# lightweight v<spec_version> tag reserved by the release train.

set -euo pipefail

source_dir="${1:?usage: resolve-mainnet-release.sh <source-dir> [output-file]}"
output_file="${2:-${GITHUB_OUTPUT:-}}"
: "${output_file:?pass an output file or set GITHUB_OUTPUT}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"

sha=$(git -C "$source_dir" rev-parse HEAD)
spec=$(grep -m 1 -Eo 'spec_version: *[0-9]+' "$source_dir/runtime/src/lib.rs" | grep -Eo '[0-9]+')
release_tag="v${spec:?could not parse spec_version from $source_dir/runtime/src/lib.rs}"

mainnet_sha=$(gh api "repos/$GITHUB_REPOSITORY/git/ref/heads/mainnet" --jq '.object.sha')
[[ "$mainnet_sha" == "$sha" ]] || {
  echo "mainnet mirror is at $mainnet_sha; refusing to publish $sha as a mainnet release" >&2
  exit 1
}

tag_json=$(gh api "repos/$GITHUB_REPOSITORY/git/ref/tags/$release_tag")
[[ "$(jq -er '.object.type' <<<"$tag_json")" == commit ]] || {
  echo "release tag $release_tag must be a lightweight Git tag" >&2
  exit 1
}
tag_sha=$(jq -er '.object.sha' <<<"$tag_json")
[[ "$tag_sha" == "$sha" ]] || {
  echo "release tag $release_tag points at $tag_sha, not mainnet commit $sha" >&2
  exit 1
}

echo "release_tag=$release_tag" >> "$output_file"
