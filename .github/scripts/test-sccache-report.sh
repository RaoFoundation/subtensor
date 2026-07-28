#!/usr/bin/env bash

set -euo pipefail
trap 'printf "sccache report test failed at line %s: %s\n" "$LINENO" "$BASH_COMMAND" >&2' ERR

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPORT="$SCRIPT_DIR/sccache-report.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/runner"

cat >"$tmp/bin/sccache" <<'EOF'
#!/usr/bin/env bash
set -u
[[ "${MOCK_ALL_FAIL:-false}" != true ]] || exit 1
case "${1:-}" in
  --show-adv-stats)
    [[ "${MOCK_ADV_FAIL:-false}" != true ]] || exit 1
    printf 'Cache hits (rust) 7\nCache misses (rust) 1\n'
    ;;
  --show-stats)
    if [[ "$*" == *--stats-format=json* ]]; then
      printf '{"stats":{"cache_hits":{"counts":{"Rust":7}},"cache_misses":{"counts":{"Rust":1}}}}\n'
    else
      printf 'Cache hits (Rust) 7\nCache misses (Rust) 1\n'
    fi
    ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/sccache"

export PATH="$tmp/bin:$PATH"
export RUNNER_TEMP="$tmp/runner"
export GITHUB_STEP_SUMMARY="$tmp/summary"
export SCCACHE_ENABLED=true
export SCCACHE_BACKEND=r2
export SCCACHE_LOCAL_TIER=true

"$REPORT" "Runtime cache" "$tmp/advanced"
grep -Fq 'Cache hits (rust) 7' "$tmp/advanced.txt"
grep -Fq '"Rust":7' "$tmp/advanced.json"
grep -Fq 'Configured backend: r2' "$GITHUB_STEP_SUMMARY"
grep -Fq 'combines host-local and R2 hits' "$GITHUB_STEP_SUMMARY"

export MOCK_ADV_FAIL=true
"$REPORT" "Fallback cache" "$tmp/fallback"
grep -Fq 'Cache hits (Rust) 7' "$tmp/fallback.txt"

export MOCK_ALL_FAIL=true
"$REPORT" "Unavailable cache" "$tmp/unavailable"
grep -Fq 'statistics were unavailable' "$tmp/unavailable.txt"

export SCCACHE_ENABLED=false
"$REPORT" "Disabled cache" "$tmp/disabled"
grep -Fq 'sccache is disabled' "$tmp/disabled.txt"

echo "sccache report tests passed"
