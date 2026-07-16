#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: classify-runtime-changes.sh OUTPUT_FILE" >&2
  exit 2
fi

output_file="$1"

runtime=false
docs=false
python_sdk=false
sdk_drift=false
snapshot_ci=false

# Keep path ownership explicit. SDK-only changes are covered by sdk-checks and
# the Rust SDK e2e workflow; they should not force clone-upgrade or SDK drift.
while IFS= read -r path; do
  case "$path" in
    # Rust test modules are compiled and executed by Check Rust. They cannot
    # change the production node or runtime wasm consumed by this workflow, so
    # rebuilding and sudo-upgrading a mainnet clone adds no coverage.
    pallets/*/src/tests/*|pallets/*/src/tests.rs|pallets/*/src/mock.rs)
      ;;
    common/*|node/*|pallets/*|precompiles/*|primitives/*|runtime/*|support/*|chain-extensions/*|src/*|vendor/*|Cargo.toml|build.rs|rust-toolchain.toml)
      runtime=true
      sdk_drift=true
      ;;
    clones/*|website/apps/bittensor-website/scripts/*)
      runtime=true
      ;;
    .github/workflows/runtime-checks.yml|.github/workflows/refresh-mainnet-snapshot.yml|.github/actions/rust-setup/*|.github/actions/sccache-setup/*|.github/scripts/sccache-configure.sh|.github/scripts/sccache-config.py|.github/scripts/sccache-report.sh)
      runtime=true
      ;;
    .github/scripts/classify-runtime-changes.sh|.github/scripts/test-runtime-change-filter.sh|.github/scripts/snapshot-artifact.sh|.github/scripts/test-snapshot-artifact.sh|.github/scripts/download-artifact.sh|.github/scripts/test-download-artifact.sh|.github/scripts/r2-artifact-mirror.py|.github/scripts/test-r2-artifact-mirror.py|.github/scripts/publish-artifact-mirror.sh|.github/scripts/publish-current-run-artifact-mirror.sh|.github/scripts/prewarm-exact-runtime.sh|.github/scripts/test-prewarm-exact-runtime.sh|.github/scripts/benchmark-sccache-paired.sh|.github/scripts/benchmark-artifact-cache.sh)
      runtime=true
      snapshot_ci=true
      ;;
  esac

  case "$path" in
    website/*|sdk/python/*|.github/workflows/runtime-checks.yml) docs=true ;;
  esac

  case "$path" in
    sdk/python/*|sdk/bittensor-core/*|sdk/bittensor-core-py/*|sdk/bittensor-core-wasm/*|Cargo.lock|.github/workflows/runtime-checks.yml)
      python_sdk=true
      ;;
  esac

  case "$path" in
    .github/workflows/runtime-checks.yml|.github/workflows/refresh-mainnet-snapshot.yml)
      snapshot_ci=true
      ;;
  esac
done

echo "runtime=$runtime, docs=$docs, python_sdk=$python_sdk, sdk_drift=$sdk_drift, snapshot_ci=$snapshot_ci"
{
  echo "runtime=$runtime"
  echo "docs=$docs"
  echo "python_sdk=$python_sdk"
  echo "sdk_drift=$sdk_drift"
  echo "snapshot_ci=$snapshot_ci"
} >> "$output_file"
