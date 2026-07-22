#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 [--all] OUTPUT_FILE" >&2
  exit 2
}

all=false
if [[ "${1:-}" == "--all" ]]; then
  all=true
  shift
fi
[[ $# -eq 1 ]] || usage
output_file=$1

evm=false
staking=false
coldkey_swap=false
dev=false
subnets=false
shield=false
topology_audit=false

enable_all() {
  evm=true
  staking=true
  coldkey_swap=true
  dev=true
  subnets=true
  shield=true
}

if [[ "$all" == true ]]; then
  enable_all
else
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    case "$path" in
      # These surfaces cannot alter the node binaries or TypeScript harness.
      # Keep them explicit so the default below remains fail-closed.
      *.md|LICENSE|docs/*|website/*|sdk/*|eco-tests/*|ink-contract/*|clones/*|.maintain/*|.vscode/*|.agents/*|.claude/*)
        ;;

      # Tests that only change one suite do not need unrelated networks.
      ts-tests/suites/zombienet_evm/*)
        evm=true
        ;;
      ts-tests/suites/zombienet_staking/*)
        staking=true
        ;;
      ts-tests/suites/zombienet_coldkey_swap/*)
        coldkey_swap=true
        ;;
      ts-tests/suites/zombienet_subnets/*)
        subnets=true
        ;;
      ts-tests/suites/zombienet_shield/*)
        shield=true
        ;;
      ts-tests/suites/dev/*)
        dev=true
        ;;

      # Shard topology is proposed as data and expanded by a trusted generic
      # builder. When that contract changes, run the canonical unsharded
      # environments as an equivalence audit in addition to the fast shards.
      ts-tests/e2e-shards.json|ts-tests/e2e-suite-ownership.json|ts-tests/moonwall.config.json|ts-tests/configs/zombie_single_node.json|ts-tests/configs/zombie_extended.json|ts-tests/scripts/e2e-shard-plan.mjs|ts-tests/scripts/test-e2e-shard-plan.mjs|ts-tests/scripts/validate-e2e-config.mjs|.github/workflows/typescript-e2e.yml|.github/actions/run-typescript-e2e/*|.github/scripts/classify-typescript-e2e-changes.sh|.github/scripts/test-classify-typescript-e2e-changes.sh)
        enable_all
        topology_audit=true
        ;;

      # Rust unit-test-only edits cannot change the node exercised by E2E.
      pallets/*/src/tests/*|pallets/*/src/tests.rs|pallets/*/src/mock.rs|chain-extensions/src/tests.rs|chain-extensions/src/mock.rs|precompiles/src/mock.rs|node/tests/*|runtime/tests/*|support/*/tests/*|support/procedural-fork/src/pallet/parse/tests/*)
        ;;

      # Fresh-genesis E2E networks never execute on-runtime-upgrade hooks.
      # Migration changes are covered by the cached try-runtime replays and
      # the sudo-upgraded mainnet clone instead.
      pallets/subtensor/src/migrations/*)
        ;;

      # These isolated production areas have tightly owned E2E coverage.
      precompiles/*)
        evm=true
        ;;
      pallets/shield/*|pallets/limit-orders/*)
        shield=true
        dev=true
        ;;

      # Shared test infrastructure and chain/runtime inputs can affect any
      # suite. Unknown paths inside an E2E-relevant tree deliberately fall
      # back to the complete matrix rather than guessing. In particular,
      # subtensor staking, subnet, and swap code is exercised across several
      # nominally separate suites, so all production changes there stay full.
      ts-tests/*|common/*|node/*|pallets/*|primitives/*|runtime/*|support/*|chain-extensions/*|src/*|vendor/*|Cargo.toml|Cargo.lock|build.rs|rust-toolchain.toml|.github/actions/rust-setup/*|.github/actions/sccache-setup/*|.github/scripts/rust-setup-preflight.sh|.github/scripts/install-rust-toolchain.sh|.github/scripts/sccache-configure.sh|.github/scripts/sccache-config.py|.github/scripts/sccache-report.sh|.github/scripts/extract-pull-file-paths.sh)
        enable_all
        ;;

      # Unknown files may be future build inputs. Full coverage is cheaper
      # than silently teaching a required check to pass without exercising
      # them; known-safe surfaces belong in the explicit exemption above.
      *)
        enable_all
        ;;
    esac
  done
fi

e2e=false
if [[ "$evm" == true || "$staking" == true || "$coldkey_swap" == true ||
      "$dev" == true || "$subnets" == true || "$shield" == true ]]; then
  e2e=true
fi

{
  echo "e2e=$e2e"
  echo "evm=$evm"
  echo "staking=$staking"
  echo "coldkey_swap=$coldkey_swap"
  echo "dev=$dev"
  echo "subnets=$subnets"
  echo "shield=$shield"
  echo "topology_audit=$topology_audit"
} >> "$output_file"
