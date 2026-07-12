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
  jq -n --argjson artifacts "$1" '{artifacts: $artifacts}' > "$tmp/artifacts.json"
}

artifact() {
  local id="$1" name="$2" created="$3" branch="$4" repository_id="$5" expired="$6" run_id="$7"
  jq -nc \
    --argjson id "$id" --arg name "$name" --arg created "$created" --arg branch "$branch" \
    --argjson repository_id "$repository_id" --argjson expired "$expired" --argjson run_id "$run_id" '
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
          head_branch: $branch
        }
      }
    '
}

select_fixture() {
  local requirement="${1:-required}"
  local output="$tmp/output"
  local status=0
  : > "$output"
  ARTIFACTS_JSON_FILE="$tmp/artifacts.json" NOW_EPOCH="$now" \
    "$helper" select try-runtime-snap-v0.10.1-mainnet main "$repo_id" 72 "$output" "$requirement" || status=$?
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
write_artifacts "$(jq -nc --argjson a "$valid_old" --argjson b "$valid_new" --argjson c "$wrong_branch" --argjson d "$wrong_repo" --argjson e "$expired" '[ $a, $b, $c, $d, $e ]')"
selected=$(select_fixture)
grep -qx 'artifact-id=12' <<<"$selected"
grep -qx 'run-id=102' <<<"$selected"

# The selected immutable artifact ID is downloaded as its archive, verified
# against the API digest, and only then extracted.
mkdir -p "$tmp/archive-input"
printf 'artifact payload' > "$tmp/archive-input/payload.txt"
(cd "$tmp/archive-input" && zip -q "$tmp/artifact.zip" payload.txt)
archive_digest="sha256:$(sha256sum "$tmp/artifact.zip" | awk '{print $1}')"
ARTIFACT_ZIP_FILE="$tmp/artifact.zip" \
  "$helper" download 12 "$archive_digest" "$tmp/downloaded" >/dev/null
grep -qx 'artifact payload' "$tmp/downloaded/payload.txt"
if ARTIFACT_ZIP_FILE="$tmp/artifact.zip" \
  "$helper" download 12 sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \
    "$tmp/bad-download" >/dev/null 2>&1; then
  echo "expected artifact archive checksum mismatch to fail" >&2
  exit 1
fi

# Exercise the real ranged-download branch with deterministic fake GitHub and
# byte-range clients. One worker fails once, forcing a fresh signed URL; exact
# part sizing, ordered assembly, archive digest, and extraction must still pass.
fake_bin="$tmp/fake-bin"
mkdir -p "$fake_bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'wc -c < "$MOCK_ARTIFACT_ZIP" | tr -d " "' \
  > "$fake_bin/gh"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'headers= output= range=' \
  'while (($#)); do' \
  '  case "$1" in' \
  '    --dump-header) headers="$2"; shift 2 ;;' \
  '    --output) output="$2"; shift 2 ;;' \
  '    --range) range="$2"; shift 2 ;;' \
  '    --connect-timeout|--max-time|--header|--write-out) shift 2 ;;' \
  '    *) shift ;;' \
  '  esac' \
  'done' \
  'if [[ -n "$headers" ]]; then' \
  '  printf "HTTP/1.1 302 Found\r\nLocation: https://artifact.invalid/download\r\n\r\n" > "$headers"' \
  '  printf "x\n" >> "$MOCK_REDIRECT_LOG"' \
  '  printf 302' \
  '  exit 0' \
  'fi' \
  'if [[ -n "${MOCK_FAIL_ONCE_DIR:-}" ]] && mkdir "$MOCK_FAIL_ONCE_DIR" 2>/dev/null; then' \
  '  exit 22' \
  'fi' \
  'start=${range%-*}; end=${range#*-}; count=$((end - start + 1))' \
  'if [[ "${MOCK_IGNORE_RANGE:-false}" == true ]]; then' \
  '  cp "$MOCK_ARTIFACT_ZIP" "$output"' \
  'else' \
  '  dd if="$MOCK_ARTIFACT_ZIP" of="$output" bs=1 skip="$start" count="$count" status=none' \
  'fi' \
  > "$fake_bin/curl"
chmod +x "$fake_bin/gh" "$fake_bin/curl"

mock_digest="sha256:$(sha256sum "$tmp/artifact.zip" | awk '{print $1}')"
redirect_log="$tmp/redirect.log"
PATH="$fake_bin:$PATH" \
  GH_TOKEN=test-token \
  GITHUB_REPOSITORY=RaoFoundation/subtensor \
  MOCK_ARTIFACT_ZIP="$tmp/artifact.zip" \
  MOCK_REDIRECT_LOG="$redirect_log" \
  MOCK_FAIL_ONCE_DIR="$tmp/fail-once" \
  ARTIFACT_DOWNLOAD_CONCURRENCY=4 \
  ARTIFACT_RETRY_DELAY_SECONDS=0 \
  "$helper" download 12 "$mock_digest" "$tmp/ranged-download" >/dev/null
grep -qx 'artifact payload' "$tmp/ranged-download/payload.txt"
[[ "$(wc -l < "$redirect_log" | tr -d ' ')" == 2 ]]

# A server that ignores Range must fail closed after bounded retries and clean
# every temporary part/archive even though the helper exits from the function.
range_tmp="$tmp/range-tmp"
mkdir -p "$range_tmp"
if PATH="$fake_bin:$PATH" \
  TMPDIR="$range_tmp" \
  GH_TOKEN=test-token \
  GITHUB_REPOSITORY=RaoFoundation/subtensor \
  MOCK_ARTIFACT_ZIP="$tmp/artifact.zip" \
  MOCK_REDIRECT_LOG="$tmp/ignored-range-redirect.log" \
  MOCK_IGNORE_RANGE=true \
  ARTIFACT_DOWNLOAD_CONCURRENCY=4 \
  ARTIFACT_RETRY_DELAY_SECONDS=0 \
  "$helper" download 12 "$mock_digest" "$tmp/ignored-range" >/dev/null 2>&1; then
  echo "expected ignored range responses to fail" >&2
  exit 1
fi
[[ -z "$(find "$range_tmp" -mindepth 1 -print -quit)" ]]

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
warning_log=$(ARTIFACTS_JSON_FILE="$tmp/artifacts.json" NOW_EPOCH="$now" \
  "$helper" select try-runtime-snap-v0.10.1-mainnet main "$repo_id" 72 "$warning_output" required)
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
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 0.10.1 >/dev/null

if "$helper" validate "$tmp/mainnet.manifest.json" "$tmp/mainnet.snap" testnet \
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 0.10.1 >/dev/null 2>&1; then
  echo "expected network mismatch to fail" >&2
  exit 1
fi

printf 'corrupt' >> "$tmp/mainnet.snap"
if "$helper" validate "$tmp/mainnet.manifest.json" "$tmp/mainnet.snap" mainnet \
  0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03 0.10.1 >/dev/null 2>&1; then
  echo "expected checksum/size mismatch to fail" >&2
  exit 1
fi

echo "snapshot artifact helper tests passed"
