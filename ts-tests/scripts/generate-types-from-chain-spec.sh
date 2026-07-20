#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/.."

export PAPI_CHAIN_SPEC_PATH="${PAPI_CHAIN_SPEC_PATH:-./specs/chain-spec.json}"
exec ./scripts/generate-types.sh
