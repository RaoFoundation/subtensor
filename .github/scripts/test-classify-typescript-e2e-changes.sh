#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
classifier="$script_dir/classify-typescript-e2e-changes.sh"
extractor="$script_dir/extract-pull-file-paths.sh"
workflow="$script_dir/../workflows/typescript-e2e.yml"
suite_registry="$repo_root/ts-tests/e2e-suite-ownership.json"
scheduled_smoke_workflow="$repo_root/.github/workflows/scheduled-smoke-tests.yml"
output=$(mktemp)
paths=$(mktemp)
mutated_config=
validation_error=
trap 'rm -f "$output" "$paths" "${mutated_config:-}" "${validation_error:-}"' EXIT

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

classify README.md
assert_value e2e false
assert_value topology_audit false

classify Cargo.lock
assert_value e2e true
assert_value evm true
assert_value staking true
assert_value shield true
assert_value topology_audit false

classify .cargo/config.toml
assert_value e2e true
assert_value evm true
assert_value shield true

classify .github/scripts/rust-setup-preflight.sh .github/scripts/install-rust-toolchain.sh
assert_value e2e true
assert_value evm true
assert_value shield true
assert_value topology_audit false

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
assert_value evm true
assert_value shield true

if printf '%s\n' '[{"filename":"README.md"}]' | "$extractor" 2 >/dev/null 2>&1; then
  echo "expected an incomplete pull-file response to fail closed" >&2
  exit 1
fi

classify ts-tests/suites/zombienet_evm/precompile.test.ts
assert_value evm true
assert_value staking false
assert_value shield false

classify ts-tests/suites/zombienet_staking/staking.test.ts ts-tests/suites/zombienet_shield/shield.test.ts
assert_value staking true
assert_value shield true
assert_value evm false

classify ts-tests/suites/zombienet_coldkey_swap/swap.test.ts ts-tests/suites/zombienet_subnets/register.test.ts
assert_value coldkey_swap true
assert_value subnets true
assert_value dev false

classify ts-tests/suites/dev/staking.test.ts
assert_value dev true
assert_value evm false

# The ownership registry is the structural completeness boundary. Every suite
# directory must be present, every PR-owned suite must route through its named
# selector, and scheduled suites stay explicitly visible instead of becoming
# an accidental omission hidden by the generic ts-tests fallback.
actual_suites=$(
  for directory in "$repo_root"/ts-tests/suites/*; do
    [[ -d "$directory" ]] && basename "$directory"
  done | sort
)
registered_suites=$(jq -r '.suites | keys[]' "$suite_registry")
if [[ "$actual_suites" != "$registered_suites" ]]; then
  echo "TypeScript E2E suite ownership is incomplete." >&2
  diff -u <(printf '%s\n' "$registered_suites") <(printf '%s\n' "$actual_suites") >&2 || true
  echo "Fix: add or remove the matching entry in ts-tests/e2e-suite-ownership.json." >&2
  echo "For PR coverage, set owner=pull_request and add its selector to the classifier and e2e-shard-plan.mjs; for scheduled smoke coverage, set owner=scheduled and selector=null." >&2
  echo "Verify: node ts-tests/scripts/validate-e2e-config.mjs && node ts-tests/scripts/test-e2e-shard-plan.mjs && .github/scripts/test-classify-typescript-e2e-changes.sh" >&2
  exit 1
fi

while IFS=$'\t' read -r suite selector; do
  classify "ts-tests/suites/$suite/future.test.ts"
  assert_value e2e true
  assert_value "$selector" true
  if ! grep -Fq "steps.filter.outputs.$selector" "$workflow"; then
    echo "PR-owned TypeScript E2E selector is not wired into the workflow: $selector" >&2
    echo "Fix: expose steps.filter.outputs.$selector to the planner in .github/workflows/typescript-e2e.yml." >&2
    echo "Verify: .github/scripts/test-classify-typescript-e2e-changes.sh" >&2
    exit 1
  fi
done < <(jq -r '.suites | to_entries[] | select(.value.owner == "pull_request") | [.key, .value.selector] | @tsv' "$suite_registry")

while IFS= read -r environment; do
  if ! grep -Fq "$environment" "$scheduled_smoke_workflow"; then
    echo "Scheduled Moonwall environment has no executing workflow: $environment" >&2
    echo "Fix: add $environment to the test matrix in .github/workflows/scheduled-smoke-tests.yml, or change its ownership in ts-tests/e2e-suite-ownership.json." >&2
    echo "Verify: .github/scripts/test-classify-typescript-e2e-changes.sh" >&2
    exit 1
  fi
done < <(jq -r '.suites[] | select(.owner == "scheduled") | .environments[]' "$suite_registry")

# Core production code is cross-suite and must retain the complete selection.
for path in \
  pallets/subtensor/src/staking/stake.rs \
  pallets/subtensor/src/subnets/registration.rs \
  pallets/subtensor/src/swap/swap_stake.rs; do
  classify "$path"
  for suite in evm staking coldkey_swap dev subnets shield; do
    assert_value "$suite" true
  done
done

classify precompiles/subtensor/src/lib.rs
assert_value evm true
assert_value dev false
assert_value shield false

classify pallets/shield/src/lib.rs
assert_value shield true
assert_value dev true
assert_value evm false

for path in \
  pallets/subtensor/src/tests/staking.rs \
  pallets/shield/src/tests.rs \
  pallets/drand/src/mock.rs \
  chain-extensions/src/tests.rs \
  chain-extensions/src/mock.rs \
  precompiles/src/mock.rs \
  node/tests/chain_spec.rs \
  runtime/tests/metadata.rs \
  support/macros/tests/tests.rs \
  support/procedural-fork/src/pallet/parse/tests/tasks.rs \
  pallets/subtensor/src/migrations/migrate_staking.rs; do
  classify "$path"
  assert_value e2e false
done

for path in \
  ts-tests/e2e-shards.json \
  ts-tests/e2e-suite-ownership.json \
  ts-tests/moonwall.config.json \
  ts-tests/configs/zombie_single_node.json \
  ts-tests/configs/zombie_extended.json \
  ts-tests/scripts/e2e-shard-plan.mjs \
  .github/workflows/typescript-e2e.yml \
  .github/actions/run-typescript-e2e/action.yml; do
  classify "$path"
  assert_value e2e true
  assert_value topology_audit true
  assert_value evm true
  assert_value shield true
done

: > "$output"
"$classifier" --all "$output"
assert_value e2e true
while IFS= read -r selector; do
  assert_value "$selector" true
done < <(jq -r '.suites[] | select(.owner == "pull_request") | .selector' "$suite_registry")
assert_value topology_audit false

# Canonical environments must discover their complete owned directory. A
# partial include allowlist would otherwise make Moonwall pass while silently
# omitting tests that are not represented in the sharded manifest.
mutated_config=$(mktemp)
validation_error=$(mktemp)
jq '(.environments[] | select(.name == "zombienet_coldkey_swap")).include = ["suites/zombienet_coldkey_swap/00-coldkey-swap.test.ts"]' \
  "$repo_root/ts-tests/moonwall.config.json" > "$mutated_config"
if E2E_CONFIG_PATH="$mutated_config" node "$repo_root/ts-tests/scripts/validate-e2e-config.mjs" >"$validation_error" 2>&1; then
  echo "expected a partial canonical include list to fail E2E ownership validation" >&2
  exit 1
fi
grep -Fq 'canonical environment "zombienet_coldkey_swap" may not define include' "$validation_error"
rm -f "$mutated_config" "$validation_error"
mutated_config=
validation_error=

# Both the E2E and Runtime classifiers treat these conventional Rust paths as
# unit-test-only. Fail this routing contract if a future pallet introduces one
# without a file- or module-level cfg(test) gate.
assert_cfg_test_module() {
  local lib="$1" module="$2"
  MODULE="$module" perl -0777 -e '
    my $module = quotemeta($ENV{MODULE});
    my $source = <>;
    my $pattern = qr/#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+$module\s*;/s;
    exit($source =~ $pattern ? 0 : 1);
  ' "$lib"
}

shopt -s nullglob
for file in "$repo_root"/pallets/*/src/mock.rs "$repo_root"/pallets/*/src/tests.rs; do
  module=${file##*/}
  module=${module%.rs}
  lib=${file%/*}/lib.rs
  if ! grep -Eq '^[[:space:]]*#!\[cfg\(test\)\][[:space:]]*$' "$file" &&
     ! assert_cfg_test_module "$lib" "$module"; then
    echo "classifier-ignored path is not cfg(test)-gated: ${file#"$repo_root"/}" >&2
    exit 1
  fi
done
for directory in "$repo_root"/pallets/*/src/tests; do
  lib=${directory%/tests}/lib.rs
  if ! assert_cfg_test_module "$lib" tests; then
    echo "classifier-ignored directory is not cfg(test)-gated: ${directory#"$repo_root"/}" >&2
    exit 1
  fi
done
for file in \
  "$repo_root"/chain-extensions/src/mock.rs \
  "$repo_root"/chain-extensions/src/tests.rs \
  "$repo_root"/precompiles/src/mock.rs; do
  module=${file##*/}
  module=${module%.rs}
  lib=${file%/*}/lib.rs
  if ! assert_cfg_test_module "$lib" "$module"; then
    echo "classifier-ignored path is not cfg(test)-gated: ${file#"$repo_root"/}" >&2
    exit 1
  fi
done
if ! assert_cfg_test_module \
  "$repo_root/support/procedural-fork/src/pallet/parse/mod.rs" tests; then
  echo "classifier-ignored path is not cfg(test)-gated: support/procedural-fork/src/pallet/parse/tests" >&2
  exit 1
fi

# Routing policy and matrix topology must be separate: trusted base code picks
# suites, while a trusted generic builder expands the proposed data manifest.
grep -Fq 'ref: ${{ github.event_name == '\''pull_request'\'' && github.event.pull_request.base.sha || github.sha }}' "$workflow"
grep -Fq '.trusted-e2e-filter/ts-tests/scripts/e2e-shard-plan.mjs' "$workflow"
grep -Fq '.proposed-e2e-plan/ts-tests/e2e-shards.json' "$workflow"
grep -Fq 'matrix: ${{ fromJSON(needs.changes.outputs.shield_matrix) }}' "$workflow"
grep -Fq 'name: Audit canonical unsharded ${{ matrix.test }}' "$workflow"
grep -Fq 'EVM_SELECTED: ${{ needs.changes.outputs.evm }}' "$workflow"
grep -Fq 'SHIELD_SELECTED: ${{ needs.changes.outputs.shield }}' "$workflow"

echo "typescript E2E change classifier tests passed"
