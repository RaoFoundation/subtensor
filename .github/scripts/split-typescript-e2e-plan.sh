#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUTPUT_FILE" >&2
  exit 2
fi

output_file=$1
: "${STATE_MATRIX:?STATE_MATRIX must contain the trusted state matrix}"
: "${BUILD_MATRIX:?BUILD_MATRIX must contain the trusted build matrix}"

jq -e '
  type == "object" and
  (.include | type == "array") and
  all(.include[]; type == "object" and (.binary == "fast" or .binary == "release"))
' <<< "$STATE_MATRIX" >/dev/null
jq -e '
  type == "object" and
  (.include | type == "array") and
  all(.include[]; type == "object" and (.variant == "fast" or .variant == "release"))
' <<< "$BUILD_MATRIX" >/dev/null

fast_state_matrix=$(jq -c '{include: [.include[] | select(.binary == "fast")]}' <<< "$STATE_MATRIX")
release_state_matrix=$(jq -c '{include: [.include[] | select(.binary == "release")]}' <<< "$STATE_MATRIX")
fast_state_count=$(jq '.include | length' <<< "$fast_state_matrix")
release_state_count=$(jq '.include | length' <<< "$release_state_matrix")
fast_build=$(jq -r 'any(.include[]; .variant == "fast")' <<< "$BUILD_MATRIX")
release_build=$(jq -r 'any(.include[]; .variant == "release")' <<< "$BUILD_MATRIX")

if (( fast_state_count > 0 )) && [[ "$fast_build" != true ]]; then
  echo "fast state jobs were planned without a fast binary build" >&2
  exit 1
fi
if (( release_state_count > 0 )) && [[ "$release_build" != true ]]; then
  echo "release state jobs were planned without a release binary build" >&2
  exit 1
fi

{
  echo "fast_state_count=$fast_state_count"
  echo "fast_state_matrix=$fast_state_matrix"
  echo "release_state_count=$release_state_count"
  echo "release_state_matrix=$release_state_matrix"
  echo "fast_build=$fast_build"
  echo "release_build=$release_build"
} >> "$output_file"
