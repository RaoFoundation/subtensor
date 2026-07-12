#!/usr/bin/env bash
set -euo pipefail

pkill -f "node-subtensor.*--base-path clones/mainnet-clone" \
  || pkill -f "node-subtensor.*--base-path ../clones/mainnet-clone" \
  || true
