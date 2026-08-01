#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
source_script="$repo_root/clones/scripts/run-clone-regression-phase.sh"
supervisor_script="$repo_root/clones/scripts/clone-process-supervision.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/repo/clones/scripts" "$tmp/repo/clones/js-tests" "$tmp/repo/sdk/python" "$tmp/bin"
cp "$source_script" "$supervisor_script" "$tmp/repo/clones/scripts/"

for helper in start-local-clone-and-wait.sh stop-local-clone.sh local-clone-checkpoint.sh; do
  cat > "$tmp/repo/clones/scripts/$helper" <<'EOF'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$HARNESS_LOG"
EOF
  chmod +x "$tmp/repo/clones/scripts/$helper"
done

cat > "$tmp/repo/clones/scripts/run-clone-block-monitor.sh" <<'EOF'
#!/usr/bin/env bash
printf 'monitor start %s\n' "$*" >> "$HARNESS_LOG"
if [[ "${MOCK_MONITOR_FAIL:-false}" == true ]]; then
  exit 1
fi
trap 'printf "monitor stop\n" >> "$HARNESS_LOG"; exit 0' TERM INT
while true; do
  /bin/sleep 0.02
done
EOF

cat > "$tmp/bin/npm" <<'EOF'
#!/usr/bin/env bash
printf 'npm %s phase=%s cwd=%s\n' "$*" "${CLONE_REGRESSION_PHASE:-}" "$PWD" >> "$HARNESS_LOG"
if [[ -n "${MOCK_NPM_FAIL_ONCE:-}" && "$*" == *"$MOCK_NPM_FAIL_ONCE"* && ! -e "$MOCK_NPM_STATE" ]]; then
  : > "$MOCK_NPM_STATE"
  exit 1
fi
if [[ -n "${MOCK_NPM_FAIL:-}" && "$*" == *"$MOCK_NPM_FAIL"* ]]; then
  exit 1
fi
EOF
cat > "$tmp/bin/sleep" <<'EOF'
#!/usr/bin/env bash
printf 'sleep %s\n' "$*" >> "$HARNESS_LOG"
EOF
cat > "$tmp/bin/uv" <<'EOF'
#!/usr/bin/env bash
printf 'uv %s cwd=%s\n' "$*" "$PWD" >> "$HARNESS_LOG"
EOF
chmod +x \
  "$tmp/bin/npm" \
  "$tmp/bin/sleep" \
  "$tmp/bin/uv" \
  "$tmp/repo/clones/scripts/run-clone-block-monitor.sh" \
  "$tmp/repo/clones/scripts/run-clone-regression-phase.sh"

export PATH="$tmp/bin:$PATH"
export HARNESS_LOG="$tmp/harness.log"
export MOCK_NPM_STATE="$tmp/npm-state"
checkpoint="$tmp/checkpoint.tar"
: > "$checkpoint"

run_phase() {
  : > "$HARNESS_LOG"
  RUN_SDK_DRIFT=false CLONE_CHECKPOINT="$checkpoint" \
    "$tmp/repo/clones/scripts/run-clone-regression-phase.sh" "$1"
}

run_phase pristine
grep -Fq 'start-local-clone-and-wait.sh accelerated' "$HARNESS_LOG"
grep -Fq 'npm run runtime:update:alice' "$HARNESS_LOG"
grep -Fq 'npm run test:clone-regressions phase=pristine' "$HARNESS_LOG"
grep -Fq 'monitor start fail-fast pristine ' "$HARNESS_LOG"
grep -Fq 'monitor stop' "$HARNESS_LOG"
grep -Fq 'stop-local-clone.sh ' "$HARNESS_LOG"
if grep -Fq 'npm test' "$HARNESS_LOG"; then
  echo "pristine phase unexpectedly ran remaining smoke tests" >&2
  exit 1
fi

run_phase remaining
grep -Fq 'npm test phase=' "$HARNESS_LOG"
grep -Fq 'npm run test:clone-regressions phase=remaining' "$HARNESS_LOG"
grep -Fq 'monitor start fail-fast remaining ' "$HARNESS_LOG"

run_phase combined
[[ $(grep -Fc 'start-local-clone-and-wait.sh accelerated' "$HARNESS_LOG") -eq 2 ]]
[[ $(grep -Fc 'monitor start fail-fast' "$HARNESS_LOG") -eq 2 ]]
grep -Fq "local-clone-checkpoint.sh restore $checkpoint" "$HARNESS_LOG"
grep -Fq 'npm run test:clone-regressions phase=pristine' "$HARNESS_LOG"
grep -Fq 'npm run test:clone-regressions phase=remaining' "$HARNESS_LOG"

: > "$HARNESS_LOG"
RUN_SDK_DRIFT=true "$tmp/repo/clones/scripts/run-clone-regression-phase.sh" remaining
grep -Fq 'uv sync --locked --all-extras --dev' "$HARNESS_LOG"
grep -Fq 'uv run python -m codegen.check --drift ws://127.0.0.1:9944' "$HARNESS_LOG"

: > "$HARNESS_LOG"
rm -f "$MOCK_NPM_STATE"
MOCK_NPM_FAIL_ONCE=runtime:update:alice \
  RUN_SDK_DRIFT=false \
  "$tmp/repo/clones/scripts/run-clone-regression-phase.sh" remaining
[[ $(grep -Fc 'npm run runtime:update:alice' "$HARNESS_LOG") -eq 2 ]]
grep -Fq 'sleep 15' "$HARNESS_LOG"

: > "$HARNESS_LOG"
if MOCK_NPM_FAIL=test:clone-regressions \
  RUN_SDK_DRIFT=false \
  "$tmp/repo/clones/scripts/run-clone-regression-phase.sh" pristine; then
  echo "failed clone regression phase unexpectedly succeeded" >&2
  exit 1
fi
grep -Fq 'stop-local-clone.sh ' "$HARNESS_LOG"

: > "$HARNESS_LOG"
if MOCK_MONITOR_FAIL=true \
  RUN_SDK_DRIFT=false \
  "$tmp/repo/clones/scripts/run-clone-regression-phase.sh" pristine; then
  echo "failed clone block monitor unexpectedly succeeded" >&2
  exit 1
fi
grep -Fq 'monitor start fail-fast pristine ' "$HARNESS_LOG"
grep -Fq 'stop-local-clone.sh ' "$HARNESS_LOG"

if "$tmp/repo/clones/scripts/run-clone-regression-phase.sh" invalid >/dev/null 2>&1; then
  echo "invalid clone phase was accepted" >&2
  exit 1
fi

echo "clone regression phase harness tests passed"
