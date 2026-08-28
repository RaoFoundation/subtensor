#!/usr/bin/env bash
# Tear down the local rails rig: agents, anvil, localnet. Keeps .rails state
# (registry, manifest) unless --purge is passed.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

"$SCRIPT_DIR/agents.sh" stop

if [ -f "$RAILS_STATE_DIR/anvil.pid" ]; then
    kill "$(cat "$RAILS_STATE_DIR/anvil.pid")" 2>/dev/null || true
    rm -f "$RAILS_STATE_DIR/anvil.pid"
fi
# Belt and braces: the pid file can be stale (e.g. after --purge), but a
# surviving anvil keeps old CREATE2 deployments and breaks the next rig.
for pid in $(lsof -ti tcp:"$ANVIL_PORT" 2>/dev/null); do
    kill "$pid" 2>/dev/null || true
done
echo "[rails] anvil stopped"

# localnet.sh traps TERM and shuts down its three nodes cleanly.
if [ -f "$RAILS_STATE_DIR/localnet-runner.pid" ]; then
    kill "$(cat "$RAILS_STATE_DIR/localnet-runner.pid")" 2>/dev/null || true
    rm -f "$RAILS_STATE_DIR/localnet-runner.pid"
    echo "[rails] localnet stopped"
fi
# Belt and braces: any nodes recorded by localnet.sh itself.
if [ -f /tmp/subtensor-localnet.pids ]; then
    while IFS= read -r pid; do
        case "$pid" in
            ''|*[!0-9]*) continue ;;
        esac
        kill "$pid" 2>/dev/null || true
    done </tmp/subtensor-localnet.pids
fi

if [ "${1:-}" = "--purge" ]; then
    rm -rf "$RAILS_STATE_DIR"
    echo "[rails] state purged"
fi

echo "[rails] rig is down"
