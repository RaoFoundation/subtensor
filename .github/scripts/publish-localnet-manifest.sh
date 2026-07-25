#!/usr/bin/env bash

set -euo pipefail

: "${IMAGE:?IMAGE is required}"
: "${TAG:?TAG is required}"
: "${SHA:?SHA is required}"
: "${PUBLISH_LATEST:?PUBLISH_LATEST is required}"

descriptor_dir="${1:-image-descriptors}"

[[ "$SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "SHA must be a full lowercase commit SHA: $SHA" >&2
  exit 1
}

read_source() {
  local arch="$1"
  local descriptor="$descriptor_dir/$arch.txt"
  local source digest

  [[ -s "$descriptor" ]] || {
    echo "missing $arch image descriptor: $descriptor" >&2
    return 1
  }

  IFS= read -r source < "$descriptor"
  digest="${source#"$IMAGE@"}"
  [[ "$source" == "$IMAGE@$digest" && "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "invalid $arch image descriptor: $source" >&2
    return 1
  }

  printf '%s' "$source"
}

amd64_source="$(read_source amd64)"
arm64_source="$(read_source arm64)"

tags=(
  --tag "$IMAGE:$TAG"
  --tag "$IMAGE:sha-$SHA"
)
case "$PUBLISH_LATEST" in
  true)
    tags+=(--tag "$IMAGE:latest")
    ;;
  false)
    ;;
  *)
    echo "PUBLISH_LATEST must be true or false: $PUBLISH_LATEST" >&2
    exit 1
    ;;
esac

docker buildx imagetools create \
  "${tags[@]}" \
  --annotation "index:org.opencontainers.image.description=Subtensor local development network for CI and local testing" \
  --annotation "index:org.opencontainers.image.source=https://github.com/RaoFoundation/subtensor" \
  --annotation "index:org.opencontainers.image.licenses=Apache-2.0" \
  --annotation "index:org.opencontainers.image.revision=$SHA" \
  --annotation "index:org.opencontainers.image.version=$TAG" \
  "$amd64_source" \
  "$arm64_source"
