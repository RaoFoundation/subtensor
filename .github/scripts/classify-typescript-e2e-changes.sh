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

      # Rust unit-test-only edits cannot change the node exercised by E2E.
      pallets/subtensor/src/tests/*|pallets/*/src/tests/*)
        ;;

      # These production areas have tightly owned E2E coverage. Include
      # adjacent suites where their setup depends on the same state.
      precompiles/*)
        evm=true
        dev=true
        ;;
      pallets/shield/*|pallets/limit-orders/*)
        shield=true
        dev=true
        ;;
      pallets/subtensor/src/staking/*)
        staking=true
        coldkey_swap=true
        ;;
      pallets/subtensor/src/subnets/*)
        subnets=true
        staking=true
        coldkey_swap=true
        ;;
      pallets/subtensor/src/swap/*|pallets/subtensor/src/guards/check_coldkey_swap.rs)
        staking=true
        coldkey_swap=true
        ;;

      # Shared test infrastructure and chain/runtime inputs can affect any
      # suite. Unknown paths inside an E2E-relevant tree deliberately fall
      # back to the complete matrix rather than guessing.
      ts-tests/*|common/*|node/*|pallets/*|primitives/*|runtime/*|support/*|chain-extensions/*|src/*|vendor/*|Cargo.toml|build.rs|rust-toolchain.toml|.github/workflows/typescript-e2e.yml|.github/actions/run-typescript-e2e/*|.github/actions/rust-setup/*|.github/actions/sccache-setup/*|.github/scripts/sccache-configure.sh|.github/scripts/sccache-report.sh|.github/scripts/classify-typescript-e2e-changes.sh|.github/scripts/test-classify-typescript-e2e-changes.sh)
        enable_all
        ;;
    esac
  done
fi

state_entries=()
[[ "$evm" != true ]] || state_entries+=('{"test":"zombienet_evm","binary":"fast"}')
[[ "$staking" != true ]] || state_entries+=('{"test":"zombienet_staking","binary":"fast"}')
[[ "$coldkey_swap" != true ]] || state_entries+=('{"test":"zombienet_coldkey_swap","binary":"fast"}')
[[ "$dev" != true ]] || state_entries+=('{"test":"dev","binary":"release"}')
[[ "$subnets" != true ]] || state_entries+=('{"test":"zombienet_subnets","binary":"fast"}')

state_matrix='{"include":['
if (( ${#state_entries[@]} > 0 )); then
  joined=$(IFS=,; echo "${state_entries[*]}")
  state_matrix+="$joined"
fi
state_matrix+=']}'

e2e=false
if (( ${#state_entries[@]} > 0 )) || [[ "$shield" == true ]]; then
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
  echo "state_count=${#state_entries[@]}"
  echo "state_matrix=$state_matrix"
} >> "$output_file"
