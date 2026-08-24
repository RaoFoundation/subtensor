#!/usr/bin/env bash
# Deploy Hyperlane core (mailbox, ISM, hooks) to both local chains using a
# file-based local registry. Idempotent: skips chains that already have
# deployed core addresses in the registry.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

HL="npx --yes @hyperlane-xyz/cli"

# Seed the local registry with chain metadata.
for chain in "$BT_CHAIN_NAME" "$BASE_CHAIN_NAME"; do
    mkdir -p "$RAILS_REGISTRY_DIR/chains/$chain"
    cp "$SCRIPT_DIR/hyperlane/$chain.metadata.yaml" \
        "$RAILS_REGISTRY_DIR/chains/$chain/metadata.yaml"
done

rpc_for_chain() { # chain -> rpc url
    if [ "$1" = "$BT_CHAIN_NAME" ]; then echo "$BT_RPC_HTTP"; else echo "$BASE_RPC_HTTP"; fi
}

for chain in "$BT_CHAIN_NAME" "$BASE_CHAIN_NAME"; do
    addresses="$RAILS_REGISTRY_DIR/chains/$chain/addresses.yaml"
    if [ -f "$addresses" ] && grep -q mailbox "$addresses"; then
        # The registry survives chain restarts; trust it only if the mailbox
        # actually has code on the (possibly fresh-from-genesis) chain.
        mailbox="$(awk '$1 == "mailbox:" { print $2; exit }' "$addresses" | tr -d '"')"
        if [ -n "$mailbox" ] && \
            [ "$(cast code "$mailbox" --rpc-url "$(rpc_for_chain "$chain")")" != "0x" ]; then
            echo "[rails] hyperlane core already deployed on $chain"
            continue
        fi
        echo "[rails] registry has $chain addresses but chain has no code; redeploying ..."
        rm -f "$addresses"
    fi
    echo "[rails] deploying hyperlane core to $chain ..."
    $HL core deploy \
        --registry "$RAILS_REGISTRY_DIR" \
        --overrides " " \
        --chain "$chain" \
        --config "$SCRIPT_DIR/hyperlane/core-config.yaml" \
        --key "$DEPLOYER_PK" \
        --yes 2>&1 | tee "$RAILS_LOG_DIR/hyperlane-core-$chain.log" | tail -5
    [ -f "$addresses" ] || { echo "[rails] ERROR: core deploy on $chain produced no addresses" >&2; exit 1; }
done

# Generate the agent config consumed by validator/relayer containers.
echo "[rails] generating agent config ..."
$HL registry agent-config \
    --registry "$RAILS_REGISTRY_DIR" \
    --overrides " " \
    --chains "$BT_CHAIN_NAME" "$BASE_CHAIN_NAME" \
    --yes \
    --out "$RAILS_AGENT_DIR/agent-config.json" 2>&1 | tail -2

# Containers reach host RPCs via host.docker.internal.
jq \
    --arg bt "$BT_RPC_DOCKER" \
    --arg base "$BASE_RPC_DOCKER" \
    '.chains.btlocal.rpcUrls = [{http: $bt}]
     | .chains.baselocal.rpcUrls = [{http: $base}]' \
    "$RAILS_AGENT_DIR/agent-config.json" >"$RAILS_AGENT_DIR/agent-config.docker.json"

echo "[rails] hyperlane core ready"
