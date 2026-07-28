#!/bin/bash

set -euo pipefail

# Pinned to the commit behind srtool v0.18.3 so the builder cannot change
# under a re-used tag. Optional first argument overrides the image tag.
IMAGE_TAG="${1:-srtool}"
SRTOOL_COMMIT="0a446889c5e60abe92a41e426377276f5c7295e6"
SRTOOL_CONTEXT="https://github.com/paritytech/srtool.git#${SRTOOL_COMMIT}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DOCKERFILE="${SCRIPT_DIR}/Dockerfile"

build_image() {
  local ubuntu_mirror="$1"

  docker build \
    --platform linux/amd64 \
    --build-arg RUSTC_VERSION="1.89.0" \
    --build-arg UBUNTU_MIRROR="${ubuntu_mirror}" \
    --file - \
    --tag "${IMAGE_TAG}" \
    "${SRTOOL_CONTEXT}" < "${DOCKERFILE}"
}

if ! build_image "http://ubuntu.mirrors.ovh.net/ubuntu"; then
  echo >&2 "OVH Ubuntu mirror build failed; retrying with Ubuntu's geographic mirror service."
  build_image "mirror+http://mirrors.ubuntu.com/mirrors.txt"
fi
