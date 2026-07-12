#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 8 ]]; then
  echo "usage: $0 ARTIFACT_ID DIGEST SIZE_BYTES CONCURRENCY DESTINATION NETWORK GENESIS CLI_VERSION" >&2
  exit 2
fi

artifact_id="$1"
expected_digest="$2"
size_bytes="$3"
concurrency="$4"
destination="$5"
network="$6"
genesis="$7"
cli_version="$8"

[[ "$artifact_id" =~ ^[0-9]+$ ]] || { echo "invalid artifact id" >&2; exit 2; }
[[ "$expected_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "invalid artifact digest" >&2; exit 2; }
[[ "$size_bytes" =~ ^[1-9][0-9]*$ ]] || { echo "invalid artifact size" >&2; exit 2; }
[[ "$concurrency" =~ ^[1-9][0-9]*$ ]] || { echo "invalid concurrency" >&2; exit 2; }
: "${GH_TOKEN:?GH_TOKEN must be set}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"

archive=$(mktemp)
parts_dir=$(mktemp -d)
trap 'rm -f "$archive"; rm -rf "$parts_dir"' EXIT

api_url="https://api.github.com/repos/$GITHUB_REPOSITORY/actions/artifacts/$artifact_id/zip"

get_download_url() {
  local headers status url
  headers=$(mktemp)
  status=$(curl --disable --silent --show-error \
    --dump-header "$headers" \
    --output /dev/null \
    --max-redirs 0 \
    --header 'Accept: application/vnd.github+json' \
    --header "Authorization: Bearer $GH_TOKEN" \
    --header 'X-GitHub-Api-Version: 2022-11-28' \
    --write-out '%{http_code}' \
    "$api_url")
  if [[ "$status" != 302 ]]; then
    echo "artifact redirect returned HTTP $status" >&2
    rm -f "$headers"
    return 1
  fi
  url=$(awk 'BEGIN { IGNORECASE=1 } /^location:/ { sub(/^[^:]+:[[:space:]]*/, ""); sub(/\r$/, ""); print; exit }' "$headers")
  rm -f "$headers"
  [[ "$url" == https://* ]] || { echo "artifact redirect did not contain an HTTPS URL" >&2; return 1; }
  printf '%s' "$url"
}

download_range() {
  local start="$1" end="$2" part="$3" expected_size attempt url actual_size
  expected_size=$((end - start + 1))
  for attempt in 1 2 3; do
    url=$(get_download_url) || continue
    if curl --disable --fail --silent --show-error \
      --range "$start-$end" \
      --output "$part" \
      "$url"; then
      actual_size=$(stat -c '%s' "$part")
      if [[ "$actual_size" == "$expected_size" ]]; then
        return 0
      fi
      echo "range $start-$end size mismatch: expected $expected_size, got $actual_size" >&2
    fi
    rm -f "$part"
    sleep "$attempt"
  done
  return 1
}

chunk_size=$(((size_bytes + concurrency - 1) / concurrency))
overall_started=$(date +%s)
download_started=$(date +%s)
pids=()
parts=()
for ((worker = 0; worker < concurrency; worker++)); do
  start=$((worker * chunk_size))
  ((start < size_bytes)) || break
  end=$((start + chunk_size - 1))
  ((end < size_bytes)) || end=$((size_bytes - 1))
  printf -v part '%s/part-%03d' "$parts_dir" "$worker"
  parts+=("$part")
  download_range "$start" "$end" "$part" &
  pids+=("$!")
done

failed=0
for pid in "${pids[@]}"; do
  wait "$pid" || failed=1
done
((failed == 0)) || { echo "one or more range downloads failed" >&2; exit 1; }
download_seconds=$(($(date +%s) - download_started))

assembly_started=$(date +%s)
for index in "${!parts[@]}"; do
  command cat "${parts[$index]}" >> "$archive"
done
assembly_seconds=$(($(date +%s) - assembly_started))

actual_size=$(stat -c '%s' "$archive")
[[ "$actual_size" == "$size_bytes" ]] || { echo "archive size mismatch" >&2; exit 1; }

verify_started=$(date +%s)
actual_digest="sha256:$(sha256sum "$archive" | awk '{print $1}')"
[[ "$actual_digest" == "$expected_digest" ]] || {
  echo "artifact digest mismatch: expected $expected_digest, got $actual_digest" >&2
  exit 1
}
verify_seconds=$(($(date +%s) - verify_started))

extract_started=$(date +%s)
mkdir -p "$destination"
unzip -q "$archive" -d "$destination"
extract_seconds=$(($(date +%s) - extract_started))

validation_started=$(date +%s)
.github/scripts/snapshot-artifact.sh validate \
  "$destination/$network.manifest.json" \
  "$destination/$network.snap" \
  "$network" "$genesis" "$cli_version"
validation_seconds=$(($(date +%s) - validation_started))
total_seconds=$(($(date +%s) - overall_started))
mbps=$(awk -v bytes="$size_bytes" -v seconds="$download_seconds" 'BEGIN { if (seconds == 0) seconds = 1; printf "%.1f", bytes * 8 / seconds / 1000000 }')
trial="${BENCHMARK_TRIAL:-unknown}"

echo "artifact=$artifact_id trial=$trial concurrency=$concurrency download=${download_seconds}s assembly=${assembly_seconds}s archive_verify=${verify_seconds}s extract=${extract_seconds}s snapshot_validate=${validation_seconds}s total=${total_seconds}s throughput=${mbps}Mbps"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  echo "| $trial | $concurrency | $size_bytes | ${download_seconds}s | ${assembly_seconds}s | ${verify_seconds}s | ${extract_seconds}s | ${validation_seconds}s | ${total_seconds}s | ${mbps} Mbps |" >> "$GITHUB_STEP_SUMMARY"
fi
