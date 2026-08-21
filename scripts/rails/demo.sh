#!/usr/bin/env bash
# Serve the CHUTES demo page against the live rig. MetaMask requires an
# http(s) origin, so we serve the repo root (page fetches ../.rails/manifest.json).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

[ -f "$RAILS_MANIFEST" ] || { echo "[rails] no manifest; run up.sh first" >&2; exit 1; }

PORT="${DEMO_PORT:-8666}"
URL="http://127.0.0.1:${PORT}/demo/"

echo "[rails] serving demo at $URL"
command -v open >/dev/null && open "$URL" &
exec python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$RAILS_BASE_DIR"
