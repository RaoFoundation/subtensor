#!/usr/bin/env bash
# Resolve the id of the `mainnet-upgrade-<spec>` workflow artifact, but only
# accept one whose provenance and contents match the immutable release tag and
# the runtime bytes finalized on chain.
#
# Why this matters: GitHub stores artifacts uploaded by *fork* pull requests in
# the *base* repository's artifact store. The artifact name is attacker-
# controlled, so a fork PR can plant `mainnet-upgrade-<spec>` containing an
# attacker `.commit` / wasm. watch-mainnet-release.yml downloads this artifact,
# checks out its recorded commit, and publishes it to PyPI (`bittensor`),
# crates.io, and Vercel, and force-pushes `mainnet`. Selecting the artifact by
# name / newest-first (the previous behaviour) would let that planted artifact
# ride a single routine mainnet-environment approval straight to the package
# registries. Selecting by name alone is therefore unsafe.
#
# We require the artifact's producing run to be:
#   * from this repository (not a fork)   — head_repository_id == repo id
#   * a train run on the trunk             — push/workflow_dispatch on main
#   * the release train workflow           — path == release-train.yml
#   * built from the immutable release tag — head_sha == expected commit
#   * built from a commit on main          — head_sha is an ancestor of main
#   * the exact finalized runtime          — BLAKE2b-256(wasm) == :code hash
#
# Prints the resolved artifact id to stdout on success; exits non-zero if no
# trustworthy artifact exists.
set -euo pipefail

spec="${1:?spec_version required}"
expected_commit="${2:?expected commit required}"
expected_code_hash="${3:?expected finalized code hash required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"
: "${GH_TOKEN:?GH_TOKEN required}"

[[ "$spec" =~ ^[0-9]+$ ]] || { echo "spec_version must be an integer" >&2; exit 1; }
[[ "$expected_commit" =~ ^[0-9a-f]{40}$ ]] \
  || { echo "expected commit must be a lowercase 40-byte Git SHA" >&2; exit 1; }
[[ "$expected_code_hash" =~ ^0x[0-9a-f]{64}$ ]] \
  || { echo "expected code hash must be a lowercase 32-byte hex digest" >&2; exit 1; }

repo_id=$(gh api "repos/$GITHUB_REPOSITORY" --jq '.id')
temp_dir=$(mktemp -d)
trap 'rm -rf "$temp_dir"' EXIT

# Newest-first so that, among equally-trusted artifacts, we take the latest.
artifacts=$(gh api \
  "repos/$GITHUB_REPOSITORY/actions/artifacts?name=mainnet-upgrade-${spec}&per_page=100" \
  --paginate \
  | jq -sc '[.[].artifacts[]] | sort_by(.created_at) | reverse')

count=$(jq 'length' <<<"$artifacts")
i=0
while (( i < count )); do
  art=$(jq -c ".[$i]" <<<"$artifacts")
  i=$(( i + 1 ))

  art_id=$(jq -r '.id' <<<"$art")
  expired=$(jq -r '.expired' <<<"$art")
  run_id=$(jq -r '.workflow_run.id // empty' <<<"$art")
  head_branch=$(jq -r '.workflow_run.head_branch // empty' <<<"$art")
  head_repo_id=$(jq -r '.workflow_run.head_repository_id // empty' <<<"$art")
  head_sha=$(jq -r '.workflow_run.head_sha // empty' <<<"$art")

  [[ "$expired" == "true" ]] && continue
  [[ -n "$run_id" ]] || continue
  [[ "$head_repo_id" == "$repo_id" ]] || { echo "skip artifact $art_id: not from this repo (head_repository_id=$head_repo_id)" >&2; continue; }
  [[ "$head_branch" == "main" ]] || { echo "skip artifact $art_id: head_branch=$head_branch != main" >&2; continue; }
  [[ "$head_sha" == "$expected_commit" ]] || { echo "skip artifact $art_id: head_sha=$head_sha != release tag $expected_commit" >&2; continue; }

  run=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/${run_id}")
  event=$(jq -r '.event' <<<"$run")
  wf_path=$(jq -r '.path' <<<"$run")
  run_sha=$(jq -r '.head_sha // empty' <<<"$run")
  run_repo_id=$(jq -r '.head_repository.id // empty' <<<"$run")
  [[ "$run_repo_id" == "$repo_id" ]] || { echo "skip artifact $art_id: run is not from this repo" >&2; continue; }
  [[ "$event" == "push" || "$event" == "workflow_dispatch" ]] \
    || { echo "skip artifact $art_id: unsupported run event=$event" >&2; continue; }
  [[ "$wf_path" == ".github/workflows/release-train.yml" ]] || { echo "skip artifact $art_id: run workflow=$wf_path" >&2; continue; }
  [[ "$run_sha" == "$expected_commit" ]] || { echo "skip artifact $art_id: run head_sha=$run_sha != release tag $expected_commit" >&2; continue; }

  # head_sha must be reachable from main. Use the compare API so we do not need
  # a full local clone: main is "ahead"/"identical" iff head_sha is an ancestor.
  status=$(gh api "repos/$GITHUB_REPOSITORY/compare/${head_sha}...main" --jq '.status' 2>/dev/null || echo "")
  case "$status" in
    ahead|identical) ;;
    *) echo "skip artifact $art_id: head_sha $head_sha not an ancestor of main (compare status=$status)" >&2; continue ;;
  esac

  archive="$temp_dir/artifact-${art_id}.zip"
  if ! gh api "repos/$GITHUB_REPOSITORY/actions/artifacts/${art_id}/zip" > "$archive"; then
    echo "skip artifact $art_id: could not download archive" >&2
    continue
  fi
  if ! python3 .github/scripts/verify-release-artifact.py "$archive" \
      --spec "$spec" \
      --commit "$expected_commit" \
      --code-hash "$expected_code_hash" >&2; then
    echo "skip artifact $art_id: contents do not match finalized release" >&2
    continue
  fi

  echo "$art_id"
  exit 0
done

echo "no trustworthy mainnet-upgrade-${spec} artifact matches tag commit ${expected_commit} and finalized code hash ${expected_code_hash}" >&2
exit 1
