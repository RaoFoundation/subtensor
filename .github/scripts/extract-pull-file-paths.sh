#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^[0-9]+$ ]]; then
  echo "usage: $0 EXPECTED_FILE_COUNT" >&2
  exit 2
fi

expected=$1
payload=$(mktemp)
trap 'rm -f "$payload"' EXIT
cat > "$payload"

observed=$(jq -es '
  if all(.[]; type == "array")
  then (map(length) | add // 0)
  else error("pull-file response contains a non-array page")
  end
' "$payload")

if [[ "$observed" != "$expected" ]]; then
  echo "changed-file list incomplete ($observed/$expected)" >&2
  exit 1
fi

jq -ers '
  [
    .[] | .[] |
    if type != "object" or (.filename | type) != "string" or .filename == ""
    then error("pull-file entry has no filename")
    elif has("previous_filename") and (.previous_filename | type) != "string"
    then error("pull-file entry has an invalid previous_filename")
    else .filename, (.previous_filename // empty)
    end
    | select(length > 0)
  ]
  | unique[]
' "$payload"
