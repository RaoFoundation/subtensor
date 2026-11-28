#!/usr/bin/env bash
# Stops the local rig: the agents, the keeper, the spoke and the hub. Pass --purge to
# also delete what the rig wrote under .rails/.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

# Stops the process group that a pid file names.
stop_group() { # name
    local file="$RAILS_STATE/$1.pid" pid
    [[ -f "$file" ]] || return 0
    pid="$(cat "$file")"
    rm -f "$file"
    [[ "$pid" =~ ^[0-9]+$ ]] || return 0
    kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
}

"$RAILS_SCRIPT_DIR/agents.sh" stop >/dev/null
stop_group keeper
stop_group anvil
# scripts/localnet.sh stops its nodes when it receives the signal.
stop_group localnet
for _ in $(seq 1 40); do
    rpc_up "$HUB_RPC" || rpc_up "$SPOKE_RPC" || break
    sleep 1
done
if rpc_up "$HUB_RPC" || rpc_up "$SPOKE_RPC"; then
    die "a chain still answers on the rig's ports; it was not started by this rig"
fi

if [[ "${1:-}" == "--purge" ]]; then
    rm -rf "$RAILS_STATE"
    say "deleted the rig's state"
fi
say "the rig is down"
