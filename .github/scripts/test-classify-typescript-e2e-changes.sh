#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
classifier="$script_dir/classify-typescript-e2e-changes.sh"
output=$(mktemp)
trap 'rm -f "$output"' EXIT

value() {
  sed -n "s/^$1=//p" "$output"
}

classify() {
  : > "$output"
  printf '%s\n' "$@" | "$classifier" "$output"
  assert_selected_builds_available
}

assert_value() {
  local key=$1 expected=$2 actual
  actual=$(value "$key")
  if [[ "$actual" != "$expected" ]]; then
    echo "expected $key=$expected, got $actual" >&2
    cat "$output" >&2
    exit 1
  fi
}

assert_selected_builds_available() {
  local matrix
  matrix=$(value build_matrix)

  if [[ "$(value evm)" == true || "$(value staking)" == true || \
        "$(value coldkey_swap)" == true || "$(value subnets)" == true ]]; then
    if [[ "$matrix" != *'"variant":"fast"'* ]]; then
      echo "a fast-runtime suite was selected without a fast build" >&2
      cat "$output" >&2
      exit 1
    fi
  fi

  if [[ "$(value dev)" == true || "$(value shield)" == true ]]; then
    if [[ "$matrix" != *'"variant":"release"'* ]]; then
      echo "a release suite was selected without a release build" >&2
      cat "$output" >&2
      exit 1
    fi
  fi
}

classify README.md Cargo.lock
assert_value e2e false
assert_value state_count 0
assert_value build_count 0
assert_value build_matrix '{"include":[]}'

classify ts-tests/suites/zombienet_evm/precompile.test.ts
assert_value evm true
assert_value state_count 1
assert_value shield false
assert_value build_count 1
assert_value build_matrix '{"include":[{"variant":"fast","flags":"--features fast-runtime"}]}'

# Core production code is cross-suite: staking is also exercised by EVM,
# dev, and subnet setup, so it must retain the complete matrix.
classify pallets/subtensor/src/staking/stake.rs
assert_value evm true
assert_value staking true
assert_value coldkey_swap true
assert_value dev true
assert_value subnets true
assert_value shield true
assert_value state_count 5
assert_value build_count 2

classify pallets/subtensor/src/subnets/registration.rs
assert_value evm true
assert_value subnets true
assert_value staking true
assert_value coldkey_swap true
assert_value dev true
assert_value shield true
assert_value state_count 5
assert_value build_count 2

classify pallets/subtensor/src/swap/swap_stake.rs
assert_value evm true
assert_value staking true
assert_value coldkey_swap true
assert_value dev true
assert_value subnets true
assert_value shield true
assert_value state_count 5
assert_value build_count 2

classify precompiles/subtensor/src/lib.rs
assert_value evm true
assert_value dev true
assert_value state_count 2
assert_value build_count 2

classify pallets/shield/src/lib.rs
assert_value shield true
assert_value dev true
assert_value state_count 1
assert_value build_count 1
assert_value build_matrix '{"include":[{"variant":"release","flags":""}]}'

classify pallets/subtensor/src/tests/staking.rs
assert_value e2e false
assert_value build_count 0

classify pallets/subtensor/src/epoch/run_epoch.rs
assert_value e2e true
assert_value state_count 5
assert_value shield true
assert_value build_count 2

classify ts-tests/suites/zombienet_staking/staking.test.ts ts-tests/suites/zombienet_shield/shield.test.ts
assert_value staking true
assert_value shield true
assert_value state_count 1
assert_value build_count 2

classify ts-tests/suites/dev/staking.test.ts
assert_value dev true
assert_value state_count 1
assert_value build_count 1
assert_value build_matrix '{"include":[{"variant":"release","flags":""}]}'

: > "$output"
"$classifier" --all "$output"
assert_selected_builds_available
assert_value e2e true
assert_value state_count 5
assert_value shield true
assert_value build_count 2
assert_value build_matrix '{"include":[{"variant":"fast","flags":"--features fast-runtime"},{"variant":"release","flags":""}]}'

echo "typescript E2E change classifier tests passed"
