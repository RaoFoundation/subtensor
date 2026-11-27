#!/usr/bin/env bash
# Deploys the transport's core contracts (Mailbox, security module, hooks) to both local
# chains with the Hyperlane CLI, and writes the configuration its agents read.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

hyperlane() { npx --yes "$HYPERLANE_CLI" "$@"; }

rpc_of() { [[ "$1" == "$HUB_NAME" ]] && echo "$HUB_RPC" || echo "$SPOKE_RPC"; }

for chain in "$HUB_NAME" "$SPOKE_NAME"; do
    mkdir -p "$RAILS_REGISTRY/chains/$chain"
    cp "$RAILS_SCRIPT_DIR/hyperlane/$chain.metadata.yaml" "$RAILS_REGISTRY/chains/$chain/metadata.yaml"
done

for chain in "$HUB_NAME" "$SPOKE_NAME"; do
    addresses="$RAILS_REGISTRY/chains/$chain/addresses.yaml"
    if [[ -f "$addresses" ]]; then
        # The registry outlives a chain. Trust it only if the Mailbox it names has code.
        mailbox="$(awk '$1 == "mailbox:" { print $2; exit }' "$addresses" | tr -d '"')"
        if [[ -n "$mailbox" && "$(cast code "$mailbox" --rpc-url "$(rpc_of "$chain")")" != "0x" ]]; then
            say "the transport is already on $chain"
            continue
        fi
        rm -f "$addresses"
    fi
    say "deploying the transport to $chain"
    hyperlane core deploy \
        --registry "$RAILS_REGISTRY" \
        --overrides " " \
        --chain "$chain" \
        --config "$RAILS_SCRIPT_DIR/hyperlane/core-config.yaml" \
        --key "$DEPLOYER_KEY" \
        --yes >"$RAILS_LOGS/hyperlane-$chain.log" 2>&1 ||
        { tail -n 30 "$RAILS_LOGS/hyperlane-$chain.log"; die "the transport did not deploy on $chain"; }
    [[ -f "$addresses" ]] || die "the transport's deploy on $chain wrote no addresses"
done

say "writing the agents' configuration"
hyperlane registry agent-config \
    --registry "$RAILS_REGISTRY" \
    --overrides " " \
    --chains "$HUB_NAME" "$SPOKE_NAME" \
    --yes \
    --out "$RAILS_AGENTS/agent-config.json" >"$RAILS_LOGS/hyperlane-agent-config.log" 2>&1 ||
    { tail -n 30 "$RAILS_LOGS/hyperlane-agent-config.log"; die "no agent configuration"; }

# The agents run in containers: they reach the chains through the host.
jq --arg hub "$HUB_RPC_DOCKER" --arg spoke "$SPOKE_RPC_DOCKER" \
    --arg hubName "$HUB_NAME" --arg spokeName "$SPOKE_NAME" \
    '.chains[$hubName].rpcUrls = [{http: $hub}] | .chains[$spokeName].rpcUrls = [{http: $spoke}]' \
    "$RAILS_AGENTS/agent-config.json" >"$RAILS_AGENTS/agent-config.docker.json"
say "the transport is deployed"
