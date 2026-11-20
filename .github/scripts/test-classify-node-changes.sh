#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
classifier="$script_dir/classify-node-changes.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

assert_reuse() {
  local expected="$1" output="$tmp/output"
  shift
  : > "$output"
  printf '%s\n' "$@" | "$classifier" "$output" >/dev/null
  diff -u <(printf 'node_reuse=%s\n' "$expected") "$output"
}

# Runtime code is rebuilt into the wasm; unlinked files never reach the node.
assert_reuse true runtime/src/lib.rs
assert_reuse true pallets/subtensor/src/staking/add_stake.rs
assert_reuse true pallets/subtensor/src/rpc_info/metagraph.rs
assert_reuse true pallets/subtensor/src/tests/staking.rs
assert_reuse true precompiles/src/staking.rs
assert_reuse true chain-extensions/src/lib.rs
assert_reuse true primitives/safe-math/src/lib.rs
assert_reuse true docs/guides/local-development.mdx README.md ts-tests/suites/dev/a.test.ts
assert_reuse true sdk/python/bittensor/__init__.py clones/scripts/clone-mainnet.sh
assert_reuse true runtime/src/lib.rs website/apps/bittensor-website/package.json

# Crates the node compiles and calls natively.
assert_reuse false pallets/subtensor/runtime-api/src/lib.rs
assert_reuse false pallets/subtensor/rpc/src/lib.rs
assert_reuse false pallets/swap/runtime-api/src/lib.rs
assert_reuse false pallets/drand/src/lib.rs
assert_reuse false node/src/service.rs
assert_reuse false common/src/lib.rs
assert_reuse false support/macros/src/lib.rs
assert_reuse false vendor/frontier/client/rpc/src/eth/submit.rs

# Manifests, lockfiles, build scripts, and toolchain inputs change how
# everything, including the node, is compiled.
assert_reuse false Cargo.lock
assert_reuse false Cargo.toml
assert_reuse false runtime/Cargo.toml
assert_reuse false runtime/build.rs
assert_reuse false pallets/subtensor/Cargo.toml
assert_reuse false rust-toolchain.toml
assert_reuse false .cargo/config.toml

# CI, packaging, and unknown roots fail closed.
assert_reuse false .github/workflows/typescript-e2e.yml
assert_reuse false scripts/localnet.sh
assert_reuse false Dockerfile
assert_reuse false some-new-root/file.rs

# One node path anywhere in the PR decides.
assert_reuse false runtime/src/lib.rs node/src/rpc.rs pallets/subtensor/src/lib.rs

# A PR with no changed paths builds nothing new into the node.
: > "$tmp/output"
"$classifier" "$tmp/output" < /dev/null >/dev/null
diff -u <(echo node_reuse=true) "$tmp/output"

echo "classify-node-changes tests passed"
