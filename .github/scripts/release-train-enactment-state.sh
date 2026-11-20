#!/usr/bin/env bash
# Report whether a release-train run for COMMIT is still publishing, or has
# already published, the stable Python packages and production docs for its
# runtime upgrade.
#
# Usage: release-train-enactment-state.sh COMMIT
# Prints:
#   train_publishes_python=true|false
#   train_deploys_docs=true|false
#
# true means a release-train job for COMMIT is still waiting or running, or
# finished its upload/deploy step successfully. watch-mainnet-release.yml uses
# this to ask for its own `mainnet` approval only when the train will not
# publish: its jobs stopped for a newer spec, gave up, or failed, or the
# runtime was deployed outside the train. Any GitHub API failure prints false,
# which falls back to asking for the approval.

set -uo pipefail

commit="${1:?commit required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
python_job="Publish stable Python once mainnet runs this upgrade"
python_step="Publish to PyPI"
docs_job="Deploy production docs once mainnet runs this upgrade"
docs_step="Deploy to Vercel (production)"

gh_api() {
  if command -v timeout >/dev/null 2>&1; then
    timeout 60 gh api "$@"
  else
    gh api "$@"
  fi
}

python=false
docs=false

# Prints true when the named job is not finished yet, or finished with the
# named step succeeding.
handles() {
  jq -r --arg job "$2" --arg step "$3" '
    [.jobs[] | select(.name == $job)
      | (.status != "completed")
        or ((first(.steps[]? | select(.name == $step) | .conclusion) // "") == "success")]
    | any' <<<"$1"
}

if runs=$(gh_api "repos/$GITHUB_REPOSITORY/actions/workflows/release-train.yml/runs?head_sha=${commit}&per_page=10" \
    | jq -r '.workflow_runs[] | select(.head_branch == "main") | .id'); then
  for run in $runs; do
    jobs=$(gh_api "repos/$GITHUB_REPOSITORY/actions/runs/${run}/jobs?filter=latest&per_page=100") || continue
    [ "$(handles "$jobs" "$python_job" "$python_step")" = true ] && python=true
    [ "$(handles "$jobs" "$docs_job" "$docs_step")" = true ] && docs=true
  done
else
  echo "could not list release-train runs for $commit; assuming the train is not publishing" >&2
fi

echo "train_publishes_python=$python"
echo "train_deploys_docs=$docs"
