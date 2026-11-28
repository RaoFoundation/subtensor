#!/usr/bin/env bash
# Deploys the rails' contracts: the gateway on the hub, then the vault and the portal on
# the spoke. Each deployment is the same script a production deployment runs.
#
# The rig's timelock delay is 0 seconds, so that the e2e can act as the owner. The
# production default of the scripts is 48 hours.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

mailbox_of() { # chain
    awk '$1 == "mailbox:" { print $2; exit }' "$RAILS_REGISTRY/chains/$1/addresses.yaml" | tr -d '"'
}

HUB_MAILBOX="$(mailbox_of "$HUB_NAME")"
SPOKE_MAILBOX="$(mailbox_of "$SPOKE_NAME")"
[[ -n "$HUB_MAILBOX" && -n "$SPOKE_MAILBOX" ]] || die "deploy the transport first (hyperlane.sh)"

deployed() { # file key rpc -> the address, if it has code
    local file="$RAILS_CONTRACTS/deployments/$1" address
    [[ -f "$file" ]] || return 1
    address="$(jq -r ".$2 // empty" "$file")"
    [[ -n "$address" && "$(cast code "$address" --rpc-url "$3")" != "0x" ]] || return 1
    echo "$address"
}

mkdir -p "$RAILS_CONTRACTS/deployments"
(cd "$RAILS_CONTRACTS" && forge soldeer install >"$RAILS_LOGS/soldeer.log" 2>&1) ||
    { tail -n 20 "$RAILS_LOGS/soldeer.log"; die "the contracts' dependencies did not install"; }

if GATEWAY="$(deployed hub.local.json gateway "$HUB_RPC")"; then
    say "the gateway is already on $HUB_NAME at $GATEWAY"
else
    say "deploying the gateway to $HUB_NAME"
    # --legacy: Bittensor's EVM prices gas the old way. --slow: one transaction a block.
    (cd "$RAILS_CONTRACTS" && MAILBOX="$HUB_MAILBOX" MULTISIG="$MULTISIG" GUARDIAN="$GUARDIAN" \
        TIMELOCK_DELAY=0 FLUSH_LIMIT=8 OUTPUT=hub.local.json \
        forge script script/DeployGateway.s.sol:DeployGateway \
        --rpc-url "$HUB_RPC" --private-key "$DEPLOYER_KEY" \
        --broadcast --slow --legacy >"$RAILS_LOGS/deploy-gateway.log" 2>&1) ||
        { tail -n 30 "$RAILS_LOGS/deploy-gateway.log"; die "the gateway did not deploy"; }
    GATEWAY="$(deployed hub.local.json gateway "$HUB_RPC")" || die "the gateway has no code"
fi

if PORTAL="$(deployed spoke.local.json portal "$SPOKE_RPC")"; then
    say "the portal is already on $SPOKE_NAME at $PORTAL"
else
    say "deploying the vault and the portal to $SPOKE_NAME"
    # The vault releases at most 1,000,000 of the stablecoin in an hour.
    (cd "$RAILS_CONTRACTS" && MAILBOX="$SPOKE_MAILBOX" HUB_DOMAIN="$HUB_DOMAIN" GATEWAY="$GATEWAY" \
        MULTISIG="$MULTISIG" GUARDIAN="$GUARDIAN" TIMELOCK_DELAY=0 \
        RELEASE_LIMIT=1000000000000 RELEASE_PERIOD=3600 \
        ANSWER_FEE="$ANSWER_FEE" FEE_RECIPIENT="$MULTISIG" OUTPUT=spoke.local.json \
        forge script script/DeploySpoke.s.sol:DeploySpoke \
        --rpc-url "$SPOKE_RPC" --private-key "$DEPLOYER_KEY" \
        --broadcast --slow >"$RAILS_LOGS/deploy-spoke.log" 2>&1) ||
        { tail -n 30 "$RAILS_LOGS/deploy-spoke.log"; die "the spoke did not deploy"; }
    PORTAL="$(deployed spoke.local.json portal "$SPOKE_RPC")" || die "the portal has no code"
fi

say "gateway $GATEWAY, portal $PORTAL"
