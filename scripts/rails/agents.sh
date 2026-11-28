#!/usr/bin/env bash
# Runs the transport's agents in containers: one validator for each chain, which signs
# what the chain's Mailbox sent, and one relayer, which delivers it to the other chain.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

CONTAINERS=("rails-validator-$HUB_NAME" "rails-validator-$SPOKE_NAME" rails-relayer)
CHECKPOINTS="$RAILS_STATE/checkpoints"

stop_all() {
    local container
    for container in "${CONTAINERS[@]}"; do
        docker rm -f "$container" >/dev/null 2>&1 || true
    done
}

# What the agents stored belongs to the chains it was made on. New chains at the same
# addresses look like a reorganisation to a validator, and it stops signing.
forget_state() {
    # The containers wrote these files as their own user: remove them the same way.
    docker run --rm -v "$RAILS_STATE:/state" --entrypoint sh "$HYPERLANE_AGENT_IMAGE" \
        -c 'rm -rf /state/checkpoints /state/relayer-db /state/validator-db-*' >/dev/null 2>&1 || true
    rm -rf "$CHECKPOINTS" "$RAILS_STATE"/relayer-db "$RAILS_STATE"/validator-db-* 2>/dev/null || true
}

start_validator() { # chain metrics-port
    local chain="$1" metrics="$2"
    mkdir -p "$CHECKPOINTS/$chain" "$RAILS_STATE/validator-db-$chain"
    chmod -R a+rwX "$CHECKPOINTS" "$RAILS_STATE/validator-db-$chain"
    # Validators and the relayer mount the checkpoints at the same path: a validator
    # announces where its checkpoints are, and the relayer reads them there.
    docker run -d --name "rails-validator-$chain" "${AGENT_NETWORK[@]}" \
        -v "$RAILS_AGENTS/agent-config.docker.json:/config/agent-config.json:ro" \
        -v "$CHECKPOINTS:/checkpoints" \
        -v "$RAILS_STATE/validator-db-$chain:/db" \
        -e CONFIG_FILES=/config/agent-config.json \
        "$HYPERLANE_AGENT_IMAGE" ./validator \
        --db /db \
        --metrics-port "$metrics" \
        --originChainName "$chain" \
        --checkpointSyncer.type localStorage \
        --checkpointSyncer.path "/checkpoints/$chain" \
        --validator.key "$VALIDATOR_KEY" \
        --chains."$chain".signer.key "$VALIDATOR_KEY" >/dev/null
    say "started the validator of $chain"
}

start_relayer() {
    mkdir -p "$RAILS_STATE/relayer-db"
    chmod -R a+rwX "$RAILS_STATE/relayer-db" "$CHECKPOINTS"
    docker run -d --name rails-relayer "${AGENT_NETWORK[@]}" \
        -v "$RAILS_AGENTS/agent-config.docker.json:/config/agent-config.json:ro" \
        -v "$CHECKPOINTS:/checkpoints:ro" \
        -v "$RAILS_STATE/relayer-db:/db" \
        -e CONFIG_FILES=/config/agent-config.json \
        "$HYPERLANE_AGENT_IMAGE" ./relayer \
        --db /db \
        --metrics-port 9193 \
        --relayChains "$HUB_NAME,$SPOKE_NAME" \
        --allowLocalCheckpointSyncers true \
        --defaultSigner.key "$RELAYER_KEY" \
        --gasPaymentEnforcement '[{"type": "none"}]' >/dev/null
    say "started the relayer"
}

case "${1:-start}" in
    start)
        stop_all
        [[ "${2:-}" != "--fresh" ]] || forget_state
        # Each agent serves its metrics on its own port: on Linux they share the host's.
        start_validator "$HUB_NAME" 9191
        start_validator "$SPOKE_NAME" 9192
        start_relayer
        ;;
    stop)
        stop_all
        say "stopped the agents"
        ;;
    logs)
        for container in "${CONTAINERS[@]}"; do
            printf '===== %s =====\n' "$container"
            docker logs --tail "${2:-20}" "$container" 2>&1 || true
        done
        ;;
    *)
        die "usage: agents.sh start [--fresh] | stop | logs [lines]"
        ;;
esac
