#!/bin/bash
set -euo pipefail
R=/tmp/ms-release-483b52ceea
cargo check --locked --offline -p bittensor-core --no-default-features --target wasm32-unknown-unknown > "$R/wasm-core.log" 2>&1
cargo check --locked --offline -p bittensor-core-wasm --target wasm32-unknown-unknown > "$R/wasm-binding.log" 2>&1
wasm-pack build sdk/bittensor-core-wasm --target nodejs --out-dir pkg-node --no-opt --locked --offline > "$R/wasm-build.log" 2>&1
node sdk/bittensor-core-wasm/tests/smoke.mjs > "$R/wasm-smoke.log" 2>&1
echo 0 > "$R/wasm.exit"
