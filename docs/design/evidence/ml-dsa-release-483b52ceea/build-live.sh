#!/bin/bash
set -euo pipefail
R=/tmp/ms-release-483b52ceea
while [ ! -f "$R/preflight-rerun.exit" ]; do sleep 5; done
[ "$(cat "$R/preflight-rerun.exit")" = 0 ]
env -u SKIP_WASM_BUILD cargo build --release -p node-subtensor --features metadata-hash --locked > "$R/build-metadata.log" 2>&1
cp target/release/node-subtensor "$R/node-subtensor"
cp target/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm "$R/runtime.wasm"
sha256sum "$R/node-subtensor" "$R/runtime.wasm" > "$R/artifacts.sha256"
cargo build --release -p bittensor-core-py --locked > "$R/native-build.log" 2>&1
mkdir -p "$R/native"
cp target/release/libbittensor_core.so "$R/native/bittensor_core.so"
echo 0 > "$R/build.exit"
exec sdk/python/.venv/bin/python "$R/run-live.py"
