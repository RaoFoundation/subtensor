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

# Known-safe surfaces stay explicitly exempt. Everything else fails closed to
# runtime + SDK-drift coverage so future production roots and Cargo inputs do
# not silently bypass try-runtime or clone-upgrade.
while IFS= read -r path; do
  case "$path" in
    # Rust test modules are compiled and executed by Check Rust. They cannot
    # change the production node or runtime wasm consumed by this workflow, so
    # rebuilding and sudo-upgrading a mainnet clone adds no coverage.
    pallets/*/src/tests/*|pallets/*/src/tests.rs|pallets/*/src/mock.rs|chain-extensions/src/tests.rs|chain-extensions/src/mock.rs|precompiles/src/mock.rs|node/tests/*|runtime/tests/*|support/*/tests/*|support/procedural-fork/src/pallet/parse/tests/*)
      ;;
    common/*|node/*|pallets/*|precompiles/*|primitives/*|runtime/*|support/*|chain-extensions/*|src/*|vendor/*|Cargo.toml|Cargo.lock|build.rs|rust-toolchain.toml|.cargo/*)
      runtime=true
      sdk_drift=true
      ;;
    clones/*|website/apps/bittensor-website/scripts/*)
      runtime=true
      ;;
    .github/workflows/runtime-checks.yml|.github/workflows/refresh-mainnet-snapshot.yml|.github/actions/rust-setup/*|.github/actions/sccache-setup/*|.github/scripts/rust-setup-preflight.sh|.github/scripts/install-rust-toolchain.sh|.github/scripts/sccache-configure.sh|.github/scripts/sccache-config.py|.github/scripts/sccache-report.sh)
      runtime=true
      ;;
    .github/scripts/classify-runtime-changes.sh|.github/scripts/test-runtime-change-filter.sh|.github/scripts/snapshot-artifact.sh|.github/scripts/test-snapshot-artifact.sh|.github/scripts/download-artifact.sh|.github/scripts/test-download-artifact.sh|.github/scripts/select-shared-release-artifact.sh|.github/scripts/test-select-shared-release-artifact.sh|.github/scripts/r2-artifact-mirror.py|.github/scripts/test-r2-artifact-mirror.py|.github/scripts/publish-artifact-mirror.sh|.github/scripts/publish-current-run-artifact-mirror.sh|.github/scripts/prewarm-exact-runtime.sh|.github/scripts/test-prewarm-exact-runtime.sh|.github/scripts/benchmark-sccache-paired.sh|.github/scripts/benchmark-artifact-cache.sh|.github/scripts/test-clone-regression-phase.sh|clones/scripts/run-clone-regression-phase.sh)
      runtime=true
      snapshot_ci=true
      ;;
    *.md|LICENSE|docs/*|website/*|sdk/*|ts-tests/*|eco-tests/*|ink-contract/*|.maintain/*|.vscode/*|.agents/*|.claude/*|.github/scripts/prepare-sdk-dist.py|.github/scripts/test_prepare_sdk_dist.py)
      ;;
    .github/*)
      runtime=true
      sdk_drift=true
      snapshot_ci=true
      ;;
    *)
      runtime=true
      sdk_drift=true
      ;;
  esac

  case "$path" in
    docs/*|website/*|sdk/python/*|.github/workflows/runtime-checks.yml) docs=true ;;
  esac

  case "$path" in
    sdk/python/*|sdk/bittensor-core/*|sdk/bittensor-core-py/*|sdk/bittensor-core-wasm/*|Cargo.lock|.github/workflows/runtime-checks.yml|.github/scripts/prepare-sdk-dist.py|.github/scripts/test_prepare_sdk_dist.py)
      python_sdk=true
      ;;
  esac

  case "$path" in
    .github/workflows/runtime-checks.yml|.github/workflows/refresh-mainnet-snapshot.yml|clones/scripts/run-clone-regression-phase.sh)
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
