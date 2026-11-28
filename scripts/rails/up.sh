#!/usr/bin/env bash
# Starts the local rig of the canonical rails from nothing:
#
#   1. the hub: a subtensor localnet built from this tree (fast runtime);
#   2. the spoke: anvil playing Base;
#   3. the transport: Hyperlane's core contracts on both, and its agents;
#   4. the rails' contracts: the gateway on the hub, the vault and the portal on the spoke;
#   5. the hub's configuration: bridge, pool, subnets, tokens and markets.
#
# It ends by writing .rails/manifest.json, which btcli, the e2e and the swap page read.
# A rig that already runs is stopped first: every run starts from two new chains.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

for tool in anvil forge cast jq docker npx uv curl; do
    command -v "$tool" >/dev/null || die "$tool is not installed"
done
[[ -d "$RAILS_REPO/sdk/python/.venv" ]] ||
    die "the SDK's environment is missing. Run: (cd sdk/python && uv sync --python 3.14 --locked --all-extras --dev)"

"$RAILS_SCRIPT_DIR/down.sh" >/dev/null
# The addresses of the last rig mean nothing on new chains.
rm -f "$RAILS_CONTRACTS"/deployments/*.local.json "$RAILS_MANIFEST" \
    "$RAILS_STATE/page.env" "$RAILS_STATE/validators.json"

say "1/7 the hub"
if [[ ! -x "$NODE_BINARY" ]]; then
    say "building the node (fast runtime); this takes a while"
    BUILD_BINARY=1 "$RAILS_REPO/scripts/localnet.sh" --build-only
fi
BUILD_BINARY=0 setsid nohup "$RAILS_REPO/scripts/localnet.sh" >"$RAILS_LOGS/localnet.log" 2>&1 &
echo $! >"$RAILS_STATE/localnet.pid"
wait_for_rpc "$HUB_RPC" "the hub" 180

say "2/7 the spoke"
setsid nohup anvil --chain-id "$SPOKE_CHAIN_ID" --port "$SPOKE_PORT" --block-time 1 --silent \
    >"$RAILS_LOGS/anvil.log" 2>&1 &
echo $! >"$RAILS_STATE/anvil.pid"
wait_for_rpc "$SPOKE_RPC" "the spoke" 30

say "3/7 the hub's EVM and the accounts that pay gas"
rails_python setup.py prepare
# The keeper feeds the price and flushes the hub's outbox: see keeper.py.
setsid nohup bash -c "source '$RAILS_SCRIPT_DIR/env.sh' && rails_python keeper.py" \
    >"$RAILS_LOGS/keeper.log" 2>&1 &
echo $! >"$RAILS_STATE/keeper.pid"

say "4/7 the transport"
"$RAILS_SCRIPT_DIR/hyperlane.sh"

say "5/7 the rails' contracts"
"$RAILS_SCRIPT_DIR/contracts.sh"

say "6/7 the transport's agents"
"$RAILS_SCRIPT_DIR/agents.sh" start --fresh

say "7/7 the hub's configuration"
rails_python setup.py configure

say "the rig is up. Manifest: $RAILS_MANIFEST"
say "try it:   just rails-e2e"
say "the page: just rails-page, then open http://localhost:3000/swap"
