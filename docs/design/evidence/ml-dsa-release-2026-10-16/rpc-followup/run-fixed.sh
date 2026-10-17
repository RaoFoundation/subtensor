#!/usr/bin/env bash
set -euo pipefail
cd /home/johnreed/Desktop/work/subtensor
root=/tmp/ms-release-validation/rpc-followup
export BUILD_BINARY=0
export RPC_TEST_BINARY="$root/node-subtensor"
export LOCALNET_PID_FILE="$root/localnet.pids"
export PYTHONPATH="/tmp/ms-cleanup-native:$PWD/sdk/python"
bash "$root/localnet.sh" False > "$root/fixed-localnet.log" 2>&1 &
launcher=$!
cleanup() {
  trap - EXIT INT TERM
  kill -TERM "$launcher" 2>/dev/null || true
  wait "$launcher" || true
}
trap cleanup EXIT INT TERM
sdk/python/.venv/bin/python - <<'PY'
import json,time,urllib.request
deadline=time.monotonic()+90
while True:
    try:
        for port in (20544,20545,20546):
            req=urllib.request.Request(f'http://127.0.0.1:{port}', data=json.dumps({'jsonrpc':'2.0','id':1,'method':'system_health','params':[]}).encode(), headers={'Content-Type':'application/json'})
            with urllib.request.urlopen(req,timeout=2) as r:
                health=json.load(r)['result']
            assert not health['isSyncing'] and health['peers'] == 2
        break
    except Exception:
        assert time.monotonic()<deadline,'localnet readiness timeout'
        time.sleep(1)
PY
sdk/python/.venv/bin/python "$root/live_rpc.py" fixed > "$root/fixed-live.log" 2>&1
sdk/python/.venv/bin/python "$root/ethereum_pending_transfers.py" --funder Alice > "$root/ethereum-live.log" 2>&1
sdk/python/.venv/bin/python "$root/pending_contract_semantics.py" > "$root/pending-contract-live.log" 2>&1
