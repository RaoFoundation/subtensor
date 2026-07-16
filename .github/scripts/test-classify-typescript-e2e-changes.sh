#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
classifier="$script_dir/classify-typescript-e2e-changes.sh"
extractor="$script_dir/extract-pull-file-paths.sh"
workflow="$script_dir/../workflows/typescript-e2e.yml"
output=$(mktemp)
paths=$(mktemp)
trap 'rm -f "$output" "$paths"' EXIT

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

# GitHub reports only the destination as filename for a rename. Preserve the
# previous path so moving production code out of a covered tree cannot bypass
# the E2E matrix.
printf '%s\n%s\n' \
  '[{"filename":"docs/moved.rs","previous_filename":"pallets/subtensor/src/staking.rs"}]' \
  '[{"filename":"README.md"}]' \
  | "$extractor" 2 > "$paths"
grep -qx 'docs/moved.rs' "$paths"
grep -qx 'pallets/subtensor/src/staking.rs' "$paths"
: > "$output"
"$classifier" "$output" < "$paths"
assert_value state_count 5
assert_value shield true

if printf '%s\n' '[{"filename":"README.md"}]' | "$extractor" 2 >/dev/null 2>&1; then
  echo "expected an incomplete pull-file response to fail closed" >&2
  exit 1
fi

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

# The pull-request routing decision must execute base-branch code. The inline
# bootstrap fallback is unavoidable while this PR introduces the scripts, so
# keep its fixed full matrices synchronized with classifier --all.
grep -Fq 'ref: ${{ github.event_name == '\''pull_request'\'' && github.event.pull_request.base.sha || github.sha }}' "$workflow"
grep -Fq 'if [[ ! -x "$classifier" || ! -x "$extractor" ]]; then' "$workflow"
grep -Fq "echo 'shield=$(value shield)'" "$workflow"
grep -Fq "echo 'state_count=$(value state_count)'" "$workflow"
grep -Fq "echo 'state_matrix=$(value state_matrix)'" "$workflow"
grep -Fq "echo 'build_count=$(value build_count)'" "$workflow"
grep -Fq "echo 'build_matrix=$(value build_matrix)'" "$workflow"

echo "typescript E2E change classifier tests passed"
