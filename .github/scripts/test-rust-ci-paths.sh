#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
registry="$repo_root/.github/rust-ci-paths.txt"
validator="$script_dir/validate-rust-ci-paths.sh"
classifier="$script_dir/classify-rust-changes.sh"
rust_workflow="$repo_root/.github/workflows/check-rust.yml"
docker_workflow="$repo_root/.github/workflows/check-docker.yml"
eco_workflow="$repo_root/.github/workflows/eco-tests.yml"
validation_workflow="$repo_root/.github/workflows/validate-sccache.yml"
incomplete=$(mktemp)
output=$(mktemp)
trap 'rm -f "$incomplete" "$output"' EXIT

classify() {
  : > "$output"
  printf '%s\n' "$@" | "$classifier" "$output"
  sed -n 's/^rust=//p' "$output"
}

"$validator" "$registry"

grep -vx 'sdk/bittensor-core-wasm' "$registry" > "$incomplete"
if "$validator" "$incomplete" > "$output" 2>&1; then
  echo "expected an unowned Rust workspace package to fail validation" >&2
  exit 1
fi
grep -Fq 'Rust workspace packages have no CI path owner:' "$output"
grep -Fq '  - sdk/bittensor-core-wasm' "$output"
grep -Fq 'Fix: add the narrowest stable parent directory' "$output"
grep -Fq 'Verify: .github/scripts/test-rust-ci-paths.sh' "$output"

[[ "$(classify README.md)" == false ]]
[[ "$(classify sdk/python/README.md)" == false ]]
[[ "$(classify Cargo.toml)" == true ]]
[[ "$(classify future-workspace/member/Cargo.toml)" == true ]]
[[ "$(classify .cargo/config.toml)" == true ]]
[[ "$(classify .github/rust-ci-paths.txt)" == true ]]
[[ "$(classify .github/scripts/rust-setup-preflight.sh)" == true ]]
[[ "$(classify .github/scripts/install-rust-toolchain.sh)" == true ]]
while IFS= read -r prefix; do
  [[ "$(classify "$prefix/future.rs")" == true ]]
done < "$registry"

grep -Fq "needs.changes.outputs.rust == 'false'" "$rust_workflow"
grep -Fq 'trusted Rust classifier failed or emitted an invalid selection; running all Rust checks' "$rust_workflow"

for path in \
  '.cargo/**' \
  '.dockerignore' \
  '*.json' \
  'chainspecs/**' \
  'scripts/docker_entrypoint.sh' \
  '.github/scripts/rust-setup-preflight.sh' \
  '.github/scripts/install-rust-toolchain.sh'; do
  grep -Fq -- "- \"$path\"" "$docker_workflow" || {
    echo "Docker coverage path is missing: $path" >&2
    exit 1
  }
done

grep -Fq 'PR file listing failed; running eco-tests.' "$eco_workflow"
grep -Fq '(.previous_filename // empty)' "$eco_workflow"
grep -Fq 'install-rust-toolchain' "$eco_workflow"
grep -Fq 'name: cargo test (eco-tests)' "$eco_workflow"
grep -Fq 'RUST_RELEVANT: ${{ needs.changes.outputs.rust }}' "$eco_workflow"
grep -Fq 'true) [ "$TRUSTED" = success ] && [ "$TEST_RESULT" = success ]' "$eco_workflow"
grep -Fq '".github/scripts/install-rust-toolchain.sh"' "$validation_workflow"

echo "Rust CI path registry tests passed"
