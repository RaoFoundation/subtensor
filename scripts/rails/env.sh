#!/usr/bin/env bash
# Shared settings of the local rails rig. Everything here is local: two chains on this
# machine, the transport between them, and well-known development keys that hold nothing.
#
# The rig is:
#   - a subtensor localnet built from this tree (fast runtime), the hub;
#   - an anvil chain that plays Base, the spoke;
#   - Hyperlane core contracts on both, and the agents that carry messages between them;
#   - the rails contracts: the gateway on the hub, the vault and the portal on the spoke.

RAILS_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
RAILS_REPO="$(cd "$RAILS_SCRIPT_DIR/../.." &>/dev/null && pwd)"
RAILS_STATE="${RAILS_STATE:-$RAILS_REPO/.rails}"
RAILS_LOGS="$RAILS_STATE/logs"
RAILS_REGISTRY="$RAILS_STATE/registry"
RAILS_AGENTS="$RAILS_STATE/agents"
RAILS_MANIFEST="$RAILS_STATE/manifest.json"
RAILS_CONTRACTS="$RAILS_REPO/contracts/evm"

# The hub: the subtensor localnet. Its EVM chain id is set by `btcli evm setup-localnet`.
HUB_NAME="btlocal"
HUB_CHAIN_ID=945
# The transport's domain id. Not the chain id: Hyperlane already knows other chains by the
# small numbers a localnet uses.
HUB_DOMAIN=10945
HUB_WS="ws://127.0.0.1:9944"
HUB_RPC="http://127.0.0.1:9944"

# The spoke: anvil playing Base. Its chain id is not Base's 8453, so that a wallet which
# knows Base does not mistake one for the other. The transport's domain is Base's.
SPOKE_NAME="baselocal"
SPOKE_CHAIN_ID=84530
SPOKE_DOMAIN=8453
SPOKE_PORT=8545
SPOKE_RPC="http://127.0.0.1:$SPOKE_PORT"
# The bridge id the hub knows the spoke's portal by.
SPOKE_BRIDGE=8453

# The transport's agents run in containers and reach the chains on the host. On Linux they
# share the host's network. Elsewhere (Docker Desktop) they go through host.docker.internal.
if [[ "$(uname -s)" == "Linux" ]]; then
    AGENT_NETWORK=(--network host)
    HUB_RPC_DOCKER="$HUB_RPC"
    SPOKE_RPC_DOCKER="$SPOKE_RPC"
else
    AGENT_NETWORK=(--add-host host.docker.internal:host-gateway)
    HUB_RPC_DOCKER="http://host.docker.internal:9944"
    SPOKE_RPC_DOCKER="http://host.docker.internal:$SPOKE_PORT"
fi

# Anvil's development accounts. Public keys of no value: never use them elsewhere.
DEPLOYER_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
DEPLOYER="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
VALIDATOR_KEY="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
VALIDATOR="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
RELAYER_KEY="0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
RELAYER="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
# The multisig and the guardian of the rig: plain accounts, so the e2e can act as both.
MULTISIG_KEY="0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6"
MULTISIG="0x90F79bf6EB2c4f870365E785982E1f101E93b906"
GUARDIAN_KEY="0x47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a"
GUARDIAN="0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65"
# The keeper: it asks the gateway to send what waits in the hub's outbox.
KEEPER_KEY="0x92db14e403b83dfe3df233f83dfa3a0d7096f21ca9b0d6d6b8d88b2b4ec1564e"
KEEPER="0x976EA74026E726554dB657fA54763abd0C3a0aa9"
# The person who uses the rails in the e2e, and the swap page's test account.
USER_KEY="0x8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba"
USER_ADDRESS="0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc"

# Who pays for the hub's messages to the spoke.
# - ANSWER_FEE: what each order pays on the spoke for the hub's answer, in wei.
# - TRANSPORT_FEE: what the rig's transport charges for one message, in rao. It is the
#   protocol fee in hyperlane/core-config.yaml: keep the two the same.
# - MESSAGE_RESERVE: what the hub sets aside for one message to the spoke, in rao. The
#   spoke's account on the hub holds this for each queued message, and an exit pays it in.
# - MESSAGE_CEILING: the most the hub pays for one message to the spoke, in rao. Above the
#   reserve it pays from what the spoke's account can spare.
# - MESSAGE_BUDGET: what the rig pays into the spoke's account at the start, in rao.
ANSWER_FEE=100000000000000
TRANSPORT_FEE=1000
MESSAGE_RESERVE=1000000
MESSAGE_CEILING=2000000
MESSAGE_BUDGET=1000000000

# Pinned versions of the transport's tooling.
HYPERLANE_CLI="@hyperlane-xyz/cli@45.2.0"
HYPERLANE_AGENT_IMAGE="gcr.io/abacus-labs-dev/hyperlane-agent:agents-v1.4.0"

# The node that scripts/localnet.sh builds and runs.
NODE_BINARY="$RAILS_REPO/target/fast-runtime/release/node-subtensor"

export PATH="$HOME/.foundry/bin:$HOME/.local/bin:$PATH"
mkdir -p "$RAILS_STATE" "$RAILS_LOGS" "$RAILS_REGISTRY" "$RAILS_AGENTS"

# The rig's Python scripts read these settings from the environment.
export RAILS_STATE RAILS_MANIFEST HUB_WS HUB_RPC HUB_CHAIN_ID HUB_DOMAIN \
    SPOKE_NAME SPOKE_RPC SPOKE_BRIDGE SPOKE_DOMAIN SPOKE_CHAIN_ID \
    DEPLOYER_KEY DEPLOYER VALIDATOR RELAYER MULTISIG_KEY MULTISIG GUARDIAN_KEY GUARDIAN \
    KEEPER_KEY KEEPER USER_KEY USER_ADDRESS ANSWER_FEE TRANSPORT_FEE MESSAGE_RESERVE \
    MESSAGE_CEILING MESSAGE_BUDGET

# Runs one of the rig's Python scripts in the SDK's environment.
rails_python() { # script [arguments]
    local script="$1"
    shift
    (cd "$RAILS_REPO/sdk/python" && uv run --no-sync python "$RAILS_SCRIPT_DIR/$script" "$@")
}

say() { printf '[rails] %s\n' "$*"; }
die() { printf '[rails] error: %s\n' "$*" >&2; exit 1; }

# Waits until an Ethereum JSON-RPC endpoint answers.
wait_for_rpc() { # url name [tries]
    local url="$1" name="$2" tries="${3:-120}" i
    for ((i = 0; i < tries; i++)); do
        if curl -sf -m 3 -X POST -H 'Content-Type: application/json' \
            --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' "$url" |
            grep -q result; then
            say "$name answers at $url"
            return 0
        fi
        sleep 1
    done
    die "$name at $url did not answer"
}

rpc_up() { # url
    curl -sf -m 3 -X POST -H 'Content-Type: application/json' \
        --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' "$1" >/dev/null 2>&1
}
