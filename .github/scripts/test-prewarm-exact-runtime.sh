#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/runner"

cat > "$tmp/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  clean) exit 0 ;;
  check)
    [[ "$*" == "check --locked -p node-subtensor-runtime" ]]
    ;;
  *) exit 2 ;;
esac
EOF

cat > "$tmp/bin/sccache" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  --zero-stats) ;;
  --show-stats)
    cat <<STATS
Cache hits (Rust) ${MOCK_RUST_HITS:-600}
Cache misses (Rust) ${MOCK_RUST_MISSES:-0}
STATS
    ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/cargo" "$tmp/bin/sccache"

export PATH="$tmp/bin:$PATH"
export RUNNER_TEMP="$tmp/runner"
export GITHUB_STEP_SUMMARY="$tmp/summary"
export SCCACHE_ENABLED=true
export SCCACHE_BACKEND=r2
export SCCACHE_LOCAL_TIER=false

: > "$GITHUB_STEP_SUMMARY"
"$script_dir/prewarm-exact-runtime.sh" >/dev/null
grep -q '^### Exact runtime-only R2 prewarm$' "$GITHUB_STEP_SUMMARY"
grep -q 'Clean verification: .*600 Rust hits, 0 misses' "$GITHUB_STEP_SUMMARY"

if MOCK_RUST_MISSES=11 "$script_dir/prewarm-exact-runtime.sh" >/dev/null 2>&1; then
  echo "expected excessive exact-key misses to fail verification" >&2
  exit 1
fi

echo "exact runtime prewarm tests passed"
