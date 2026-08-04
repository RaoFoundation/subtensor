#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
splitter="$script_dir/split-typescript-e2e-plan.sh"
output=$(mktemp)
trap 'rm -f "$output"' EXIT

value() {
  sed -n "s/^$1=//p" "$output"
}

run_split() {
  : > "$output"
  STATE_MATRIX=$1 BUILD_MATRIX=$2 "$splitter" "$output"
}

mixed_state='{"include":[{"test":"evm-a","binary":"fast"},{"test":"dev","binary":"release"},{"test":"staking-a","binary":"fast"}]}'
mixed_build='{"include":[{"variant":"release","flags":""},{"variant":"fast","flags":"--features fast-runtime"}]}'
run_split "$mixed_state" "$mixed_build"
[[ "$(value fast_state_count)" == 2 ]]
[[ "$(value release_state_count)" == 1 ]]
[[ "$(value fast_build)" == true ]]
[[ "$(value release_build)" == true ]]
jq -e '.include | length == 2 and all(.[]; .binary == "fast")' <<< "$(value fast_state_matrix)" >/dev/null
jq -e '.include | length == 1 and all(.[]; .binary == "release")' <<< "$(value release_state_matrix)" >/dev/null

fast_state='{"include":[{"test":"evm-a","binary":"fast"}]}'
fast_build='{"include":[{"variant":"fast","flags":"--features fast-runtime"}]}'
run_split "$fast_state" "$fast_build"
[[ "$(value fast_state_count)" == 1 ]]
[[ "$(value release_state_count)" == 0 ]]
[[ "$(value fast_build)" == true ]]
[[ "$(value release_build)" == false ]]

release_state='{"include":[{"test":"dev","binary":"release"}]}'
release_build='{"include":[{"variant":"release","flags":""}]}'
run_split "$release_state" "$release_build"
[[ "$(value fast_state_count)" == 0 ]]
[[ "$(value release_state_count)" == 1 ]]
[[ "$(value fast_build)" == false ]]
[[ "$(value release_build)" == true ]]

run_split '{"include":[]}' '{"include":[]}'
[[ "$(value fast_state_count)" == 0 ]]
[[ "$(value release_state_count)" == 0 ]]
[[ "$(value fast_build)" == false ]]
[[ "$(value release_build)" == false ]]

if STATE_MATRIX='{"include":[{"test":"future","binary":"unknown"}]}' \
   BUILD_MATRIX="$mixed_build" "$splitter" "$output" >/dev/null 2>&1; then
  echo "expected an unknown binary lane to fail closed" >&2
  exit 1
fi

if STATE_MATRIX="$fast_state" BUILD_MATRIX="$release_build" \
   "$splitter" "$output" >/dev/null 2>&1; then
  echo "expected a missing fast build to fail closed" >&2
  exit 1
fi

echo "TypeScript E2E lane split tests passed"
