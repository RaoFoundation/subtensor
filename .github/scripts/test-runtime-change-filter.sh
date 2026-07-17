#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
classifier="$script_dir/classify-runtime-changes.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

assert_classification() {
  local path="$1" expected="$2" output="$tmp/output"
  : > "$output"
  printf '%s\n' "$path" | "$classifier" "$output" >/dev/null
  diff -u <(printf '%s\n' "$expected") "$output"
}

all_false=$'runtime=false\ndocs=false\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=false'
runtime_only=$'runtime=true\ndocs=false\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=false'
runtime_and_sdk=$'runtime=true\ndocs=false\npython_sdk=false\nsdk_drift=true\nsnapshot_ci=false'
runtime_and_docs=$'runtime=true\ndocs=true\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=false'
runtime_and_snapshot_only=$'runtime=true\ndocs=false\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=true'
runtime_and_snapshot=$'runtime=true\ndocs=true\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=true'
docs_and_python=$'runtime=false\ndocs=true\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=false'
python_only=$'runtime=false\ndocs=false\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=false'

assert_classification README.md "$all_false"
assert_classification .github/actions/rust-setup/action.yml "$runtime_only"
assert_classification .github/actions/sccache-setup/action.yml "$runtime_only"
assert_classification .github/scripts/classify-runtime-changes.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-runtime-change-filter.sh "$runtime_and_snapshot_only"
assert_classification clones/scripts/start-local-clone-and-wait.sh "$runtime_only"
assert_classification .github/workflows/refresh-mainnet-snapshot.yml "$runtime_and_snapshot_only"
assert_classification .github/workflows/runtime-checks.yml "$runtime_and_snapshot"
assert_classification website/apps/bittensor-website/scripts/generate-metadata.mjs "$runtime_and_docs"
assert_classification sdk/bittensor-core/src/lib.rs "$python_only"
assert_classification .github/scripts/prepare-sdk-dist.py "$python_only"
assert_classification .github/scripts/test_prepare_sdk_dist.py "$python_only"
assert_classification Cargo.lock "$python_only"
assert_classification rust-toolchain.toml "$runtime_and_sdk"
assert_classification $'README.md\nsdk/python/example.py' "$docs_and_python"
assert_classification $'README.md\nnode/src/renamed-service.rs' "$runtime_and_sdk"

echo "runtime change filter tests passed"
