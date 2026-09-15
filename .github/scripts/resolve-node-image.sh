#!/usr/bin/env bash

set -euo pipefail

output_file="${1:-${GITHUB_ENV:-}}"
: "${output_file:?pass an output file or set GITHUB_ENV}"
: "${INPUT_TAG:?INPUT_TAG is required}"
: "${SOURCE_REF:?SOURCE_REF is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"

# Docker tags cannot contain '/', so sanitize manual refs such as feat/x.
tag="${INPUT_TAG//[^a-zA-Z0-9._-]/-}"
[[ -n "$tag" ]] || { echo "Docker tag is empty" >&2; exit 1; }

# Main is the only production publication path. A successful main build
# updates :main and :latest together; release and network tags cannot race it
# and move :latest backward.
if [[ "$tag" == main && "$SOURCE_REF" == refs/heads/main ]]; then
  latest_tag=true
else
  latest_tag=false
fi

image_repository=$(printf '%s' "$GITHUB_REPOSITORY" | tr '[:upper:]' '[:lower:]')

{
  echo "tag=$tag"
  echo "latest_tag=$latest_tag"
  echo "image=ghcr.io/$image_repository"
} >> "$output_file"
