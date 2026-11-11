#!/usr/bin/env bash
# Run the real btcli lifecycle against a disposable, single-validator chain.
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
NODE=${PROTECTED_E2E_NODE:-$ROOT/target/release/node-subtensor}
RPC_PORT=${PROTECTED_E2E_PORT:-9957}
EVIDENCE=${E2E_EVIDENCE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/protected-cli-e2e.XXXXXX")}
mkdir -p "$EVIDENCE"
NODE_PID=''
cleanup() {
  local status=$?
  if [[ -n "$NODE_PID" ]]; then
    kill "$NODE_PID" 2>/dev/null || true
    wait "$NODE_PID" 2>/dev/null || true
  fi
  if [[ "$status" != 0 ]]; then
    tail -n 80 "$EVIDENCE/node.log"
  fi
  exit "$status"
}
trap cleanup EXIT

"$NODE" --chain dev --tmp --alice --validator --sealing 1000 \
  --rpc-port "$RPC_PORT" --port 0 --prometheus-port 0 --rpc-methods Unsafe \
  --no-telemetry --no-mdns --unsafe-force-node-key-generation >"$EVIDENCE/node.log" 2>&1 &
NODE_PID=$!
ready=false
for _ in $(seq 1 60); do
  kill -0 "$NODE_PID" 2>/dev/null || break
  # Never attach to an unrelated node that already occupies the requested
  # port. Our own process must have successfully published its RPC listener.
  if ! grep -Eq "Running JSON-RPC server: addr=127[.]0[.]0[.]1:${RPC_PORT}([, ]|$)" \
    "$EVIDENCE/node.log"; then
    sleep 1
    continue
  fi
  if curl --fail --silent --max-time 2 -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"state_getRuntimeVersion","params":[]}' \
    "http://127.0.0.1:$RPC_PORT" | python3 -c \
    'import json,sys; assert "specVersion" in json.load(sys.stdin)["result"]' 2>/dev/null; then
    ready=true
    break
  fi
  sleep 1
done
[[ "$ready" == true ]] || { echo "Disposable node did not start" >&2; exit 1; }
cd "$ROOT/sdk/python"
E2E_WALLET_ENDPOINT="ws://127.0.0.1:$RPC_PORT" E2E_EVIDENCE_DIR="$EVIDENCE" \
  INSTA_UPDATE=no uv run --no-sync pytest tests/e2e/test_protected_cli.py -vv
