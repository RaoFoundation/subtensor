#!/usr/bin/env bash
# Bring up the full local rails rig:
#   subtensor localnet + anvil (fake Base) + Hyperlane core + self-run agents
#   + rails contracts, then write the rig manifest consumed by ts-tests/btcli.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

cd "$RAILS_BASE_DIR"

echo "[rails] === 1/7 localnet ==="
if curl -sf -X POST -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' \
    "$BT_RPC_HTTP" >/dev/null 2>&1; then
    echo "[rails] localnet already running"
else
    if [ ! -x "$NODE_BINARY" ]; then
        echo "[rails] node binary missing; building (this takes a while)..."
        BUILD_BINARY=1 "$RAILS_BASE_DIR/scripts/localnet.sh" --build-only
    fi
    BUILD_BINARY=0 nohup "$RAILS_BASE_DIR/scripts/localnet.sh" \
        >"$RAILS_LOG_DIR/localnet.log" 2>&1 &
    echo $! >"$RAILS_STATE_DIR/localnet-runner.pid"
    wait_for_rpc "$BT_RPC_HTTP" "localnet" 180
fi

echo "[rails] === 2/7 anvil (fake Base) ==="
if curl -sf -X POST -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' \
    "$BASE_RPC_HTTP" >/dev/null 2>&1; then
    echo "[rails] anvil already running"
else
    nohup anvil --chain-id "$BASE_CHAIN_ID" --port "$ANVIL_PORT" --block-time 1 \
        --silent >"$RAILS_LOG_DIR/anvil.log" 2>&1 &
    echo $! >"$RAILS_STATE_DIR/anvil.pid"
    wait_for_rpc "$BASE_RPC_HTTP" "anvil" 30
fi

echo "[rails] === 3/7 substrate bootstrap (whitelist + funding) ==="
(cd "$RAILS_BASE_DIR/ts-tests" && BT_RPC_WS="$BT_RPC_WS" pnpm exec tsx scripts/rails-bootstrap.ts)

echo "[rails] === 4/7 hyperlane core ==="
"$SCRIPT_DIR/hyperlane-deploy.sh"

echo "[rails] === 5/7 hyperlane agents ==="
"$SCRIPT_DIR/agents.sh" start

echo "[rails] === 6/7 rails contracts ==="
"$SCRIPT_DIR/deploy-contracts.sh"

echo "[rails] === 7/7 manifest ==="
"$SCRIPT_DIR/manifest.sh"

echo "[rails] rig is up. Manifest: $RAILS_MANIFEST"
