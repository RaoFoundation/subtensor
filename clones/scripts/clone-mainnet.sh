#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

CLONE_DIR="clones/mainnet-clone"
CHAINSPEC_FILE="clones/mainnet-clone-chainspec.json"

# Warp sync waits until it reaches a peer whose release can serve finality
# proofs; observed successful searches took up to 35 minutes, after which proof
# and state download finish in a few minutes. A search that has not succeeded
# within the per-attempt limit retries with a fresh peer set.
SYNC_ATTEMPTS="${CLONE_SYNC_ATTEMPTS:-2}"
SYNC_TIMEOUT_SEC="${CLONE_SYNC_TIMEOUT_SEC:-2700}"

# Resync only if the chainspec file is missing. A missing clone directory is OK:
# it means the local chain should restart from the existing spec.
if [ -f "${CHAINSPEC_FILE}" ]; then
  echo "Chainspec file exists, keeping existing data."
  exit 0
fi

echo "Chainspec file is missing."
for attempt in $(seq 1 "${SYNC_ATTEMPTS}"); do
  echo "Deleting and rebuilding clone data (attempt ${attempt}/${SYNC_ATTEMPTS})..."
  rm -rf "${CLONE_DIR}"
  rm -f "${CHAINSPEC_FILE}"

  if target/release/node-subtensor build-patched-spec \
    --base-path "${CLONE_DIR}" \
    --chain chainspecs/raw_spec_finney.json \
    --bootnodes /dns/bootnode.finney.chain.opentensor.ai/tcp/30333/ws/p2p/12D3KooWRwbMb85RWnT8DSXSYMWQtuDwh4LJzndoRrTDotTR5gDC \
    --sync-timeout-sec "${SYNC_TIMEOUT_SEC}" \
    --output "${CHAINSPEC_FILE}"; then
    exit 0
  fi
  echo "Mainnet scrape attempt ${attempt} failed or timed out." >&2
done

rm -rf "${CLONE_DIR}"
rm -f "${CHAINSPEC_FILE}"
echo "Mainnet scrape failed after ${SYNC_ATTEMPTS} attempts." >&2
exit 1
