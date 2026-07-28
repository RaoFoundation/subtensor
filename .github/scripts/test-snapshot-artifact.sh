#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
helper="$script_dir/snapshot-artifact.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Use jq's own timestamp conversion so boundary tests remain stable on both
# GNU/Linux and macOS, whose jq builds differ in local-time handling.
now=$(jq -nr '"2026-07-12T14:00:00Z" | fromdateiso8601')
repo_id=608683796

write_artifacts() {
  local artifacts="$1"
  jq -n --argjson artifacts "$artifacts" '{artifacts: $artifacts}' > "$tmp/artifacts.json"
  jq -n --argjson artifacts "$artifacts" '
    {
      workflow_runs: [
        $artifacts[]
        | {
            id: .workflow_run.id,
            path: ".github/workflows/refresh-mainnet-snapshot.yml",
            head_branch: .workflow_run.head_branch,
            head_sha: .workflow_run.head_sha,
            repository: {id: .workflow_run.repository_id},
            head_repository: {id: .workflow_run.head_repository_id},
            conclusion: (.producer_conclusion // "success")
          }
      ] | unique_by(.id)
    }
  ' > "$tmp/workflow-runs.json"
}

artifact() {
  local id="$1" name="$2" created="$3" branch="$4" repository_id="$5" expired="$6" run_id="$7"
  local head_sha
  printf -v head_sha '%040x' "$run_id"
  jq -nc \
    --argjson id "$id" --arg name "$name" --arg created "$created" --arg branch "$branch" \
    --argjson repository_id "$repository_id" --argjson expired "$expired" --argjson run_id "$run_id" \
    --arg head_sha "$head_sha" '
      {
        id: $id,
        name: $name,
        size_in_bytes: 1234,
        expired: $expired,
        digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        created_at: $created,
        workflow_run: {
          id: $run_id,
          repository_id: $repository_id,
          head_repository_id: $repository_id,
          head_branch: $branch,
          head_sha: $head_sha
        }
      }
    '
}

select_fixture() {
  local requirement="${1:-required}"
  local output="$tmp/output"
  local status=0
  : > "$output"
  ARTIFACTS_JSON_FILE="$tmp/artifacts.json" \
    WORKFLOW_RUNS_JSON_FILE="$tmp/workflow-runs.json" \
    NOW_EPOCH="$now" \
    "$helper" select try-runtime-snap-v0.10.1-mainnet main "$repo_id" \
      .github/workflows/refresh-mainnet-snapshot.yml 72 "$output" "$requirement" || status=$?
  cat "$output"
  return "$status"
}

mode_fixture() {
  local event_name="$1" labels_json="$2" manual_fresh="$3"
  local output="$tmp/mode-output"
  : > "$output"
  "$helper" mode "$event_name" "$labels_json" "$manual_fresh" "$output" >/dev/null
  cat "$output"
}

# Live state is reachable only through the established PR label or the manual
# workflow input. All other event/input combinations stay fail-closed.
grep -qx 'fresh-state=true' <<<"$(mode_fixture pull_request '["fresh-try-runtime-state"]' false)"
grep -qx 'fresh-state=true' <<<"$(mode_fixture workflow_dispatch '[]' true)"
grep -qx 'fresh-state=false' <<<"$(mode_fixture pull_request '[]' false)"
grep -qx 'fresh-state=false' <<<"$(mode_fixture workflow_dispatch '[]' false)"
grep -qx 'fresh-state=false' <<<"$(mode_fixture schedule null false)"

# Newest valid main-branch artifact wins even though the artifact payload does
# not expose (and therefore does not depend on) the producer run conclusion.
valid_old=$(artifact 11 try-runtime-snap-v0.10.1-mainnet 2026-07-11T02:00:00Z main "$repo_id" false 101)
# The artifacts API intentionally has no producer conclusion dependency. This
# synthetic marker models an artifact uploaded before a sibling matrix job
# failed; selection must still use the independently successful artifact.
valid_new=$(artifact 12 try-runtime-snap-v0.10.1-mainnet 2026-07-12T02:00:00Z main "$repo_id" false 102 \
  | jq -c '.producer_conclusion = "failure"')
wrong_branch=$(artifact 13 try-runtime-snap-v0.10.1-mainnet 2026-07-12T13:00:00Z feature "$repo_id" false 103)
wrong_repo=$(artifact 14 try-runtime-snap-v0.10.1-mainnet 2026-07-12T13:30:00Z main 999 false 104)
expired=$(artifact 15 try-runtime-snap-v0.10.1-mainnet 2026-07-12T13:45:00Z main "$repo_id" true 105)
wrong_workflow=$(artifact 16 try-runtime-snap-v0.10.1-mainnet 2026-07-12T13:50:00Z main "$repo_id" false 106)
wrong_sha=$(artifact 17 try-runtime-snap-v0.10.1-mainnet 2026-07-12T13:55:00Z main "$repo_id" false 107)
write_artifacts "$(jq -nc --argjson a "$valid_old" --argjson b "$valid_new" --argjson c "$wrong_branch" --argjson d "$wrong_repo" --argjson e "$expired" --argjson f "$wrong_workflow" --argjson g "$wrong_sha" '[ $a, $b, $c, $d, $e, $f, $g ]')"
jq '(.workflow_runs[] | select(.id == 106) | .path) = ".github/workflows/untrusted-producer.yml"' \
  "$tmp/workflow-runs.json" > "$tmp/workflow-runs-updated.json"
mv "$tmp/workflow-runs-updated.json" "$tmp/workflow-runs.json"
jq '(.workflow_runs[] | select(.id == 107) | .head_sha) = "ffffffffffffffffffffffffffffffffffffffff"' \
  "$tmp/workflow-runs.json" > "$tmp/workflow-runs-updated.json"
mv "$tmp/workflow-runs-updated.json" "$tmp/workflow-runs.json"
selected=$(select_fixture)
grep -qx 'artifact-id=12' <<<"$selected"
grep -qx 'run-id=102' <<<"$selected"
grep -qx 'producer-sha=0000000000000000000000000000000000000066' <<<"$selected"
grep -qx 'artifact-size-bytes=1234' <<<"$selected"
grep -qx 'artifact-digest=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' <<<"$selected"

# Exactly 72 hours is accepted, one second older fails, and optional lookup
# reports a miss. An artifact older than 36 hours remains usable with a warning.
at_boundary=$(artifact 19 try-runtime-snap-v0.10.1-mainnet 2026-07-09T14:00:00Z main "$repo_id" false 199)
write_artifacts "[$at_boundary]"
boundary=$(select_fixture)
grep -qx 'artifact-id=19' <<<"$boundary"

warning_age=$(artifact 18 try-runtime-snap-v0.10.1-mainnet 2026-07-11T01:59:59Z main "$repo_id" false 198)
write_artifacts "[$warning_age]"
warning_output="$tmp/warning-output"
: > "$warning_output"
warning_log=$(ARTIFACTS_JSON_FILE="$tmp/artifacts.json" \
  WORKFLOW_RUNS_JSON_FILE="$tmp/workflow-runs.json" NOW_EPOCH="$now" \
  "$helper" select try-runtime-snap-v0.10.1-mainnet main "$repo_id" \
    .github/workflows/refresh-mainnet-snapshot.yml 72 "$warning_output" required)
grep -q '^::warning::' <<<"$warning_log"

too_old=$(artifact 20 try-runtime-snap-v0.10.1-mainnet 2026-07-09T13:59:59Z main "$repo_id" false 200)
write_artifacts "[$too_old]"
if select_fixture required >/dev/null 2>&1; then
  echo "expected required stale lookup to fail" >&2
  exit 1
fi
optional=$(select_fixture optional)
grep -qx 'found=false' <<<"$optional"

# Validate a complete manifest, then prove identity, size, and checksum failures are rejected.
printf 'snapshot payload' > "$tmp/mainnet.snap"
size=$(stat -c '%s' "$tmp/mainnet.snap" 2>/dev/null || stat -f '%z' "$tmp/mainnet.snap")
sha=$(sha256sum "$tmp/mainnet.snap" | awk '{print $1}')
jq -n \
  --arg sha "$sha" --argjson size "$size" '
    {
      schema_version: 1,
      kind: "try-runtime-state",
      network: "mainnet",
      genesis_hash: "0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03",
      finalized_block_hash: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      finalized_block_number: 123,
      source_spec_name: "node-subtensor",
      source_spec_version: 428,
      try_runtime_cli_version: "0.10.1",
      created_at: "2026-07-12T02:00:00Z",
      producer_sha: "647ca2b0493ed5c74399b73f2595643ba785c1b8",
      snapshot_file: "mainnet.snap",
      snapshot_size_bytes: $size,
      snapshot_sha256: $sha
    }
  ' > "$tmp/mainnet.manifest.json"

"$helper" validate "$tmp/mainnet.manifest.json" "$tmp/mainnet.snap" mainnet \
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 \
  0.10.1 647ca2b0493ed5c74399b73f2595643ba785c1b8 >/dev/null

if "$helper" validate "$tmp/mainnet.manifest.json" "$tmp/mainnet.snap" mainnet \
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 \
  0.10.1 ffffffffffffffffffffffffffffffffffffffff >/dev/null 2>&1; then
  echo "expected producer SHA mismatch to fail" >&2
  exit 1
fi

if "$helper" validate "$tmp/mainnet.manifest.json" "$tmp/mainnet.snap" testnet \
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 \
  0.10.1 647ca2b0493ed5c74399b73f2595643ba785c1b8 >/dev/null 2>&1; then
  echo "expected network mismatch to fail" >&2
  exit 1
fi

printf 'corrupt' >> "$tmp/mainnet.snap"
if "$helper" validate "$tmp/mainnet.manifest.json" "$tmp/mainnet.snap" mainnet \
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 \
  0.10.1 647ca2b0493ed5c74399b73f2595643ba785c1b8 >/dev/null 2>&1; then
  echo "expected checksum/size mismatch to fail" >&2
  exit 1
fi

echo "snapshot artifact helper tests passed"
