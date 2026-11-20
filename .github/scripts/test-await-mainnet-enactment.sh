#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
waiter="$script_dir/await-mainnet-enactment.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

printf 'not really wasm' > "$tmp/subtensor.wasm"
code_hash="0x$(python3 -c '
import hashlib, sys
print(hashlib.blake2b(open(sys.argv[1], "rb").read(), digest_size=32).hexdigest())
' "$tmp/subtensor.wasm")"
other_hash="0x$(printf '%064d' 7)"
release_sha=$(printf 'a%.0s' {1..40})
other_sha=$(printf 'b%.0s' {1..40})

cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
endpoint="${!#}"
case "$endpoint" in
  */git/ref/tags/*)
    if [ -z "$MOCK_TAG_SHA" ]; then
      echo 'gh: Not Found (HTTP 404)' >&2
      exit 1
    fi
    jq -n --arg sha "$MOCK_TAG_SHA" '{object: {type: "commit", sha: $sha}}'
    ;;
  */actions/runs/*/jobs*)
    jq -n --arg status "$MOCK_PROPOSE_STATUS" --arg conclusion "$MOCK_PROPOSE_CONCLUSION" \
      '{jobs: [
        {name: "Publish stable Python packages once mainnet runs this upgrade", status: "in_progress", conclusion: null},
        {name: "Propose mainnet upgrade (multisig)", status: $status,
         conclusion: (if $conclusion == "" then null else $conclusion end)}
      ]}'
    ;;
  */contents/runtime/src/lib.rs*)
    printf '    spec_version: %s,\n' "$MOCK_MAIN_SPEC"
    ;;
  *) echo "unexpected endpoint: $endpoint" >&2; exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/gh"

# Each probe call consumes the next "spec hash" line; "fail" simulates an RPC
# error and the last line repeats once the sequence runs out.
cat > "$tmp/probe" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
calls=0
[[ ! -f "$MOCK_PROBE_CALLS" ]] || calls=$(cat "$MOCK_PROBE_CALLS")
calls=$((calls + 1))
echo "$calls" > "$MOCK_PROBE_CALLS"
line=$(sed -n "${calls}p" "$MOCK_PROBE_SEQUENCE")
[ -n "$line" ] || line=$(tail -n 1 "$MOCK_PROBE_SEQUENCE")
[ "$line" != fail ] || exit 1
read -r spec hash <<<"$line"
printf 'finalized_head=0x%064d\nspec_version=%s\ncode_hash=%s\n' 1 "$spec" "$hash"
EOF
chmod +x "$tmp/probe"

export PATH="$tmp/bin:$PATH"
export GH_TOKEN=test-token
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export GITHUB_RUN_ID=777
export SPEC=493
export WASM="$tmp/subtensor.wasm"
export RELEASE_SHA="$release_sha"
export RUNTIME_PROBE="$tmp/probe"
export POLL_SECONDS=0
export MOCK_PROBE_SEQUENCE="$tmp/sequence"
export MOCK_PROBE_CALLS="$tmp/probe-calls"

run_case() {
  local name="$1" expected_status="$2" expected_live="$3"
  local sequence="$4"
  rm -f "$MOCK_PROBE_CALLS"
  printf '%b\n' "$sequence" > "$MOCK_PROBE_SEQUENCE"
  export GITHUB_OUTPUT="$tmp/output-$name"
  : > "$GITHUB_OUTPUT"
  local status=0
  "$waiter" > "$tmp/log-$name" 2>&1 || status=$?
  if [ "$status" -ne "$expected_status" ]; then
    echo "FAIL $name: exit $status, expected $expected_status" >&2
    cat "$tmp/log-$name" >&2
    exit 1
  fi
  local live
  live=$(sed -n 's/^live=//p' "$GITHUB_OUTPUT")
  if [ "$live" != "$expected_live" ]; then
    echo "FAIL $name: live='$live', expected '$expected_live'" >&2
    cat "$tmp/log-$name" >&2
    exit 1
  fi
  echo "ok $name"
}

defaults() {
  export WAIT_MINUTES=5
  export MOCK_TAG_SHA="$release_sha"
  export MOCK_PROPOSE_STATUS=completed
  export MOCK_PROPOSE_CONCLUSION=success
  export MOCK_MAIN_SPEC=493
}

defaults
run_case live-after-waiting 0 true "492 $other_hash\n492 $other_hash\n493 $code_hash"
[ "$(cat "$MOCK_PROBE_CALLS")" -eq 3 ] || { echo "FAIL live-after-waiting: expected 3 probes" >&2; exit 1; }

defaults
run_case transient-rpc-error 0 true "fail\n493 $code_hash"

defaults
export MOCK_PROPOSE_STATUS=in_progress MOCK_PROPOSE_CONCLUSION=""
run_case proposal-still-running 0 true "492 $other_hash\n493 $code_hash"

defaults
run_case superseded-on-chain 0 false "494 $other_hash"

defaults
run_case other-runtime-at-spec 1 "" "493 $other_hash"

defaults
export MOCK_TAG_SHA="$other_sha"
run_case tag-names-another-commit 0 false "493 $code_hash"

defaults
export MOCK_TAG_SHA=""
run_case tag-missing 0 false "493 $code_hash"

defaults
export MOCK_PROPOSE_CONCLUSION=failure
run_case proposal-failed 1 "" "492 $other_hash"

defaults
export MOCK_PROPOSE_CONCLUSION=skipped
run_case proposal-skipped 0 false "492 $other_hash"

defaults
export MOCK_MAIN_SPEC=494
run_case newer-train-queued 0 false "492 $other_hash"

defaults
export WAIT_MINUTES=0
run_case deadline 1 "" "492 $other_hash"
grep -q 'not live on mainnet after 0 minutes' "$tmp/log-deadline" \
  || { echo "FAIL deadline: missing retry guidance" >&2; exit 1; }

echo "all await-mainnet-enactment cases passed"
