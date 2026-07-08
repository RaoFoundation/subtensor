#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

CLONE_DIR="clones/mainnet-clone"

rm -rf "${CLONE_DIR}"

exec target/release/node-subtensor \
  --base-path "${CLONE_DIR}" \
  --chain clones/mainnet-clone-chainspec.json \
  --database paritydb \
  --force-authoring \
  --alice \
  --validator \
  --unsafe-force-node-key-generation \
  "$@"
