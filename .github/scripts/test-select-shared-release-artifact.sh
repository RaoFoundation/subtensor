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
  */actions/artifacts*) cat "$MOCK_ARTIFACTS" ;;
  */actions/runs/777) cat "$MOCK_RUN" ;;
  */commits/*/pulls) cat "$MOCK_PULLS" ;;
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
export GITHUB_EVENT_NAME=pull_request
export MOCK_ARTIFACTS="$tmp/artifacts.json"
export MOCK_RUN="$tmp/run.json"
export MOCK_PULLS="$tmp/pulls.json"

cat > "$MOCK_ARTIFACTS" <<'EOF'
{"artifacts":[{"id":123,"name":"node-subtensor-release-cccccccccccccccccccccccccccccccccccccccc","size_in_bytes":456,"digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","expired":false,"created_at":"2026-07-17T00:00:00Z","workflow_run":{"id":777,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository_id":608683796}}]}
EOF
cat > "$MOCK_RUN" <<'EOF'
{"id":777,"event":"pull_request","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_repository":{"id":608683796},"path":".github/workflows/runtime-checks.yml"}
EOF
cat > "$MOCK_PULLS" <<'EOF'
[{"state":"open","merge_commit_sha":"cccccccccccccccccccccccccccccccccccccccc","head":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","repo":{"id":608683796}},"base":{"repo":{"id":608683796}}}]
EOF

: > "$tmp/output"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_id=123' "$tmp/output"
grep -qx 'artifact_name=node-subtensor-release-cccccccccccccccccccccccccccccccccccccccc' "$tmp/output"
grep -qx 'artifact_sha=cccccccccccccccccccccccccccccccccccccccc' "$tmp/output"
grep -qx 'run_id=777' "$tmp/output"
grep -qx 'size=456' "$tmp/output"
grep -qx 'digest=sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' "$tmp/output"

# A matching PR head is insufficient after the base branch changes: the
# synthetic merge SHA in the artifact name must match this run exactly.
jq '.artifacts[0].name = "node-subtensor-release-dddddddddddddddddddddddddddddddddddddddd"' \
  "$MOCK_ARTIFACTS" > "$tmp/stale-merge-artifacts.json"
export MOCK_ARTIFACTS="$tmp/stale-merge-artifacts.json"
: > "$tmp/output"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# Exact source SHAs alone are still insufficient: refuse an artifact produced
# by any other workflow, then use the unchanged local-build fallback.
export MOCK_ARTIFACTS="$tmp/artifacts.json"
jq '.path = ".github/workflows/untrusted-producer.yml"' "$MOCK_RUN" > "$tmp/wrong-run.json"
export MOCK_RUN="$tmp/wrong-run.json"
: > "$tmp/output"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# Reject malformed integrity metadata before it reaches the downloader.
export MOCK_RUN="$tmp/run.json"
jq '.artifacts[0].digest = "sha256:bad"' "$MOCK_ARTIFACTS" > "$tmp/bad-artifacts.json"
export MOCK_ARTIFACTS="$tmp/bad-artifacts.json"
: > "$tmp/output"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=false' "$tmp/output"

# A manual dispatch runs at the source head SHA. Resolve its sole open,
# same-repository PR to the synthetic merge SHA that names the trusted build.
export GITHUB_EVENT_NAME=workflow_dispatch
export GITHUB_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
export MOCK_ARTIFACTS="$tmp/artifacts.json"
export MOCK_RUN="$tmp/run.json"
: > "$tmp/output"
"$selector" "$tmp/output" 0 >/dev/null
grep -qx 'found=true' "$tmp/output"
grep -qx 'artifact_sha=cccccccccccccccccccccccccccccccccccccccc' "$tmp/output"

jq '. + [.[0]]' "$MOCK_PULLS" > "$tmp/ambiguous-pulls.json"
export MOCK_PULLS="$tmp/ambiguous-pulls.json"
if "$selector" "$tmp/output" 0 >/dev/null 2>&1; then
  echo "ambiguous manual PR context unexpectedly succeeded" >&2
  exit 1
fi

echo "shared release artifact selector tests passed"
