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
runtime_and_snapshot_only=$'runtime=true\ndocs=false\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=true'
runtime_and_snapshot=$'runtime=true\ndocs=true\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=true'
docs_and_python=$'runtime=false\ndocs=true\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=false'

assert_classification README.md "$all_false"
assert_classification .github/actions/rust-setup/action.yml "$runtime_only"
assert_classification .github/actions/sccache-setup/action.yml "$runtime_only"
assert_classification .github/scripts/classify-runtime-changes.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-runtime-change-filter.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/start-accelerated-clone.sh "$runtime_only"
assert_classification .github/workflows/runtime-checks.yml "$runtime_and_snapshot"
assert_classification $'README.md\nsdk/python/example.py' "$docs_and_python"

echo "runtime change filter tests passed"
