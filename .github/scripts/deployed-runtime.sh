#!/usr/bin/env bash
# Prints the identity of the runtime with the given spec_version as finalized
# on mainnet, as key=value lines: first_block, block_hash, code_hash,
# current_spec. Fails unless that spec_version was enacted on mainnet.
#
# spec_version only increases, so the first finalized block running it is
# found by binary search. Historical state needs an archive endpoint.
#
# Usage: deployed-runtime.sh SPEC_VERSION

set -euo pipefail

spec="${1:?spec_version required}"
[[ "$spec" =~ ^[0-9]+$ ]] || { echo "spec_version must be an integer" >&2; exit 1; }
: "${ARCHIVE_HTTP:?ARCHIVE_HTTP is required}"

# The public archive endpoint rate-limits HTTP clients and answers with
# `retry_after_seconds`; wait and retry instead of failing the search.
rpc() {
  local body response wait
  body=$(jq -cn --arg method "$1" --argjson params "$2" \
    '{id: 1, jsonrpc: "2.0", method: $method, params: $params}')
  for _ in 1 2 3 4 5 6; do
    response=$(curl -sS --retry 3 -H "Content-Type: application/json" -d "$body" "$ARCHIVE_HTTP") || true
    if wait=$(jq -er '.retry_after_seconds | numbers' <<<"$response" 2>/dev/null); then
      sleep "$wait"
      continue
    fi
    jq -e 'has("result")' <<<"$response" >/dev/null 2>&1 && { printf '%s\n' "$response"; return 0; }
    echo "archive RPC $1 failed: ${response:0:200}" >&2
    return 1
  done
  echo "archive RPC $1 still rate-limited" >&2
  return 1
}
hash_at() {
  rpc chain_getBlockHash "[$1]" | jq -er '.result | strings | select(test("^0x[0-9a-f]{64}$"))'
}
spec_at() {
  rpc state_getRuntimeVersion "$(jq -cn --arg b "$1" '[$b]')" | jq -er '.result.specVersion | numbers'
}

finalized=$(rpc chain_getFinalizedHead '[]' | jq -er '.result | strings')
head=$(rpc chain_getHeader "$(jq -cn --arg b "$finalized" '[$b]')" | jq -er '.result.number' )
head=$((head))
current_spec=$(spec_at "$finalized")
(( current_spec >= spec )) || { echo "mainnet runs spec $current_spec; $spec has not been enacted" >&2; exit 1; }

# Smallest finalized block whose runtime is at least $spec.
lo=0 hi=$head
while (( lo < hi )); do
  mid=$(( (lo + hi) / 2 ))
  s=$(spec_at "$(hash_at "$mid")")
  if (( s >= spec )); then hi=$mid; else lo=$((mid + 1)); fi
done
block_hash=$(hash_at "$lo")
found=$(spec_at "$block_hash")
(( found == spec )) || { echo "spec $spec never ran on mainnet (skipped from $((found - 1)) to $found)" >&2; exit 1; }
code_hash=$(rpc state_getStorageHash "$(jq -cn --arg b "$block_hash" '["0x3a636f6465", $b]')" \
  | jq -er '.result | strings | select(test("^0x[0-9a-f]{64}$"))')

printf 'first_block=%s\nblock_hash=%s\ncode_hash=%s\ncurrent_spec=%s\n' \
  "$lo" "$block_hash" "$code_hash" "$current_spec"
