#!/usr/bin/env bash

# Refresh the trusted warp-sync checkpoint in a raw/plain chain-spec pair. The RPC endpoint only
# needs to be a fully synced BABE node started with `--enable-sync-state-rpc`; it does not need an
# archive database. Keep that RPC endpoint private or access-controlled.

set -euo pipefail

if [[ $# -lt 1 || $# -gt 3 ]]; then
  echo "Usage: $0 <rpc-url> [raw-spec] [plain-spec]" >&2
  exit 2
fi

rpc_url="$1"
raw_spec="${2:-chainspecs/raw_spec_finney.json}"
plain_spec="${3:-chainspecs/plain_spec_finney.json}"

for command in curl jq; do
  if ! command -v "$command" >/dev/null; then
    echo "Required command not found: $command" >&2
    exit 1
  fi
done

for spec in "$raw_spec" "$plain_spec"; do
  if [[ ! -f "$spec" ]]; then
    echo "Chain spec not found: $spec" >&2
    exit 1
  fi
done

rpc_response=$(mktemp)
light_sync_state=$(mktemp)
raw_spec_updated=$(mktemp "${raw_spec}.XXXXXX")
plain_spec_updated=$(mktemp "${plain_spec}.XXXXXX")
cleanup() {
  rm -f "$rpc_response" "$light_sync_state" "$raw_spec_updated" \
    "$plain_spec_updated"
}
trap cleanup EXIT

curl_args=(
  --fail
  --silent
  --show-error
  --connect-timeout 10
  --max-time 300
  --header "Content-Type: application/json"
  --data '{"id":1,"jsonrpc":"2.0","method":"sync_state_genSyncSpec","params":[true]}'
)

if [[ -n "${SYNC_STATE_RPC_USER:-}" || -n "${SYNC_STATE_RPC_PASSWORD:-}" ]]; then
  if [[ -z "${SYNC_STATE_RPC_USER:-}" || -z "${SYNC_STATE_RPC_PASSWORD:-}" ]]; then
    echo "Set both SYNC_STATE_RPC_USER and SYNC_STATE_RPC_PASSWORD for basic authentication" >&2
    exit 1
  fi
  curl_args+=(--user "${SYNC_STATE_RPC_USER}:${SYNC_STATE_RPC_PASSWORD}")
fi

curl "${curl_args[@]}" "$rpc_url" >"$rpc_response"

if jq -e '.error != null' "$rpc_response" >/dev/null; then
  jq -r '"sync_state_genSyncSpec failed: " + (.error | tostring)' "$rpc_response" >&2
  exit 1
fi

jq -e \
  '.result.lightSyncState
   | select(type == "object")
   | select(
       has("finalizedBlockHeader")
       and has("babeEpochChanges")
       and has("babeFinalizedBlockWeight")
       and has("grandpaAuthoritySet")
     )
   | select(.finalizedBlockHeader | type == "string" and startswith("0x"))' \
  "$rpc_response" >"$light_sync_state" || {
  echo "RPC response did not contain a valid lightSyncState checkpoint" >&2
  exit 1
}

if ! jq -e --slurpfile raw_spec "$raw_spec" \
  '.result.genesis == $raw_spec[0].genesis' "$rpc_response" >/dev/null; then
  echo "RPC endpoint genesis does not match $raw_spec" >&2
  exit 1
fi

jq --slurpfile light_sync_state "$light_sync_state" \
  '.lightSyncState = $light_sync_state[0]' "$raw_spec" >"$raw_spec_updated"
jq --slurpfile light_sync_state "$light_sync_state" \
  '.lightSyncState = $light_sync_state[0]' "$plain_spec" >"$plain_spec_updated"
mv "$raw_spec_updated" "$raw_spec"
mv "$plain_spec_updated" "$plain_spec"

checkpoint=$(jq -r '.finalizedBlockHeader' "$light_sync_state")
echo "Updated lightSyncState in $raw_spec and $plain_spec (${checkpoint:0:18}...)"
