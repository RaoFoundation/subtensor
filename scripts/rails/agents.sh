#!/usr/bin/env bash
# Run the self-hosted Hyperlane agents in docker: one validator per chain
# (in production these are sidecars on subtensor validators) plus one relayer.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

CMD="${1:-start}"

CONTAINERS=(rails-validator-btlocal rails-validator-baselocal rails-relayer)

stop_all() {
    for c in "${CONTAINERS[@]}"; do
        docker rm -f "$c" >/dev/null 2>&1 || true
    done
}

start_validator() {
    local chain="$1"
    local name="rails-validator-$chain"
    mkdir -p "$RAILS_CHECKPOINT_DIR/$chain" "$RAILS_STATE_DIR/validator-db-$chain"
    # The agent image runs as uid 1000; state dirs must be writable.
    chmod -R a+rwX "$RAILS_CHECKPOINT_DIR/$chain" "$RAILS_STATE_DIR/validator-db-$chain"
    # The announced checkpoint location (file:///checkpoints/<chain>) must
    # resolve identically inside the relayer container, so validators and the
    # relayer all mount the whole checkpoint dir at /checkpoints.
    docker run -d --name "$name" \
        --add-host host.docker.internal:host-gateway \
        -v "$RAILS_AGENT_DIR/agent-config.docker.json:/config/agent-config.json:ro" \
        -v "$RAILS_CHECKPOINT_DIR:/checkpoints" \
        -v "$RAILS_STATE_DIR/validator-db-$chain:/db" \
        -e CONFIG_FILES=/config/agent-config.json \
        "$HYPERLANE_AGENT_IMAGE" \
        ./validator \
        --db /db \
        --originChainName "$chain" \
        --checkpointSyncer.type localStorage \
        --checkpointSyncer.path "/checkpoints/$chain" \
        --validator.key "$VALIDATOR_PK" \
        --chains."$chain".signer.key "$VALIDATOR_PK" \
        >/dev/null
    echo "[rails] started $name"
}

start_relayer() {
    mkdir -p "$RAILS_STATE_DIR/relayer-db"
    chmod -R a+rwX "$RAILS_STATE_DIR/relayer-db" "$RAILS_CHECKPOINT_DIR"
    docker run -d --name rails-relayer \
        --add-host host.docker.internal:host-gateway \
        -v "$RAILS_AGENT_DIR/agent-config.docker.json:/config/agent-config.json:ro" \
        -v "$RAILS_CHECKPOINT_DIR:/checkpoints:ro" \
        -v "$RAILS_STATE_DIR/relayer-db:/db" \
        -e CONFIG_FILES=/config/agent-config.json \
        "$HYPERLANE_AGENT_IMAGE" \
        ./relayer \
        --db /db \
        --relayChains "$BT_CHAIN_NAME,$BASE_CHAIN_NAME" \
        --allowLocalCheckpointSyncers true \
        --defaultSigner.key "$RELAYER_PK" \
        --gasPaymentEnforcement '[{"type": "none"}]' \
        >/dev/null
    echo "[rails] started rails-relayer"
}

case "$CMD" in
    start)
        stop_all
        start_validator "$BT_CHAIN_NAME"
        start_validator "$BASE_CHAIN_NAME"
        start_relayer
        ;;
    stop)
        stop_all
        echo "[rails] agents stopped"
        ;;
    logs)
        for c in "${CONTAINERS[@]}"; do
            echo "===== $c ====="
            docker logs --tail 20 "$c" 2>&1 || true
        done
        ;;
    *)
        echo "usage: agents.sh start|stop|logs" >&2
        exit 1
        ;;
esac
