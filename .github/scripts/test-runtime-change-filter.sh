#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
classifier="$script_dir/classify-runtime-changes.sh"
runtime_workflow="$script_dir/../workflows/runtime-checks.yml"
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
runtime_sdk_python=$'runtime=true\ndocs=false\npython_sdk=true\nsdk_drift=true\nsnapshot_ci=false'
runtime_and_docs=$'runtime=true\ndocs=true\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=false'
runtime_and_snapshot_only=$'runtime=true\ndocs=false\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=true'
runtime_and_snapshot=$'runtime=true\ndocs=true\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=true'
docs_and_python=$'runtime=false\ndocs=true\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=false'
python_only=$'runtime=false\ndocs=false\npython_sdk=true\nsdk_drift=false\nsnapshot_ci=false'
docs_only=$'runtime=false\ndocs=true\npython_sdk=false\nsdk_drift=false\nsnapshot_ci=false'

assert_classification README.md "$all_false"
assert_classification pallets/subtensor/src/tests/staking.rs "$all_false"
assert_classification pallets/shield/src/tests.rs "$all_false"
assert_classification pallets/drand/src/mock.rs "$all_false"
assert_classification chain-extensions/src/tests.rs "$all_false"
assert_classification chain-extensions/src/mock.rs "$all_false"
assert_classification precompiles/src/mock.rs "$all_false"
assert_classification node/tests/chain_spec.rs "$all_false"
assert_classification runtime/tests/metadata.rs "$all_false"
assert_classification support/macros/tests/tests.rs "$all_false"
assert_classification support/procedural-fork/src/pallet/parse/tests/tasks.rs "$all_false"
assert_classification .github/actions/rust-setup/action.yml "$runtime_only"
assert_classification .github/actions/sccache-setup/action.yml "$runtime_only"
assert_classification .github/scripts/rust-setup-preflight.sh "$runtime_only"
assert_classification .github/scripts/install-rust-toolchain.sh "$runtime_only"
assert_classification .github/scripts/sccache-report.sh "$runtime_only"
assert_classification .github/scripts/classify-runtime-changes.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-runtime-change-filter.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/select-shared-release-artifact.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-select-shared-release-artifact.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/prewarm-exact-runtime.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-prewarm-exact-runtime.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/benchmark-sccache-paired.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/benchmark-artifact-cache.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/publish-current-run-artifact-mirror.sh "$runtime_and_snapshot_only"
assert_classification clones/scripts/start-local-clone-and-wait.sh "$runtime_only"
assert_classification clones/scripts/run-clone-regression-phase.sh "$runtime_and_snapshot_only"
assert_classification clones/scripts/run-clone-epoch-soak.sh "$runtime_and_snapshot_only"
assert_classification clones/scripts/run-clone-block-monitor.sh "$runtime_and_snapshot_only"
assert_classification clones/scripts/clone-process-supervision.sh "$runtime_and_snapshot_only"
assert_classification clones/js-tests/lib/clone-performance.ts "$runtime_and_snapshot_only"
assert_classification clones/js-tests/lib/clone-invariants.ts "$runtime_and_snapshot_only"
assert_classification clones/js-tests/lib/clone-readiness.ts "$runtime_and_snapshot_only"
assert_classification clones/js-tests/scripts/wait-clone-readiness.ts "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-clone-regression-phase.sh "$runtime_and_snapshot_only"
assert_classification .github/scripts/test-clone-epoch-soak-workflow.sh "$runtime_and_snapshot_only"
assert_classification .github/workflows/refresh-mainnet-snapshot.yml "$runtime_and_snapshot_only"
assert_classification .github/workflows/clone-epoch-soak.yml "$runtime_and_snapshot_only"
assert_classification .github/workflows/runtime-checks.yml "$runtime_and_snapshot"
assert_classification website/apps/bittensor-website/scripts/generate-metadata.mjs "$runtime_and_docs"
assert_classification docs/concepts/client.mdx "$docs_only"
assert_classification sdk/bittensor-core/src/lib.rs "$python_only"
assert_classification .github/scripts/prepare-sdk-dist.py "$python_only"
assert_classification .github/scripts/test_prepare_sdk_dist.py "$python_only"
assert_classification Cargo.lock "$runtime_sdk_python"
assert_classification .cargo/config.toml "$runtime_and_sdk"
assert_classification future-runtime-crate/src/lib.rs "$runtime_and_sdk"
assert_classification rust-toolchain.toml "$runtime_and_sdk"
assert_classification $'README.md\nsdk/python/example.py' "$docs_and_python"
assert_classification $'README.md\nnode/src/renamed-service.rs' "$runtime_and_sdk"

# The snapshot-backed clone split must remain fail-closed: both independent
# phases run when a trusted artifact exists, while planner/fresh-state fallback
# preserves the complete sequential suite. These are static workflow contract
# checks; snapshot selection itself is exercised by test-snapshot-artifact.sh.
grep -Fq 'matrix={"phase":["pristine","remaining"]}' "$runtime_workflow"
grep -Fq 'matrix={"phase":["combined"]}' "$runtime_workflow"
grep -Fq "needs.clone-plan.outputs.matrix || '{\"phase\":[\"combined\"]}'" "$runtime_workflow"
grep -Fq './clones/scripts/run-clone-regression-phase.sh "${{ matrix.phase }}"' "$runtime_workflow"
clone_upgrade_job=$(sed -n '/^  clone-upgrade:/,/^  clone-upgrade-gate:/p' "$runtime_workflow")
grep -Fq 'runs-on: [self-hosted, fireactions-turbo-8]' <<< "$clone_upgrade_job"
if grep -Fq 'fireactions-validatorbench' <<< "$clone_upgrade_job"; then
  echo "clone-upgrade phases, including remaining, must use fireactions-turbo-8" >&2
  exit 1
fi
grep -Fq 'RUN_SDK_DRIFT: ${{ github.event_name != '\''pull_request'\'' || needs.changes.outputs.sdk_drift == '\''true'\'' }}' "$runtime_workflow"
grep -Fq 'clones/js-tests/temp/clone-readiness-*.json' "$runtime_workflow"
grep -Fq 'uses: actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 # v4' "$runtime_workflow"
grep -Fq 'uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4' <<< "$clone_upgrade_job"
grep -Fq 'artifact_id: ${{ steps.plan.outputs.artifact_id }}' "$runtime_workflow"
grep -Fq 'ARTIFACT_ID: ${{ needs.clone-plan.outputs.artifact_id }}' "$runtime_workflow"
grep -Fq 'gh api "repos/$GITHUB_REPOSITORY/actions/artifacts/$ARTIFACT_ID"' "$runtime_workflow"
grep -Fq '"$(jq -er '\''.digest'\'' "$metadata")"' "$runtime_workflow"
grep -Fq '"$(jq -er '\''.size_in_bytes'\'' "$metadata")"' "$runtime_workflow"

# A transient Files API outage must fail closed by selecting the full matrix,
# not fail the classifier before required aggregate contexts can be reported.
grep -Fq 'if ! pages=$(gh api "repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER/files" --paginate --slurp); then' "$runtime_workflow"
grep -Fq '::warning::PR file listing failed; enabling every check.' "$runtime_workflow"
grep -Fq 'Runtime classifier failed or emitted invalid outputs; enabling every check.' "$runtime_workflow"
grep -Fq 'RUNTIME_RELEVANT: ${{ needs.changes.outputs.runtime }}' "$runtime_workflow"
grep -Fq '[ "$RUNTIME_RELEVANT" = "false" ]' "$runtime_workflow"

echo "runtime change filter tests passed"
