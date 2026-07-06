#!/bin/bash
#
# Add ink contract metadata to polkadot-api descriptors.
# Requires the bittensor ink contract json file available in specs folder.
#
# Usage:
#   ./scripts/generate-ink-types.sh
#
set -euo pipefail

DESCRIPTORS_DIR="./.papi/contracts"
GENERATE_TYPES=false
if [ ! -d "$DESCRIPTORS_DIR" ] || [ -z "$(ls -A "$DESCRIPTORS_DIR" 2>/dev/null)" ]; then
  echo "==> Type descriptors not found or empty, will generate..."
  GENERATE_TYPES=true
else
  echo "==> Type descriptors already exist, skipping generation."
fi

if [ "$GENERATE_TYPES" = true ]; then

  echo "==> Generating Ink contract types..."
  pnpm generate-ink-types

  echo "==> Done generating Ink contract types."
  exit 0
else
  echo "==> Types are up-to-date, nothing to do."
fi
