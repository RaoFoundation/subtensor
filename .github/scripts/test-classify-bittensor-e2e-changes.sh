#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
classifier="$script_dir/classify-bittensor-e2e-changes.sh"
builder="$script_dir/build-bittensor-e2e-matrix.py"
extractor="$script_dir/extract-pull-file-paths.sh"
workflow="$script_dir/../workflows/check-bittensor-e2e-tests.yml"
publisher="$script_dir/publish-localnet-manifest.sh"
output=$(mktemp)
paths=$(mktemp)
trap 'rm -f "$output" "$paths"' EXIT

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

classify README.md sdk/python/bittensor/core.py
assert_value e2e false
assert_value build_image false

classify sdk/bittensor-core-py/src/lib.rs sdk/bittensor-core-wasm/src/lib.rs
assert_value e2e false
assert_value build_image false

classify sdk/bittensor-core/src/runtime/mod.rs
assert_value e2e true
assert_value build_image false

classify sdk/bittensor-core/tests/e2e.rs
assert_value e2e true
assert_value build_image false

classify sdk/bittensor-core/tests/Dockerfile.localnet-fast
assert_value e2e true
assert_value build_image true

classify pallets/subtensor/src/staking/stake.rs
assert_value e2e true
assert_value build_image true

classify pallets/subtensor/src/tests/staking.rs pallets/shield/src/mock.rs
assert_value e2e false
assert_value build_image false

classify pallets/swap/src/pallet/tests.rs node/tests/chain_spec.rs runtime/tests/metadata.rs
assert_value e2e false
assert_value build_image false

classify pallets/subtensor/src/migrations/migrate_staking.rs
assert_value e2e false
assert_value build_image false

classify pallets/shield/src/migrations/migrate_clear_v1_storage.rs
assert_value e2e false
assert_value build_image false

classify pallets/shield/src/benchmarking.rs node/src/benchmarking.rs support/weight-tools/src/weight_compare.rs
assert_value e2e false
assert_value build_image false

classify pallets/subtensor/README.md precompiles/src/solidity/staking.sol
assert_value e2e false
assert_value build_image false

classify Cargo.lock
assert_value e2e true
assert_value build_image true

classify .cargo/config.toml
assert_value e2e true
assert_value build_image true

classify scripts/localnet.sh snapshot.json .github/actions/rust-setup/action.yml .github/workflows/docker-localnet.yml
assert_value e2e true
assert_value build_image true

classify .github/scripts/rust-setup-preflight.sh .github/scripts/install-rust-toolchain.sh
assert_value e2e true
assert_value build_image true

classify sdk/new-chain-client/src/lib.rs
assert_value e2e true
assert_value build_image true

# Preserve the previous path on renames so production code cannot be moved out
# of a covered tree and silently avoid the suite.
printf '%s\n' \
  '[{"filename":"docs/moved.rs","previous_filename":"runtime/src/lib.rs"}]' \
  | "$extractor" 1 > "$paths"
: > "$output"
"$classifier" "$output" < "$paths"
assert_value e2e true
assert_value build_image true

: > "$output"
"$classifier" --all "$output"
assert_value e2e true
assert_value build_image true

# The real manifest must produce 32 non-empty shards containing every test
# exactly once. With 109 tests, balanced round-robin shards contain 3 or 4.
: > "$output"
"$builder" "$repo_root/sdk/bittensor-core/tests/e2e-manifest.json" "$output"
python3 - "$output" <<'PY'
import json, sys

values = dict(line.rstrip("\n").split("=", 1) for line in open(sys.argv[1], encoding="utf-8"))
matrix = json.loads(values["test_matrix"])["include"]
tests = [test for shard in matrix for test in shard["tests"]]
assert values["test_count"] == "109"
assert values["shard_count"] == "32"
assert len(matrix) == 32
assert len(tests) == len(set(tests)) == 109
assert {len(shard["tests"]) for shard in matrix} == {3, 4}
assert [shard["shard"] for shard in matrix] == list(range(1, 33))
PY

# Routing decisions execute trusted base-branch code and fail closed while the
# classifier is first being introduced. Required coverage stays at 109 tests.
grep -Fq "ref: \${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.sha }}" "$workflow"
grep -Fq 'trusted Rust SDK E2E classifier unavailable; running the full suite' "$workflow"
grep -Fq 'trusted Rust SDK E2E classifier failed or emitted invalid outputs; running the full suite' "$workflow"
grep -Fq 'max-parallel: 32' "$workflow"
if grep -Fq 'needs.plan.outputs.test_count == '\''112'\''' "$workflow"; then
  echo "the Rust SDK E2E consumer must not duplicate the builder's exact test-count contract" >&2
  exit 1
fi
grep -Fq 'TESTS_JSON: ${{ toJSON(matrix.tests) }}' "$workflow"
grep -Fq 'IMAGE_REF: ${{ needs.build-localnet-image.outputs.image_ref || needs.plan.outputs.base_image_ref }}' "$workflow"
grep -Fq 'base_image_tag="$LOCALNET_IMAGE_REPOSITORY:sha-$BASE_SHA"' "$workflow"
grep -Fq -- '--tag "$IMAGE:sha-$SHA"' "$publisher"
if sed -n '/^  pull_request:/,/^  workflow_dispatch:/p' "$workflow" \
    | grep -Eq '^[[:space:]]+paths:'; then
  echo "Rust SDK E2E must always reach its fail-closed classifier" >&2
  exit 1
fi

echo "Rust SDK E2E classifier and shard tests passed"
