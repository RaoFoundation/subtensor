#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 [--all] OUTPUT_FILE" >&2
  exit 2
}

all=false
if [[ "${1:-}" == --all ]]; then
  all=true
  shift
fi
[[ $# -eq 1 ]] || usage
output_file=$1

e2e=false
build_image=false

enable_all() {
  e2e=true
  build_image=true
}

if [[ "$all" == true ]]; then
  enable_all
else
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    case "$path" in
      # Documentation and generated interface declarations are not loaded by
      # the native Rust SDK harness or its localnet node.
      *.md|LICENSE|docs/*|website/*|ts-tests/*|eco-tests/*|ink-contract/*|clones/*|.maintain/*|.vscode/*|.agents/*|.claude/*|precompiles/src/solidity/*)
        ;;

      # These files cannot alter the node or SDK binary exercised here. Their
      # owning unit/wasm/offline jobs still run, so starting 112 chain-facing
      # tests adds latency without adding coverage.
      pallets/*/src/tests/*|pallets/*/src/tests.rs|pallets/*/src/mock.rs|pallets/*/src/pallet/tests.rs|chain-extensions/src/tests.rs|chain-extensions/src/mock.rs|precompiles/src/mock.rs|node/tests/*|runtime/tests/*|support/*/tests/*|support/procedural-fork/src/pallet/parse/tests/*)
        ;;
      pallets/*/src/migrations/*)
        # Fresh localnet genesis does not execute on-runtime-upgrade hooks.
        # Cached try-runtime replays and clone-upgrade own migration coverage.
        ;;

      # Benchmark-only modules are compiled by the all-features Rust checks,
      # but the localnet deliberately does not enable runtime-benchmarks.
      pallets/*/src/benchmarking.rs|pallets/subtensor/src/benchmarks.rs|pallets/subtensor/src/benchmarks/*|node/src/benchmarking.rs)
        ;;

      # Repository maintenance binaries and lints are workspace-tested but do
      # not participate in either the native SDK harness or node build.
      support/linting/*|support/tools/*|support/weight-tools/*)
        ;;
      sdk/python/*|sdk/bittensor-core-py/*|sdk/bittensor-core-wasm/*)
        # This workflow executes the native bittensor-core Rust harness. The
        # Python and wasm bindings have dedicated compile/behavioral checks.
        ;;

      # The harness/Dockerfile itself needs both newly compiled tests and a
      # freshly built localnet image.
      sdk/bittensor-core/tests/Dockerfile.localnet-fast)
        enable_all
        ;;

      # Native core changes need the full 112-test harness, but can safely run
      # against the immutable image built from main when chain code is unchanged.
      sdk/bittensor-core/*)
        e2e=true
        ;;

      # Production chain inputs require a localnet image built from the PR.
      common/*|node/*|pallets/*|precompiles/*|primitives/*|runtime/*|support/*|chain-extensions/*|src/*|vendor/*|Cargo.toml|Cargo.lock|build.rs|rust-toolchain.toml|Dockerfile-localnet|snapshot.json|scripts/localnet.sh|scripts/localnet_patch.sh)
        enable_all
        ;;

      # CI plumbing changes are exercised fail-closed with the complete path.
      .github/workflows/check-bittensor-e2e-tests.yml|.github/workflows/docker-localnet.yml|.github/actions/rust-setup/*|.github/actions/sccache-setup/*|.github/scripts/rust-setup-preflight.sh|.github/scripts/install-rust-toolchain.sh|.github/scripts/sccache-configure.sh|.github/scripts/sccache-config.py|.github/scripts/sccache-report.sh|.github/scripts/classify-bittensor-e2e-changes.sh|.github/scripts/build-bittensor-e2e-matrix.py|.github/scripts/test-classify-bittensor-e2e-changes.sh|.github/scripts/extract-pull-file-paths.sh)
        enable_all
        ;;

      # A new SDK subtree may contain another chain-facing client. Prefer a
      # complete run until its ownership is explicitly classified.
      sdk/*)
        enable_all
        ;;

      # The workflow deliberately invokes this classifier for every PR. New
      # top-level files and build inputs must get coverage until they are
      # reviewed and placed in a known-safe exemption above.
      *)
        enable_all
        ;;
    esac
  done
fi

{
  echo "e2e=$e2e"
  echo "build_image=$build_image"
} >> "$output_file"
