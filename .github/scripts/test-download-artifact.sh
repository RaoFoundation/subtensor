#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
helper="$script_dir/download-artifact.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/payload"
printf 'snapshot payload\n' > "$tmp/payload/mainnet-snapshot.tar.gz"
(cd "$tmp/payload" && zip -q "$tmp/artifact.zip" mainnet-snapshot.tar.gz)
size=$(stat -c '%s' "$tmp/artifact.zip" 2>/dev/null || stat -f '%z' "$tmp/artifact.zip")
digest="sha256:$(sha256sum "$tmp/artifact.zip" | awk '{print $1}')"

cat > "$tmp/metadata.json" <<'EOF'
{"schema_version":1,"endpoint":"http://192.168.128.1:8093","repository_id":608683796}
EOF

cat > "$tmp/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
output=''
headers=''
url=''
cache_only=false
for ((index=1; index<=$#; index++)); do
  value="${!index}"
  case "$value" in
    --output) next=$((index + 1)); output="${!next}" ;;
    --dump-header) next=$((index + 1)); headers="${!next}" ;;
    --header)
      next=$((index + 1))
      [[ "${!next}" != 'Cache-Control: only-if-cached' ]] || cache_only=true
      ;;
    http://*|https://*) url="$value" ;;
  esac
done
case "$url" in
  */token) printf 'mmds-token' ;;
  */artifact-cache) cp "$MOCK_METADATA" "$output" ;;
  http://192.168.128.1:8093/*)
    [[ "$cache_only" == true ]] || { echo 'local request was not cache-only' >&2; exit 2; }
    [[ "${MOCK_LOCAL_FAIL:-false}" != true ]] || exit 22
    cp "$MOCK_ARCHIVE" "$output"
    printf 'HTTP/1.1 200 OK\r\nX-Fireactions-Cache: hit\r\n\r\n' > "$headers"
    ;;
  https://api.github.com/*) cp "$MOCK_ARCHIVE" "$output" ;;
  *) echo "unexpected mock curl URL: $url" >&2; exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/curl"

export PATH="$tmp/bin:$PATH"
export MOCK_METADATA="$tmp/metadata.json"
export MOCK_ARCHIVE="$tmp/artifact.zip"
export GH_TOKEN=test-job-token
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export GITHUB_REPOSITORY_ID=608683796
export MMDS_TOKEN_URL=http://mmds/token
export ARTIFACT_CACHE_METADATA_URL=http://mmds/artifact-cache

: > "$tmp/output"
"$helper" 123 mainnet-snapshot "$digest" "$size" "$tmp/local" "$tmp/output" >/dev/null
grep -qx 'source=local-hit' "$tmp/output"
cmp "$tmp/payload/mainnet-snapshot.tar.gz" "$tmp/local/mainnet-snapshot.tar.gz"

: > "$tmp/output"
"$helper" 123 try-runtime-snap-v0.10.1-mainnet "$digest" "$size" \
  "$tmp/try-runtime" "$tmp/output" >/dev/null
grep -qx 'source=local-hit' "$tmp/output"
cmp "$tmp/payload/mainnet-snapshot.tar.gz" "$tmp/try-runtime/mainnet-snapshot.tar.gz"

: > "$tmp/output"
export MOCK_LOCAL_FAIL=true
"$helper" 123 mainnet-snapshot "$digest" "$size" "$tmp/direct" "$tmp/output" >/dev/null 2>"$tmp/fallback.log"
grep -qx 'source=github' "$tmp/output"
cmp "$tmp/payload/mainnet-snapshot.tar.gz" "$tmp/direct/mainnet-snapshot.tar.gz"
unset MOCK_LOCAL_FAIL

if "$helper" 123 mainnet-snapshot sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff \
  "$size" "$tmp/bad-digest" "$tmp/output" >/dev/null 2>&1; then
  echo "expected digest mismatch to fail" >&2
  exit 1
fi

if "$helper" 123 untrusted-artifact "$digest" "$size" \
  "$tmp/unsupported" "$tmp/output" >/dev/null 2>&1; then
  echo "expected unsupported artifact name to fail" >&2
  exit 1
fi

export RUNNER_TEMP="$tmp/runner"
export GITHUB_STEP_SUMMARY="$tmp/summary"
export ARTIFACT_ID=123
export ARTIFACT_DIGEST="$digest"
export ARTIFACT_SIZE="$size"
mkdir -p "$RUNNER_TEMP"
: > "$GITHUB_STEP_SUMMARY"
"$script_dir/benchmark-artifact-cache.sh" >/dev/null
grep -q '^### Mainnet snapshot artifact cache$' "$GITHUB_STEP_SUMMARY"
grep -q '^- Cache-only probe 1: .* (local-hit)$' "$GITHUB_STEP_SUMMARY"
grep -q '^- Cache-only probe 2: .* (local-hit)$' "$GITHUB_STEP_SUMMARY"

echo "artifact download helper tests passed"
