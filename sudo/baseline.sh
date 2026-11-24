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
case "$NETWORK" in
  devnet) initial_sha256=5f02f87c60eb9294d2836e24b67610a22411fab174a0c6ebf27149d513794deb ;;
  testnet) initial_sha256=e496f593b8a8451ad691f59fed7acc60981bc52ea681e3cd6d55dd2825276532 ;;
  mainnet) initial_sha256=80348f2056d9fe14357e682d80941455450647b4af1609202f0a7115b5212f6b ;;
  *) echo "Unknown network: $NETWORK" >&2; exit 2 ;;
esac
if [ -n "$base_commit" ] && git show "${base_commit}:sudo/${NETWORK}.toml" > "${base_file}.tmp" 2>/dev/null; then
  mv "${base_file}.tmp" "$base_file"
  echo "Using reviewed baseline ${base_commit}." >&2
elif [ "$EVENT_NAME" = "pull_request" ] &&
     git cat-file -e "${source_commit}:sudo/${NETWORK}.toml" 2>/dev/null &&
     git show "${source_commit}:sudo/${NETWORK}.toml" | cmp -s - "$copied_source" &&
     [ "$source_sha256" = "$initial_sha256" ]; then
  # Bootstrap only the exact committed initial live export, identified by hash.
  # Any later candidate edit requires an explicit reviewed base.
  base_commit="$source_commit"
  cp "$copied_source" "$base_file"
  echo "Bootstrapping unchanged initial export at ${base_commit}." >&2
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
