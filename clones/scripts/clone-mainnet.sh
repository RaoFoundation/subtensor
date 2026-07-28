#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

CLONE_DIR="clones/mainnet-clone"
CHAINSPEC_FILE="clones/mainnet-clone-chainspec.json"

NEEDS_RESYNC=false

# Resync only if the chainspec file is missing. A missing clone directory is OK:
# it means the local chain should restart from the existing spec.
if [ ! -f "${CHAINSPEC_FILE}" ]; then
  echo "Chainspec file is missing."
  NEEDS_RESYNC=true
else
  echo "Chainspec file exists, keeping existing data."
fi

if [ "${NEEDS_RESYNC}" = true ]; then
  echo "Deleting and rebuilding clone data..."

  rm -rf "${CLONE_DIR}"
  rm -f "${CHAINSPEC_FILE}"

  target/release/node-subtensor build-patched-spec \
    --base-path "${CLONE_DIR}" \
    --chain chainspecs/raw_spec_finney.json \
    --bootnodes /dns/bootnode.finney.chain.opentensor.ai/tcp/30333/ws/p2p/12D3KooWRwbMb85RWnT8DSXSYMWQtuDwh4LJzndoRrTDotTR5gDC \
    --output "${CHAINSPEC_FILE}"
fi
