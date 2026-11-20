#!/usr/bin/env bash
# Build the patched fast-runtime node packaged into the Rust SDK E2E localnet
# image. The PR image job and the daily sccache warm job both run this script:
# sccache keys cover the rustc arguments, target, linker flags, and every
# CARGO_* variable, so any drift between the two invocations turns every PR
# compile into a cache miss.
#
# BASE_NODE, when set, names the base commit's localnet node; the build then
# stops after the runtime wasm and keeps that node if it verifies
# (build-node-reusing-base.py). The cargo invocation is identical either way.

set -euo pipefail

: "${BUILD_TRIPLE:?BUILD_TRIPLE must name the Rust target triple}"
: "${RUNTIME:?RUNTIME must name the target subdirectory}"

rustup target add "$BUILD_TRIPLE"
./scripts/localnet_patch.sh

reuse=()
[[ -z "${BASE_NODE:-}" ]] || reuse=(--base-node "$BASE_NODE")

CARGO_TARGET_DIR="target/$RUNTIME" .github/scripts/build-node-reusing-base.py \
  --node "target/$RUNTIME/$BUILD_TRIPLE/release/node-subtensor" "${reuse[@]}" -- \
  cargo build \
  --locked \
  --profile release \
  --features "pow-faucet metadata-hash fast-runtime" \
  --package node-subtensor \
  --package node-subtensor-runtime \
  --target "$BUILD_TRIPLE"
