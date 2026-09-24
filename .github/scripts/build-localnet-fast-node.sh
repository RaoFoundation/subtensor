#!/usr/bin/env bash
# Build the patched fast-runtime node packaged into the Rust SDK E2E localnet
# image. The PR image job and the daily sccache warm job both run this script:
# sccache keys cover the rustc arguments, target, linker flags, and every
# CARGO_* variable, so any drift between the two invocations turns every PR
# compile into a cache miss.

set -euo pipefail

: "${BUILD_TRIPLE:?BUILD_TRIPLE must name the Rust target triple}"
: "${RUNTIME:?RUNTIME must name the target subdirectory}"

rustup target add "$BUILD_TRIPLE"
./scripts/localnet_patch.sh

CARGO_TARGET_DIR="target/$RUNTIME" cargo build \
  --locked \
  --profile release \
  --features "pow-faucet metadata-hash fast-runtime" \
  --package node-subtensor \
  --package node-subtensor-runtime \
  --target "$BUILD_TRIPLE"
