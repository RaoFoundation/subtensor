#!/bin/bash

# Pinned to the commit behind srtool v0.18.3 so the builder cannot change
# under a re-used tag. Optional first argument overrides the image tag.
IMAGE_TAG="${1:-srtool}"
SRTOOL_COMMIT="0a446889c5e60abe92a41e426377276f5c7295e6"

docker build --build-arg RUSTC_VERSION="1.93.0" -t "$IMAGE_TAG" "https://github.com/paritytech/srtool.git#${SRTOOL_COMMIT}"
