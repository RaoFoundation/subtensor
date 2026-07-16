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

classify README.md Cargo.lock
assert_value e2e false
assert_value state_count 0

classify ts-tests/suites/zombienet_evm/precompile.test.ts
assert_value evm true
assert_value state_count 1
assert_value shield false

classify pallets/subtensor/src/staking/stake.rs
assert_value staking true
assert_value coldkey_swap true
assert_value state_count 2

classify pallets/subtensor/src/subnets/registration.rs
assert_value subnets true
assert_value staking true
assert_value coldkey_swap true
assert_value state_count 3

classify precompiles/subtensor/src/lib.rs
assert_value evm true
assert_value dev true
assert_value state_count 2

classify pallets/shield/src/lib.rs
assert_value shield true
assert_value dev true
assert_value state_count 1

classify pallets/subtensor/src/tests/staking.rs
assert_value e2e false

classify pallets/subtensor/src/epoch/run_epoch.rs
assert_value e2e true
assert_value state_count 5
assert_value shield true

classify ts-tests/suites/zombienet_staking/staking.test.ts ts-tests/suites/zombienet_shield/shield.test.ts
assert_value staking true
assert_value shield true
assert_value state_count 1

: > "$output"
"$classifier" --all "$output"
assert_value e2e true
assert_value state_count 5
assert_value shield true

echo "typescript E2E change classifier tests passed"
