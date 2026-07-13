#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: classify-runtime-changes.sh OUTPUT_FILE" >&2
  exit 2
fi

output_file="$1"

# SDK-only changes are covered by sdk-checks and the Rust SDK e2e workflow;
# they should not force clone-upgrade or SDK drift.
runtime_pattern='^(common|node|pallets|precompiles|primitives|runtime|support|chain-extensions|src|vendor|clones)/|^(Cargo\.toml|build\.rs|rust-toolchain\.toml)$|^website/apps/bittensor-website/scripts/|^\.github/(workflows/(runtime-checks|refresh-mainnet-snapshot)\.yml|actions/(rust-setup|sccache-setup)/.*|scripts/(classify-runtime-changes|test-runtime-change-filter|sccache-configure|snapshot-artifact|start-accelerated-clone|test-snapshot-artifact)\.sh)$'
docs_pattern='^website/|^sdk/python/|^\.github/workflows/runtime-checks\.yml$'
python_sdk_pattern='^sdk/(python|bittensor-core|bittensor-core-py|bittensor-core-wasm)/|^Cargo\.lock$|^\.github/workflows/runtime-checks\.yml$'
sdk_drift_pattern='^(common|node|pallets|precompiles|primitives|runtime|support|chain-extensions|src|vendor)/|^(Cargo\.toml|build\.rs|rust-toolchain\.toml)$'
snapshot_pattern='^\.github/(workflows/(runtime-checks|refresh-mainnet-snapshot)\.yml|scripts/(classify-runtime-changes|test-runtime-change-filter|snapshot-artifact|test-snapshot-artifact)\.sh)$'

files=$(< /dev/stdin)
runtime=false
docs=false
python_sdk=false
sdk_drift=false
snapshot_ci=false
grep -qE "$runtime_pattern" <<< "$files" && runtime=true
grep -qE "$docs_pattern" <<< "$files" && docs=true
grep -qE "$python_sdk_pattern" <<< "$files" && python_sdk=true
grep -qE "$sdk_drift_pattern" <<< "$files" && sdk_drift=true
grep -qE "$snapshot_pattern" <<< "$files" && snapshot_ci=true

echo "runtime=$runtime, docs=$docs, python_sdk=$python_sdk, sdk_drift=$sdk_drift, snapshot_ci=$snapshot_ci"
{
  echo "runtime=$runtime"
  echo "docs=$docs"
  echo "python_sdk=$python_sdk"
  echo "sdk_drift=$sdk_drift"
  echo "snapshot_ci=$snapshot_ci"
} >> "$output_file"
