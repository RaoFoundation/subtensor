#!/usr/bin/env bash
set -euo pipefail

: "${GH_TOKEN:?GH_TOKEN is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${GITHUB_RUN_ID:?GITHUB_RUN_ID is required}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"

current_spec=${1:?usage: cancel-superseded-release-watchers.sh <current-spec>}
[[ "$current_spec" =~ ^[0-9]+$ ]] \
  || { echo "current spec must be an integer, got: $current_spec"; exit 2; }

workflow=watch-mainnet-release.yml
release_slot_available=true
run_ids=$(
  for status in waiting in_progress queued pending; do
    gh api --paginate \
      "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow/runs?status=$status&per_page=100" \
      --jq '.workflow_runs[].id'
  done | sort -u
)

while read -r run_id; do
  [ -n "$run_id" ] || continue
  [ "$run_id" != "$GITHUB_RUN_ID" ] || continue

  jobs=$(gh api --paginate \
    "repos/$GITHUB_REPOSITORY/actions/runs/$run_id/jobs?filter=all&per_page=100" \
    --jq '.jobs[] | [.name, .status] | @tsv')
  run_spec=$(sed -nE 's/^Cut GitHub release v([0-9]+)[[:space:]].*$/\1/p' \
    <<<"$jobs" | sort -n | tail -n 1)
  [ -n "$run_spec" ] || continue

  if [ "$run_spec" -eq "$current_spec" ] && \
      grep -Eq $'\t(in_progress|waiting|queued|pending|requested)$' <<<"$jobs"; then
    echo "Watcher run $run_id already owns the runtime v$current_spec release slot"
    release_slot_available=false
    continue
  fi

  # Cancel only while protected publication work is still waiting. Once any
  # job has begun executing, leave the run alone rather than interrupting a
  # package upload or branch update halfway through.
  if grep -Eq $'\t(in_progress)$' <<<"$jobs"; then
    continue
  fi
  if ! grep -Eq \
      $'^(Cut GitHub release v[0-9]+|Publish Python packages to PyPI|Publish Rust crates to crates.io|Deploy production website and docs)\t(waiting|queued|pending|requested)$' \
      <<<"$jobs"; then
    continue
  fi

  if [ "$run_spec" -lt "$current_spec" ]; then
    echo "Canceling watcher run $run_id for superseded runtime v$run_spec"
    # A run can finish between the list and cancel requests. Treat that race
    # as cleanup already accomplished rather than blocking the current release.
    gh api --method POST \
      "repos/$GITHUB_REPOSITORY/actions/runs/$run_id/cancel" \
      >/dev/null || echo "Watcher run $run_id was no longer cancelable"
  fi
done <<<"$run_ids"

echo "release_slot_available=$release_slot_available" >> "$GITHUB_OUTPUT"
