#!/bin/bash
set -euo pipefail
R=/tmp/ms-release-483b52ceea
P="$PWD/sdk/python/.venv/bin/python"
while [ ! -f "$R/build.exit" ]; do sleep 5; done
[ "$(cat "$R/build.exit")" = 0 ]
export PYTHONPATH="$R/native:$PWD/sdk/python"
"$P" "$R/crypto/build_bridge.py" --root "$PWD" > "$R/crypto/build.log" 2>&1
"$P" "$R/crypto/crosscheck.py" > "$R/crypto/crosscheck.log" 2>&1
echo 0 > "$R/crypto/exit"
SKIP_WASM_BUILD=1 MS_FEE_QUOTE_OUTPUT="$R/fee-quotes.json" cargo test -p node-subtensor-runtime --test mldsa_reference_fees --locked > "$R/production-tests.log" 2>&1
echo 0 > "$R/production-tests.exit"
env -u SKIP_WASM_BUILD CARGO_BUILD_JOBS=2 cargo build --profile production -p node-subtensor-runtime --features try-runtime,metadata-hash --locked --offline > "$R/mainnet-build.log" 2>&1
mkdir -p "$R/mainnet"
cp target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm "$R/mainnet/runtime.wasm"
"$P" docs/design/evidence/ml-dsa-mainnet-upgrade.py --runtime "$R/mainnet/runtime.wasm" --snapshot /tmp/ms-release-validation/mainnet/mainnet.snap --source /tmp/ms-head-validation-20261017/preflight/mainnet-metadata-hash/source.json --output-dir "$R/mainnet" --try-runtime /home/johnreed/.local/bin/try-runtime > "$R/mainnet/run.log" 2>&1
echo 0 > "$R/mainnet/exit"
