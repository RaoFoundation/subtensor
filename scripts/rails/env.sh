#!/usr/bin/env bash
# Shared configuration for the local rails rig. Everything is local and
# deterministic: no real ETH, no testnets, no third parties.

# Repo layout
RAILS_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
RAILS_BASE_DIR="$(cd "$RAILS_SCRIPT_DIR/../.." &>/dev/null && pwd)"
RAILS_STATE_DIR="$RAILS_BASE_DIR/.rails"
RAILS_LOG_DIR="$RAILS_STATE_DIR/logs"
RAILS_MANIFEST="$RAILS_STATE_DIR/manifest.json"
RAILS_REGISTRY_DIR="$RAILS_STATE_DIR/registry"
RAILS_CHECKPOINT_DIR="$RAILS_STATE_DIR/checkpoints"
RAILS_AGENT_DIR="$RAILS_STATE_DIR/agents"

# Chains
BT_CHAIN_NAME="btlocal"
BT_CHAIN_ID=42
# Hyperlane domain id. Distinct from the EVM chain id: 42 collides with a
# known Hyperlane domain (lukso), which the agents reject for unknown names.
BT_DOMAIN=10042
BT_RPC_HTTP="http://127.0.0.1:9944"
BT_RPC_WS="ws://127.0.0.1:9944"
# RPC URL as seen from inside docker containers (agents).
BT_RPC_DOCKER="http://host.docker.internal:9944"

BASE_CHAIN_NAME="baselocal"
# Deliberately NOT real Base's 8453: wallets that already know mainnet Base
# would silently keep using its public RPC. The Hyperlane domain keeps 8453
# (it is baked into envelopes, routes, and contract constructor args).
BASE_CHAIN_ID=84530
BASE_DOMAIN=8453
ANVIL_PORT=8545
BASE_RPC_HTTP="http://127.0.0.1:${ANVIL_PORT}"
BASE_RPC_DOCKER="http://host.docker.internal:${ANVIL_PORT}"

# Well-known anvil developer keys (public test keys, never real funds).
DEPLOYER_PK="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
DEPLOYER_ADDR="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
VALIDATOR_PK="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
VALIDATOR_ADDR="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
RELAYER_PK="0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
RELAYER_ADDR="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"

# The USD rails precompile address (2068 = 0x814). The precompile itself is
# the PSM escrow: it holds the canonical USD ERC-20 reserves and moves them
# via EVM subcalls (msg.sender = 0x814).
RAILS_PRECOMPILE="0x0000000000000000000000000000000000000814"
RAILS_PSM_ESCROW="$RAILS_PRECOMPILE"

# Hyperlane agent docker image.
HYPERLANE_AGENT_IMAGE="gcr.io/abacus-labs-dev/hyperlane-agent:agents-v1.4.0"

# Node binary produced by scripts/localnet.sh (fast runtime).
NODE_BINARY="$RAILS_BASE_DIR/target/fast-runtime/release/node-subtensor"

mkdir -p "$RAILS_STATE_DIR" "$RAILS_LOG_DIR" "$RAILS_REGISTRY_DIR" \
    "$RAILS_CHECKPOINT_DIR" "$RAILS_AGENT_DIR"

# Wait until an HTTP JSON-RPC endpoint answers eth_chainId.
wait_for_rpc() {
    local url="$1" name="$2" tries="${3:-120}"
    for _ in $(seq 1 "$tries"); do
        if curl -sf -X POST -H 'Content-Type: application/json' \
            --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' \
            "$url" | grep -q result; then
            echo "[rails] $name RPC ready at $url"
            return 0
        fi
        sleep 1
    done
    echo "[rails] ERROR: $name RPC at $url not ready" >&2
    return 1
}
