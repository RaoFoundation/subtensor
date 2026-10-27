#!/usr/bin/env bash
# Node image tag policy. Every build publishes the tag of the ref it was built
# from (:main, :devnet, :testnet, :mainnet, or a sanitized manual ref). A
# mainnet mirror build also publishes its verified release tag and :latest,
# so :latest is always the code running on mainnet.

set -euo pipefail

output_file="${1:-${GITHUB_OUTPUT:-}}"
: "${output_file:?pass an output file or set GITHUB_OUTPUT}"
: "${INPUT_TAG:?INPUT_TAG is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
release_tag="${RELEASE_TAG:-}"

# Docker tags cannot contain '/', so sanitize manual refs such as feat/x.
tag="${INPUT_TAG//[^a-zA-Z0-9._-]/-}"
[[ -n "$tag" ]] || { echo "Docker tag is empty" >&2; exit 1; }

image="ghcr.io/$(printf '%s' "$GITHUB_REPOSITORY" | tr '[:upper:]' '[:lower:]')"
tags="$image:$tag"
mainnet_release=false

if [[ "$tag" == mainnet ]]; then
  [[ "$release_tag" =~ ^v[0-9]+$ ]] || {
    echo "mainnet builds require a verified vN release tag, got: '$release_tag'" >&2
    exit 1
  }
  tags+=",$image:$release_tag,$image:latest"
  mainnet_release=true
elif [[ -n "$release_tag" ]]; then
  echo "release tags are published only from the mainnet mirror" >&2
  exit 1
fi

{
  echo "tags=$tags"
  echo "mainnet_release=$mainnet_release"
} >> "$output_file"
