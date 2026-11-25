#!/usr/bin/env bash
# Resolve immutable source/baseline provenance without signing or chain access.
# Required: NETWORK, BASE_REF, EVENT_NAME, OUTPUT_DIR. Optional: SOURCE_FILE.
set -euo pipefail

: "${NETWORK:?}" "${BASE_REF:?}" "${EVENT_NAME:?}" "${OUTPUT_DIR:?}"
source_file=${SOURCE_FILE:-sudo/${NETWORK}.toml}
base_file="${OUTPUT_DIR}/${NETWORK}-base.toml"
copied_source="${OUTPUT_DIR}/${NETWORK}-source.toml"
mkdir -p "$OUTPUT_DIR"
rm -f "$base_file" "${base_file}.tmp"
cp "$source_file" "$copied_source"
source_commit=$(git rev-parse HEAD)
base_commit=$(git rev-parse --verify "${BASE_REF}^{commit}" 2>/dev/null || true)
source_sha256=$(sha256sum "$copied_source" | cut -d ' ' -f 1)
case "$NETWORK" in devnet|testnet|mainnet) ;; *) echo "Unknown network: $NETWORK" >&2; exit 2 ;; esac
if [ "$EVENT_NAME" = "pull_request" ] && [ -n "$base_commit" ]; then
  mapfile -t merge_bases < <(git merge-base --all "$base_commit" "$source_commit")
  if [ "${#merge_bases[@]}" -ne 1 ]; then
    echo "Expected exactly one PR merge base" >&2
    exit 2
  fi
  base_commit="${merge_bases[0]}"
fi
# Only an actual pre-push ancestor can delimit a reviewed push range.
if [ "$EVENT_NAME" = "push" ] && [ -n "$base_commit" ] &&
   ! git merge-base --is-ancestor "$base_commit" "$source_commit"; then
  base_commit=""
fi
if [ -n "$base_commit" ] && git show "${base_commit}:sudo/${NETWORK}.toml" > "${base_file}.tmp" 2>/dev/null; then
  mv "${base_file}.tmp" "$base_file"
  if [ "$EVENT_NAME" = "workflow_dispatch" ]; then
    echo "Using comparison-only baseline ${base_commit}." >&2
  else
    echo "Using reviewed baseline ${base_commit}." >&2
  fi
else
  base_commit=""
  echo "No exact reviewed baseline is available; comparison is report-only." >&2
fi
base_sha256=""
if [ -f "$base_file" ]; then
  base_sha256=$(sha256sum "$base_file" | cut -d ' ' -f 1)
fi
printf 'SOURCE_FILE=%s\nBASE_FILE=%s\nSOURCE_COMMIT=%s\nBASE_COMMIT=%s\nSOURCE_SHA256=%s\nBASE_SHA256=%s\n' \
  "$copied_source" "$base_file" "$source_commit" "$base_commit" "$source_sha256" "$base_sha256"
