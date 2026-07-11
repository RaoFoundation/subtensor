#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

CLONE_DIR="clones/mainnet-clone"

# Default: start from a clean genesis init (the dir may hold a mainnet sync
# left by build-patched-spec). CI sets KEEP_CLONE_DATA=1 when it restored an
# already-initialized local-chain database from the nightly snapshot, which
# skips the multi-minute genesis init from the ~2 GB chainspec.
if [ "${KEEP_CLONE_DATA:-0}" != "1" ]; then
  rm -rf "${CLONE_DIR}"
fi

exec target/release/node-subtensor \
  --base-path "${CLONE_DIR}" \
  --chain clones/mainnet-clone-chainspec.json \
  --database paritydb \
  --force-authoring \
  --alice \
  --validator \
  --unsafe-force-node-key-generation \
  "$@"
