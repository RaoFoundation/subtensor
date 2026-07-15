#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "usage: $0 ARTIFACT_ID ARTIFACT_NAME SHA256_DIGEST SIZE_BYTES DESTINATION OUTPUT_FILE" >&2
  exit 2
}

[[ $# -eq 6 ]] || usage
artifact_id="$1"
artifact_name="$2"
expected_digest="$3"
expected_size="$4"
destination="$5"
output_file="$6"

: "${GH_TOKEN:?GH_TOKEN must contain a short-lived Actions token}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
: "${GITHUB_REPOSITORY_ID:?GITHUB_REPOSITORY_ID must be set}"

[[ "$artifact_id" =~ ^[1-9][0-9]*$ ]] || usage
[[ "$expected_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || usage
[[ "$expected_size" =~ ^[1-9][0-9]*$ ]] || usage
[[ -n "$destination" && "$destination" != / ]] || usage
case "$artifact_name" in
  mainnet-snapshot|try-runtime-snap-v0.10.1-mainnet|try-runtime-snap-v0.10.1-testnet|try-runtime-snap-v0.10.1-devnet) ;;
  *) echo "artifact is outside the host-cache allowlist: $artifact_name" >&2; exit 2 ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
archive="$tmp/artifact.zip"
headers="$tmp/headers"
metadata="$tmp/artifact-cache.json"
extract_dir="$tmp/extract"
mkdir -p "$extract_dir"
started=$(date -u +%s)
source=github

set_output() {
  printf '%s=%s\n' "$1" "$2" >> "$output_file"
}

discover_cache_endpoint() {
  [[ "${FIREACTIONS_ARTIFACT_CACHE_DISABLE:-false}" != true ]] || return 1
  local token_url="${MMDS_TOKEN_URL:-http://169.254.169.254/latest/api/token}"
  local metadata_url="${ARTIFACT_CACHE_METADATA_URL:-http://169.254.169.254/latest/meta-data/artifact-cache}"
  local token
  token=$(curl --fail --silent --show-error --connect-timeout 1 --max-time 2 \
    --request PUT --header 'X-Metadata-Token-TTL-Seconds: 60' "$token_url" 2>/dev/null) || return 1
  [[ -n "$token" ]] || return 1
  curl --fail --silent --show-error --connect-timeout 1 --max-time 3 \
    --header "X-Metadata-Token: $token" --header 'Accept: application/json' \
    --output "$metadata" "$metadata_url" 2>/dev/null || return 1
  GITHUB_REPOSITORY_ID="$GITHUB_REPOSITORY_ID" python3 -c '
import json, os, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
if set(data) != {"schema_version", "endpoint", "repository_id"}:
    raise SystemExit(1)
if data.get("schema_version") != 1:
    raise SystemExit(1)
if data.get("endpoint") != "http://192.168.128.1:8093":
    raise SystemExit(1)
if str(data.get("repository_id")) != os.environ["GITHUB_REPOSITORY_ID"]:
    raise SystemExit(1)
print(data["endpoint"], end="")
' "$metadata" 2>/dev/null
}

cache_endpoint=$(discover_cache_endpoint || true)
if [[ -n "$cache_endpoint" ]]; then
  if curl --fail --silent --show-error --retry 2 --retry-all-errors \
    --connect-timeout 5 --max-time 1800 \
    --header "Authorization: Bearer $GH_TOKEN" \
    --dump-header "$headers" --output "$archive" \
    "$cache_endpoint/v1/artifacts/$artifact_id"; then
    cache_result=$(awk -F': *' 'tolower($1)=="x-fireactions-cache" {gsub("\\r", "", $2); print tolower($2)}' "$headers" | tail -1)
    source="local-${cache_result:-unknown}"
  else
    rm -f "$archive" "$headers"
  fi
fi

if [[ ! -f "$archive" ]]; then
  curl --fail --silent --show-error --location --retry 3 --retry-all-errors \
    --connect-timeout 10 --max-time 1800 \
    --header "Authorization: Bearer $GH_TOKEN" \
    --header 'Accept: application/vnd.github+json' \
    --header 'X-GitHub-Api-Version: 2022-11-28' \
    --output "$archive" \
    "https://api.github.com/repos/$GITHUB_REPOSITORY/actions/artifacts/$artifact_id/zip"
  source=github
fi

actual_size=$(stat -c '%s' "$archive" 2>/dev/null || stat -f '%z' "$archive")
actual_digest="sha256:$(sha256sum "$archive" | awk '{print $1}')"
if [[ "$actual_size" != "$expected_size" || "$actual_digest" != "$expected_digest" ]]; then
  echo "artifact archive integrity check failed for $artifact_id" >&2
  exit 1
fi

python3 - "$archive" "$extract_dir" <<'PY'
import pathlib
import stat
import sys
import zipfile

archive, destination = sys.argv[1:]
with zipfile.ZipFile(archive) as payload:
    total = 0
    for member in payload.infolist():
        path = pathlib.PurePosixPath(member.filename)
        mode = member.external_attr >> 16
        if (
            not member.filename
            or member.filename.startswith(("/", "\\"))
            or ".." in path.parts
            or (path.parts and ":" in path.parts[0])
            or stat.S_ISLNK(mode)
        ):
            raise SystemExit("unsafe artifact archive path")
        total += member.file_size
        if total > 16 * 1024 * 1024 * 1024:
            raise SystemExit("artifact archive exceeds extraction limit")
    payload.extractall(destination)
PY

if [[ -d "$destination" && -n "$(find "$destination" -mindepth 1 -print -quit 2>/dev/null)" ]]; then
  echo "artifact destination is not empty: $destination" >&2
  exit 1
fi
mkdir -p "$destination"
cp -a "$extract_dir/." "$destination/"

seconds=$(($(date -u +%s) - started))
set_output source "$source"
set_output seconds "$seconds"
set_output artifact-id "$artifact_id"
echo "Downloaded $artifact_name artifact $artifact_id via $source in ${seconds}s."
