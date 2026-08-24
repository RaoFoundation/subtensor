#!/usr/bin/env bash

# Refresh the trusted warp-sync checkpoint in a raw/plain chain-spec pair. The RPC endpoint must
# be a node synced from genesis and started with `--enable-warp-sync-checkpoint-rpc`, because
# generating the historical transition checkpoint requires retained GRANDPA history. Keep that
# endpoint private and access-controlled.

set -euo pipefail

if [[ $# -gt 2 ]]; then
  echo "Usage: WARP_SYNC_CHECKPOINT_RPC_URL=<private-url> $0 [raw-spec] [plain-spec]" >&2
  exit 2
fi

if [[ -z "${WARP_SYNC_CHECKPOINT_RPC_URL:-}" ]]; then
  echo "Set WARP_SYNC_CHECKPOINT_RPC_URL to the private checkpoint RPC endpoint" >&2
  exit 2
fi

rpc_url="$WARP_SYNC_CHECKPOINT_RPC_URL"
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
  rm -f "$rpc_response" "$checkpoint_state" "$raw_spec_updated" \
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
  --data '{"id":1,"jsonrpc":"2.0","method":"grandpa_genWarpSyncSpec","params":[true]}'
)

if [[ -n "${WARP_SYNC_CHECKPOINT_RPC_USER:-}" || -n "${WARP_SYNC_CHECKPOINT_RPC_PASSWORD:-}" ]]; then
  if [[ -z "${WARP_SYNC_CHECKPOINT_RPC_USER:-}" || -z "${WARP_SYNC_CHECKPOINT_RPC_PASSWORD:-}" ]]; then
    echo "Set both WARP_SYNC_CHECKPOINT_RPC_USER and WARP_SYNC_CHECKPOINT_RPC_PASSWORD for basic authentication" >&2
    exit 1
  fi
  curl_args+=(--user "${WARP_SYNC_CHECKPOINT_RPC_USER}:${WARP_SYNC_CHECKPOINT_RPC_PASSWORD}")
fi

curl "${curl_args[@]}" "$rpc_url" >"$rpc_response"

if jq -e '.error != null' "$rpc_response" >/dev/null; then
  jq -r '"grandpa_genWarpSyncSpec failed: " + (.error | tostring)' "$rpc_response" >&2
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

if ! jq -e --slurpfile raw_spec "$raw_spec" \
  '.result.genesis == $raw_spec[0].genesis' "$rpc_response" >/dev/null; then
  echo "RPC endpoint genesis does not match $raw_spec" >&2
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
