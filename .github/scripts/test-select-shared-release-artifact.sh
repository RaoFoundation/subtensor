#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
selector="$script_dir/select-shared-release-artifact.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
endpoint="${!#}"
case "$endpoint" in
  */actions/artifacts*)
    calls=0
    [[ ! -f "$MOCK_ARTIFACT_CALLS" ]] || calls=$(cat "$MOCK_ARTIFACT_CALLS")
    calls=$((calls + 1))
    echo "$calls" > "$MOCK_ARTIFACT_CALLS"
    delay_calls=${MOCK_ARTIFACT_DELAY_CALLS:-0}
    [[ "${MOCK_DELAY_ARTIFACT:-false}" != true ]] || delay_calls=1
    if (( calls <= delay_calls )); then
      echo '{"artifacts":[]}'
    else
      cat "$MOCK_ARTIFACTS"
    fi
    ;;
  */actions/runs/777/jobs*)
    job_calls=0
    [[ ! -f "$MOCK_JOBS_CALLS" ]] || job_calls=$(cat "$MOCK_JOBS_CALLS")
    job_calls=$((job_calls + 1))
    echo "$job_calls" > "$MOCK_JOBS_CALLS"
    if [[ "${MOCK_FAIL_JOBS_ON_CALL:-0}" == "$job_calls" ]]; then
      echo 'transient API failure' >&2
      exit 1
    fi
    cat "$MOCK_JOBS"
    ;;
  */actions/runs/888/jobs*) cat "$MOCK_REPLACEMENT_JOBS" ;;
  */actions/runs\?*)
    run_calls=0
    [[ ! -f "$MOCK_RUNS_CALLS" ]] || run_calls=$(cat "$MOCK_RUNS_CALLS")
    run_calls=$((run_calls + 1))
    echo "$run_calls" > "$MOCK_RUNS_CALLS"
    if [[ -n "${MOCK_RUNS_AFTER_FIRST:-}" && "$run_calls" -gt 1 ]]; then
      cat "$MOCK_RUNS_AFTER_FIRST"
    else
      cat "$MOCK_RUNS"
    fi
    ;;
  */actions/runs/777) cat "$MOCK_RUN" ;;
  */actions/runs/888) cat "$MOCK_REPLACEMENT_RUN" ;;
  *) echo "unexpected endpoint: $endpoint" >&2; exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/gh"

export PATH="$tmp/bin:$PATH"
export GH_TOKEN=test-job-token
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export GITHUB_REPOSITORY_ID=608683796
export GITHUB_SHA=cccccccccccccccccccccccccccccccccccccccc
export GITHUB_PR_HEAD_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
export MOCK_ARTIFACTS="$tmp/artifacts.json"
export MOCK_ARTIFACT_CALLS="$tmp/artifact-calls"
export MOCK_RUN="$tmp/run.json"
export MOCK_RUNS="$tmp/runs.json"
export MOCK_JOBS="$tmp/jobs.json"
export MOCK_JOBS_CALLS="$tmp/job-calls"
export MOCK_RUNS_CALLS="$tmp/run-calls"
export MOCK_REPLACEMENT_JOBS="$tmp/replacement-jobs.json"
export MOCK_REPLACEMENT_RUN="$tmp/replacement-run.json"
export SELECTOR_POLL_SECONDS=0
export PRODUCER_ARTIFACT_GRACE_SECONDS=0
export PRODUCER_MAX_WAIT_SECONDS=10

cat > "$MOCK_ARTIFACTS" <<'EOF'
{"artifacts":[{"id":123,"name":"node-subtensor-release-cccccccccccccccccccccccccccccccccccccccc","size_in_bytes":456,"digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","expired":false,"created_at":"2026-07-17T00:00:00Z","workflow_run":{"id":777,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository_id":608683796}}]}
EOF
cat > "$MOCK_RUN" <<'EOF'
{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml"}
EOF
cat > "$MOCK_RUNS" <<'EOF'
{"workflow_runs":[]}
EOF
cat > "$MOCK_JOBS" <<'EOF'
{"jobs":[]}
EOF
cat > "$MOCK_REPLACEMENT_JOBS" <<'EOF'
{"jobs":[]}
EOF
cat > "$MOCK_REPLACEMENT_RUN" <<'EOF'
{"id":888,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml"}
EOF

: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_id=123' "$tmp/output"
grep -qx 'run_id=777' "$tmp/output"
grep -qx 'size=456' "$tmp/output"
grep -qx 'digest=sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' "$tmp/output"

# A matching PR head is insufficient after the base branch changes: the
# synthetic merge SHA in the artifact name must match this run exactly.
jq '.artifacts[0].name = "node-subtensor-release-dddddddddddddddddddddddddddddddddddddddd"' \
  "$MOCK_ARTIFACTS" > "$tmp/stale-merge-artifacts.json"
export MOCK_ARTIFACTS="$tmp/stale-merge-artifacts.json"
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# Exact source SHAs alone are still insufficient: refuse an artifact produced
# by any other workflow, then use the unchanged local-build fallback.
export MOCK_ARTIFACTS="$tmp/artifacts.json"
jq '.path = ".github/workflows/untrusted-producer.yml"' "$MOCK_RUN" > "$tmp/wrong-run.json"
export MOCK_RUN="$tmp/wrong-run.json"
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# Reject malformed integrity metadata before it reaches the downloader.
export MOCK_RUN="$tmp/run.json"
jq '.artifacts[0].digest = "sha256:bad"' "$MOCK_ARTIFACTS" > "$tmp/bad-artifacts.json"
export MOCK_ARTIFACTS="$tmp/bad-artifacts.json"
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# Once the exact Runtime Checks producer is active, do not let the ordinary
# producer-discovery timeout trigger a duplicate release build. The artifact
# may appear just after that boundary, as it did on PR #3035.
export MOCK_ARTIFACTS="$tmp/artifacts.json"
export MOCK_RUN="$tmp/run.json"
cat > "$MOCK_RUNS" <<'EOF'
{"workflow_runs":[{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"in_progress","conclusion":null,"created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$MOCK_JOBS" <<'EOF'
{"jobs":[{"name":"build release node","status":"waiting","conclusion":null,"created_at":"2026-07-17T00:00:00Z"}]}
EOF
export MOCK_DELAY_ARTIFACT=true
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_id=123' "$tmp/output"
unset MOCK_DELAY_ARTIFACT

# After a producer has been discovered, a transient jobs API failure must not
# forget it and re-enable the already-expired discovery timeout.
export MOCK_ARTIFACT_DELAY_CALLS=2
export MOCK_FAIL_JOBS_ON_CALL=2
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
: > "$MOCK_JOBS_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_id=123' "$tmp/output"
unset MOCK_ARTIFACT_DELAY_CALLS MOCK_FAIL_JOBS_ON_CALL

# GitHub may report a successful producer before its artifact is visible. Give
# the artifact API a short propagation window instead of rebuilding at once.
cat > "$MOCK_RUNS" <<'EOF'
{"workflow_runs":[{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"completed","conclusion":"success","created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$MOCK_JOBS" <<'EOF'
{"jobs":[{"name":"build release node","status":"completed","conclusion":"success","created_at":"2026-07-17T00:00:00Z"}]}
EOF
# Keep the behavioral test fast while still exercising a nonzero grace.
export PRODUCER_ARTIFACT_GRACE_SECONDS=1
export MOCK_DELAY_ARTIFACT=true
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
: > "$MOCK_JOBS_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_id=123' "$tmp/output"
unset MOCK_DELAY_ARTIFACT
export PRODUCER_ARTIFACT_GRACE_SECONDS=0

# A completed producer without an artifact exits to the local fallback instead
# of polling until the longer active-producer ceiling.
cat > "$MOCK_ARTIFACTS" <<'EOF'
{"artifacts":[]}
EOF
cat > "$MOCK_RUNS" <<'EOF'
{"workflow_runs":[{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"completed","conclusion":"failure","created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$MOCK_JOBS" <<'EOF'
{"jobs":[{"name":"build release node","status":"completed","conclusion":"failure","created_at":"2026-07-17T00:00:00Z"}]}
EOF
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# A stuck active producer still has a hard ceiling, so a wedged Runtime Checks
# run cannot delay the local E2E release fallback forever.
cat > "$MOCK_RUNS" <<'EOF'
{"workflow_runs":[{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"in_progress","conclusion":null,"created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$MOCK_JOBS" <<'EOF'
{"jobs":[{"name":"build release node","status":"requested","conclusion":null,"created_at":"2026-07-17T00:00:00Z"}]}
EOF
export PRODUCER_MAX_WAIT_SECONDS=0
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"
export PRODUCER_MAX_WAIT_SECONDS=10

# A cancelled producer may have been superseded by a newer Runtime Checks run
# for the same PR head (for example after a label change). Follow the newer run
# instead of immediately starting a duplicate local build.
cat > "$MOCK_ARTIFACTS" <<'EOF'
{"artifacts":[{"id":124,"name":"node-subtensor-release-cccccccccccccccccccccccccccccccccccccccc","size_in_bytes":456,"digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","expired":false,"created_at":"2026-07-17T00:01:00Z","workflow_run":{"id":888,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository_id":608683796}}]}
EOF
cat > "$MOCK_RUNS" <<'EOF'
{"workflow_runs":[{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"completed","conclusion":"cancelled","created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$tmp/replacement-runs.json" <<'EOF'
{"workflow_runs":[{"id":888,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"in_progress","conclusion":null,"created_at":"2026-07-17T00:01:00Z"},{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml","status":"completed","conclusion":"cancelled","created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$MOCK_JOBS" <<'EOF'
{"jobs":[{"name":"build release node","status":"completed","conclusion":"cancelled","created_at":"2026-07-17T00:00:00Z"}]}
EOF
cat > "$MOCK_REPLACEMENT_JOBS" <<'EOF'
{"jobs":[{"name":"build release node","status":"waiting","conclusion":null,"created_at":"2026-07-17T00:01:00Z"}]}
EOF
export MOCK_RUNS_AFTER_FIRST="$tmp/replacement-runs.json"
export MOCK_ARTIFACT_DELAY_CALLS=2
: > "$tmp/output"
: > "$MOCK_ARTIFACT_CALLS"
: > "$MOCK_RUNS_CALLS"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_id=124' "$tmp/output"
grep -qx 'run_id=888' "$tmp/output"
unset MOCK_RUNS_AFTER_FIRST MOCK_ARTIFACT_DELAY_CALLS

echo "shared release artifact selector tests passed"
