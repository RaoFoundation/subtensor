#!/usr/bin/env bash

# Refresh the trusted warp-sync checkpoint in a raw/plain chain-spec pair. The RPC endpoint need
# not have synced from genesis, but it must retain the latest GRANDPA transition record, header,
# justification, and parent state and be started with `--enable-warp-sync-checkpoint-rpc`.

set -euo pipefail

default_rpc_url="https://archive.chain.opentensor.ai"
finney_genesis_hash="0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03"

if [[ $# -gt 2 ]]; then
  echo "Usage: $0 [raw-spec] [plain-spec]" >&2
  exit 2
fi

rpc_url="${WARP_SYNC_CHECKPOINT_RPC_URL:-$default_rpc_url}"
expected_genesis_hash="${WARP_SYNC_CHECKPOINT_EXPECTED_GENESIS_HASH:-$finney_genesis_hash}"
raw_spec="${1:-chainspecs/raw_spec_finney.json}"
plain_spec="${2:-chainspecs/plain_spec_finney.json}"

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
checkpoint_state=$(mktemp)
raw_spec_updated=$(mktemp "${raw_spec}.XXXXXX")
plain_spec_updated=$(mktemp "${plain_spec}.XXXXXX")
cleanup() {
  rm -f "$rpc_response" "$checkpoint_state" "$raw_spec_updated" "$plain_spec_updated"
}
trap cleanup EXIT

curl_args=(
  --fail
  --silent
  --show-error
  --connect-timeout 10
  --max-time 300
  --header "Content-Type: application/json"
  --data '{"id":1,"jsonrpc":"2.0","method":"grandpa_genWarpSyncCheckpoint","params":[]}'
)

curl "${curl_args[@]}" "$rpc_url" >"$rpc_response"

if jq -e '.error != null' "$rpc_response" >/dev/null; then
  jq -r '"grandpa_genWarpSyncCheckpoint failed: " + (.error | tostring)' "$rpc_response" >&2
  exit 1
fi

jq -e \
  '.result.grandpaWarpSyncCheckpoint
   | select(type == "object")
   | select(has("finalizedBlockHeader") and has("grandpaAuthoritySet"))
   | select(.finalizedBlockHeader | type == "string" and startswith("0x"))
   | select(.grandpaAuthoritySet | type == "string" and startswith("0x"))' \
  "$rpc_response" >"$checkpoint_state" || {
  echo "RPC response did not contain a valid GRANDPA warp-sync checkpoint" >&2
  exit 1
}

raw_chain_spec_id=$(jq -er '.id | select(type == "string")' "$raw_spec")
plain_chain_spec_id=$(jq -er '.id | select(type == "string")' "$plain_spec")
if [[ "$raw_chain_spec_id" != "$plain_chain_spec_id" ]]; then
  echo "Chain spec IDs do not match: $raw_chain_spec_id != $plain_chain_spec_id" >&2
  exit 1
fi

if ! jq -e --arg chain_spec_id "$raw_chain_spec_id" \
  '.result.chainSpecId == $chain_spec_id' "$rpc_response" >/dev/null; then
  echo "RPC endpoint chain spec ID does not match $raw_spec" >&2
  exit 1
fi

if ! jq -e --arg expected_genesis_hash "$expected_genesis_hash" \
  '.result.genesisHash == $expected_genesis_hash' "$rpc_response" >/dev/null; then
  echo "RPC endpoint genesis hash does not match the expected chain" >&2
  exit 1
fi

jq --slurpfile checkpoint "$checkpoint_state" \
  '.grandpaWarpSyncCheckpoint = $checkpoint[0]' "$raw_spec" >"$raw_spec_updated"
jq --slurpfile checkpoint "$checkpoint_state" \
  '.grandpaWarpSyncCheckpoint = $checkpoint[0]' "$plain_spec" >"$plain_spec_updated"
mv "$raw_spec_updated" "$raw_spec"
mv "$plain_spec_updated" "$plain_spec"

checkpoint=$(jq -r '.finalizedBlockHeader' "$checkpoint_state")
echo "Updated GRANDPA warp-sync checkpoint in $raw_spec and $plain_spec (${checkpoint:0:18}...)"
