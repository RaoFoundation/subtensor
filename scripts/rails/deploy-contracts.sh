#!/usr/bin/env bash
# Deploy the chain-owned rails contracts to both local chains and wire the
# trust relationships (minter windows, trusted senders). Idempotent via
# CREATE2: re-running against live chains is a no-op re-wire.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

CONTRACTS_DIR="$RAILS_BASE_DIR/contracts/evm"

yaml_value() { # file key
    awk -v k="$2:" '$1 == k { print $2; exit }' "$1" | tr -d '"'
}

bt_mailbox="$(yaml_value "$RAILS_REGISTRY_DIR/chains/$BT_CHAIN_NAME/addresses.yaml" mailbox)"
base_mailbox="$(yaml_value "$RAILS_REGISTRY_DIR/chains/$BASE_CHAIN_NAME/addresses.yaml" mailbox)"
[ -n "$bt_mailbox" ] && [ -n "$base_mailbox" ] || {
    echo "[rails] ERROR: mailbox addresses missing; run hyperlane-deploy.sh first" >&2
    exit 1
}

extract_addr() { # log-file label
    awk -v k="$2" '$1 == k { print $2 }' "$1" | tail -1
}

# The canonical CREATE2 deployer (0x4e59b4...956C) is pre-installed on anvil
# but not on the Bittensor localnet EVM. Install it via Nick's method: fund
# the one-time deployer EOA, then broadcast the well-known presigned tx.
CREATE2_DEPLOYER="0x4e59b44847b379578588920cA78FbF26c0B4956C"
CREATE2_SIGNER="0x3fAB184622Dc19b6109349B94811493BF2a45362"
CREATE2_RAW_TX="0xf8a58085174876e800830186a08080b853604580600e600039806000f350fe7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222"
wait_for_code() { # addr
    for _ in $(seq 1 30); do
        [ "$(cast code "$1" --rpc-url "$BT_RPC_HTTP")" != "0x" ] && return 0
        sleep 1
    done
    return 1
}
if [ "$(cast code "$CREATE2_DEPLOYER" --rpc-url "$BT_RPC_HTTP")" = "0x" ]; then
    echo "[rails] installing CREATE2 deployer on $BT_CHAIN_NAME ..."
    # --async: cast's receipt watcher cannot parse this node's block JSON.
    cast send "$CREATE2_SIGNER" --value 100000000000000000 \
        --rpc-url "$BT_RPC_HTTP" --private-key "$DEPLOYER_PK" --legacy --async >/dev/null
    for _ in $(seq 1 30); do
        [ "$(cast balance "$CREATE2_SIGNER" --rpc-url "$BT_RPC_HTTP")" != "0" ] && break
        sleep 1
    done
    cast publish "$CREATE2_RAW_TX" --rpc-url "$BT_RPC_HTTP" --async >/dev/null
    wait_for_code "$CREATE2_DEPLOYER" || {
        echo "[rails] ERROR: CREATE2 deployer install failed" >&2
        exit 1
    }
fi

echo "[rails] deploying Bittensor-EVM contracts (canonical USD + Gateway) ..."
RAILS_OWNER="$DEPLOYER_ADDR" RAILS_MAILBOX="$bt_mailbox" RAILS_PSM_ESCROW="$RAILS_PSM_ESCROW" \
    forge script "$CONTRACTS_DIR/script/DeployBittensor.s.sol:DeployBittensor" \
    --root "$CONTRACTS_DIR" \
    --rpc-url "$BT_RPC_HTTP" \
    --private-key "$DEPLOYER_PK" \
    --broadcast --slow --legacy \
    >"$RAILS_LOG_DIR/deploy-bittensor.log" 2>&1
canonical_usd="$(extract_addr "$RAILS_LOG_DIR/deploy-bittensor.log" CANONICAL_USD)"
gateway="$(extract_addr "$RAILS_LOG_DIR/deploy-bittensor.log" GATEWAY)"
[ -n "$canonical_usd" ] && [ -n "$gateway" ] || {
    echo "[rails] ERROR: Bittensor deploy failed, see $RAILS_LOG_DIR/deploy-bittensor.log" >&2
    exit 1
}

# The subnet token catalog: real mainnet identities (name, description,
# logo) that the configure step replicates onto localnet subnets.
CATALOG="$SCRIPT_DIR/subnets.json"
tokens_env="$(jq -r 'map(.name + "|" + .symbol) | join(",")' "$CATALOG")"
token_count="$(jq 'length' "$CATALOG")"

echo "[rails] deploying fake-Base contracts (USDC, portal, $token_count subnet tokens) ..."
RAILS_OWNER="$DEPLOYER_ADDR" RAILS_MAILBOX="$base_mailbox" RAILS_HUB_DOMAIN="$BT_DOMAIN" \
    RAILS_GATEWAY="$gateway" RAILS_USD_ASSET_ID=0 RAILS_TOKENS="$tokens_env" \
    forge script "$CONTRACTS_DIR/script/DeployBase.s.sol:DeployBase" \
    --root "$CONTRACTS_DIR" \
    --rpc-url "$BASE_RPC_HTTP" \
    --private-key "$DEPLOYER_PK" \
    --broadcast --slow \
    >"$RAILS_LOG_DIR/deploy-base.log" 2>&1
mock_usdc="$(extract_addr "$RAILS_LOG_DIR/deploy-base.log" MOCK_USDC)"
portal="$(extract_addr "$RAILS_LOG_DIR/deploy-base.log" PORTAL)"
[ -n "$mock_usdc" ] && [ -n "$portal" ] || {
    echo "[rails] ERROR: Base deploy failed, see $RAILS_LOG_DIR/deploy-base.log" >&2
    exit 1
}
# Collect the per-catalog-entry token addresses (TOKEN_0, TOKEN_1, ...).
token_addrs=()
for i in $(seq 0 $((token_count - 1))); do
    addr="$(extract_addr "$RAILS_LOG_DIR/deploy-base.log" "TOKEN_$i")"
    [ -n "$addr" ] || {
        echo "[rails] ERROR: token $i missing from deploy log" >&2
        exit 1
    }
    token_addrs+=("$addr")
done
# Merge deployed addresses into the catalog for the configure step.
tokens_json="$(jq -cn --slurpfile cat "$CATALOG" --args \
    '$cat[0] | to_entries | map(.value + {address: $ARGS.positional[.key]})' \
    "${token_addrs[@]}")"

addr32() { # 0x-address -> bytes32
    printf '0x%024x%s' 0 "${1#0x}"
}

echo "[rails] wiring trust relationships ..."
# Gateway may mint canonical USD backing: 1M USD window, 100/s refill.
cast send --rpc-url "$BT_RPC_HTTP" --private-key "$DEPLOYER_PK" --legacy \
    "$canonical_usd" "setMinterLimits(address,uint64,uint64)" \
    "$gateway" 1000000000000000 100000000000 >/dev/null
# Portal (on Base) is a trusted UsdPortal sender: buys lock USDC at origin,
# so the Gateway mints canonical USD backing on arrival.
cast send --rpc-url "$BT_RPC_HTTP" --private-key "$DEPLOYER_PK" --legacy \
    "$gateway" "setTrustedSender(uint32,bytes32,uint8)" \
    "$BASE_DOMAIN" "$(addr32 "$portal")" 1 >/dev/null
# The deployer EOA is also trusted (walking-skeleton ping dispatches directly).
cast send --rpc-url "$BT_RPC_HTTP" --private-key "$DEPLOYER_PK" --legacy \
    "$gateway" "setTrustedSender(uint32,bytes32,uint8)" \
    "$BASE_DOMAIN" "$(addr32 "$DEPLOYER_ADDR")" 1 >/dev/null
# Token sells burn shares at origin: trusted RemoteToken senders (kind 2, no
# USD mint on arrival; the runtime unstakes escrow instead).
for addr in "${token_addrs[@]}"; do
    cast send --rpc-url "$BT_RPC_HTTP" --private-key "$DEPLOYER_PK" --legacy \
        "$gateway" "setTrustedSender(uint32,bytes32,uint8)" \
        "$BASE_DOMAIN" "$(addr32 "$addr")" 2 >/dev/null
done

echo "[rails] configuring pallet-usd-psm (gateway, PSM asset, pool, $token_count demo subnets) ..."
configure_log="$RAILS_LOG_DIR/configure-pallet.log"
resolved_json="$RAILS_STATE_DIR/subnets-resolved.json"
(cd "$RAILS_BASE_DIR/ts-tests" && BT_RPC_WS="$BT_RPC_WS" \
    GATEWAY_ADDR="$gateway" CANONICAL_USD_ADDR="$canonical_usd" \
    BT_MAILBOX_ADDR="$bt_mailbox" PORTAL_ADDR="$portal" \
    BASE_DOMAIN="$BASE_DOMAIN" TOKENS_JSON="$tokens_json" \
    RESOLVED_OUT="$resolved_json" \
    pnpm exec tsx scripts/rails-configure-pallet.ts) | tee "$configure_log"
hub_sender="$(extract_addr "$configure_log" HUB_SENDER)"
netuid="$(extract_addr "$configure_log" NETUID)"
[ -n "$hub_sender" ] && [ -n "$netuid" ] && [ -f "$resolved_json" ] || {
    echo "[rails] ERROR: configure step did not report HUB_SENDER/NETUID/catalog" >&2
    exit 1
}

echo "[rails] wiring $token_count share tokens + portal hub path on $BASE_CHAIN_NAME ..."
# Each token trusts the runtime's keyless identity for mints/index pushes and
# dispatches sell envelopes to the Gateway on the hub.
while IFS=$'\t' read -r token_addr token_netuid; do
    cast send --rpc-url "$BASE_RPC_HTTP" --private-key "$DEPLOYER_PK" \
        "$token_addr" "configureHub(address,uint32,bytes32,bytes32,address,uint16,uint32)" \
        "$base_mailbox" "$BT_DOMAIN" "$(addr32 "$hub_sender")" "$(addr32 "$gateway")" \
        "$portal" "$token_netuid" 0 >/dev/null
    # Portal: the token may draw sequential nonces for sell envelopes.
    cast send --rpc-url "$BASE_RPC_HTTP" --private-key "$DEPLOYER_PK" \
        "$portal" "setToken(address,bool)" "$token_addr" true >/dev/null
done < <(jq -r '.[] | [.address, (.netuid | tostring)] | @tsv' "$resolved_json")
# Hub releases sell proceeds through the portal.
cast send --rpc-url "$BASE_RPC_HTTP" --private-key "$DEPLOYER_PK" \
    "$portal" "setHubReleaser(bytes32)" "$(addr32 "$hub_sender")" >/dev/null

# Persist for the manifest step. `chutes` stays as an alias for the first
# catalog token (existing tooling reads it).
jq -n \
    --slurpfile tokens "$resolved_json" \
    --arg btMailbox "$bt_mailbox" \
    --arg canonicalUsd "$canonical_usd" \
    --arg gateway "$gateway" \
    --arg hubSender "$hub_sender" \
    --argjson netuid "$netuid" \
    --arg baseMailbox "$base_mailbox" \
    --arg mockUsdc "$mock_usdc" \
    --arg portal "$portal" \
    '{
        btlocal: {
            mailbox: $btMailbox,
            canonicalUsd: $canonicalUsd,
            gateway: $gateway,
            hubSender: $hubSender,
            netuid: $netuid
        },
        baselocal: {
            mailbox: $baseMailbox,
            mockUsdc: $mockUsdc,
            portal: $portal,
            chutes: $tokens[0][0].address,
            tokens: $tokens[0]
        }
    }' >"$RAILS_STATE_DIR/contracts.json"
echo "[rails] contracts deployed and wired ($token_count subnets, first netuid $netuid)"
