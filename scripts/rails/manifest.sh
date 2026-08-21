#!/usr/bin/env bash
# Write the rig manifest: the single JSON file that ts-tests and btcli read
# to find every endpoint and address in the local rig.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

[ -f "$RAILS_STATE_DIR/contracts.json" ] || {
    echo "[rails] ERROR: contracts.json missing; run deploy-contracts.sh first" >&2
    exit 1
}

jq -n \
    --slurpfile contracts "$RAILS_STATE_DIR/contracts.json" \
    --arg btRpcHttp "$BT_RPC_HTTP" \
    --arg btRpcWs "$BT_RPC_WS" \
    --arg baseRpcHttp "$BASE_RPC_HTTP" \
    --argjson btChainId "$BT_CHAIN_ID" \
    --argjson btDomain "$BT_DOMAIN" \
    --argjson baseChainId "$BASE_CHAIN_ID" \
    --argjson baseDomain "$BASE_DOMAIN" \
    --arg psmEscrow "$RAILS_PSM_ESCROW" \
    --arg precompile "$RAILS_PRECOMPILE" \
    --arg deployer "$DEPLOYER_ADDR" \
    --arg validator "$VALIDATOR_ADDR" \
    --arg relayer "$RELAYER_ADDR" \
    '{
        version: 1,
        chains: {
            btlocal: {
                chainId: $btChainId,
                domain: $btDomain,
                rpcHttp: $btRpcHttp,
                rpcWs: $btRpcWs,
                mailbox: $contracts[0].btlocal.mailbox,
                canonicalUsd: $contracts[0].btlocal.canonicalUsd,
                gateway: $contracts[0].btlocal.gateway,
                hubSender: $contracts[0].btlocal.hubSender,
                netuid: $contracts[0].btlocal.netuid,
                usdRailsPrecompile: $precompile,
                psmEscrow: $psmEscrow
            },
            baselocal: {
                chainId: $baseChainId,
                domain: $baseDomain,
                rpcHttp: $baseRpcHttp,
                mailbox: $contracts[0].baselocal.mailbox,
                mockUsdc: $contracts[0].baselocal.mockUsdc,
                portal: $contracts[0].baselocal.portal,
                chutes: $contracts[0].baselocal.chutes,
                tokens: $contracts[0].baselocal.tokens
            }
        },
        accounts: {
            deployer: $deployer,
            hyperlaneValidator: $validator,
            relayer: $relayer
        }
    }' >"$RAILS_MANIFEST"

echo "[rails] wrote $RAILS_MANIFEST"
jq . "$RAILS_MANIFEST"
