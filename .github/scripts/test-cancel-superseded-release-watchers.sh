#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
helper="$script_dir/cancel-superseded-release-watchers.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

endpoint=''
for argument in "$@"; do
  case "$argument" in
    repos/*) endpoint="$argument" ;;
  esac
done

case "$endpoint" in
  *'/runs?status=waiting&'*) printf '%s\n' 100 200 300 400 500 ;;
  *'/runs?status='*) ;;
  */runs/100/jobs*)
    printf '%s\t%s\n' 'Cut GitHub release v441' completed
    printf '%s\t%s\n' 'Publish Python packages to PyPI' in_progress
    ;;
  */runs/200/jobs*)
    printf '%s\t%s\n' 'Cut GitHub release v442' waiting
    ;;
  */runs/300/jobs*)
    printf '%s\t%s\n' 'Cut GitHub release v443' waiting
    ;;
  */runs/400/jobs*)
    printf '%s\t%s\n' 'Cut GitHub release v444' waiting
    ;;
  */runs/500/jobs*)
    printf '%s\t%s\n' 'Cut GitHub release' completed
    printf '%s\t%s\n' 'Publish Python packages to PyPI' waiting
    ;;
  */runs/200/cancel) printf '%s\n' 200 >> "$MOCK_CANCELS" ;;
  *) echo "unexpected mock gh endpoint: $endpoint" >&2; exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/gh"

export PATH="$tmp/bin:$PATH"
export GH_TOKEN=test-job-token
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export GITHUB_RUN_ID=100
export MOCK_CANCELS="$tmp/cancels"
export GITHUB_OUTPUT="$tmp/output"
: > "$MOCK_CANCELS"
: > "$GITHUB_OUTPUT"

output=$("$helper" 443)
grep -qx '200' "$MOCK_CANCELS"
grep -q 'Canceling watcher run 200 for superseded runtime v442' <<<"$output"
grep -q 'Watcher run 300 already owns the runtime v443 release slot' <<<"$output"
grep -qx 'release_slot_available=false' "$GITHUB_OUTPUT"

: > "$MOCK_CANCELS"
: > "$GITHUB_OUTPUT"
"$helper" 441 >/dev/null
[ ! -s "$MOCK_CANCELS" ]
grep -qx 'release_slot_available=true' "$GITHUB_OUTPUT"

if "$helper" not-a-spec >/dev/null 2>&1; then
  echo "expected a malformed spec to fail" >&2
  exit 1
fi

echo "superseded release watcher cancellation tests passed"
