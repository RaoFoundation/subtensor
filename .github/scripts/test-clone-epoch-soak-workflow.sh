#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
workflow="$repo_root/.github/workflows/clone-epoch-soak.yml"
soak_script="$repo_root/clones/scripts/run-clone-epoch-soak.sh"
monitor_script="$repo_root/clones/scripts/run-clone-block-monitor.sh"
supervisor_script="$repo_root/clones/scripts/clone-process-supervision.sh"
package="$repo_root/clones/js-tests/package.json"

ruby -e 'require "yaml"; YAML.parse_file(ARGV.fetch(0))' "$workflow"
bash -n "$soak_script"
bash -n "$monitor_script"
bash -n "$supervisor_script"

grep -Fq 'workflow_dispatch:' "$workflow"
grep -Fq 'types: [labeled]' "$workflow"
grep -Fq "github.event.label.name == 'run-clone-epoch-soak'" "$workflow"
grep -Fq 'github.event.pull_request.head.repo.fork == false' "$workflow"
if grep -Eq '^[[:space:]]*(push|schedule):' "$workflow"; then
  echo "clone epoch soak must remain manually triggered" >&2
  exit 1
fi
grep -Fq 'default: "2"' "$workflow"
[[ $(grep -Fc 'runs-on: [self-hosted, fireactions-turbo-8]' "$workflow") -eq 2 ]]
grep -Fq 'timeout-minutes: 150' "$workflow"
grep -Fq 'retention-days: 14' "$workflow"
grep -Fq 'gzip -9 -f clone-node.log' "$workflow"
grep -Fq 'clone-node.log.gz' "$workflow"
grep -Fq 'inputs.fresh_state != true' "$workflow"
grep -Fq 'snapshot-artifact.sh select' "$workflow"
grep -Fq 'run-clone-epoch-soak.sh "${{ inputs.epoch_cycles || '\''2'\'' }}"' "$workflow"

grep -Fq 'start-local-clone-and-wait.sh" accelerated' "$soak_script"
grep -Fq 'run-clone-block-monitor.sh" collect soak' "$soak_script"
if grep -Fq -- '--migration' "$soak_script"; then
  echo "epoch soak must not permit bypassing its migration gate" >&2
  exit 1
fi
jq -e '.scripts["monitor:block-latency"] and .scripts["soak:epochs"] and .scripts["test:clone-performance"]' \
  "$package" >/dev/null

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p \
  "$tmp/repo/clones/scripts" \
  "$tmp/repo/clones/js-tests/temp" \
  "$tmp/bin"
cp "$soak_script" "$supervisor_script" "$tmp/repo/clones/scripts/"

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
cat > "$tmp/repo/clones/scripts/start-local-clone-and-wait.sh" <<'EOF'
#!/usr/bin/env bash
repo_root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
printf 'start %s\n' "$*" >> "$HARNESS_LOG"
: > "$repo_root/clone-node.log"
EOF
cat > "$tmp/repo/clones/scripts/stop-local-clone.sh" <<'EOF'
#!/usr/bin/env bash
printf 'stop\n' >> "$HARNESS_LOG"
EOF
cat > "$tmp/bin/npm" <<'EOF'
#!/usr/bin/env bash
printf 'npm %s\n' "$*" >> "$HARNESS_LOG"
if [[ "$*" == *"soak:epochs"* ]]; then
  exit "${MOCK_EPOCH_STATUS:-0}"
fi
EOF
chmod +x "$tmp/repo/clones/scripts/"*.sh "$tmp/bin/npm"

export PATH="$tmp/bin:$PATH"
export HARNESS_LOG="$tmp/harness.log"
deadline=$(( $(date +%s) * 1000 + 60000 ))
SOAK_DEADLINE_EPOCH_MS="$deadline" "$tmp/repo/clones/scripts/run-clone-epoch-soak.sh" 2
grep -Fq 'start accelerated' "$HARNESS_LOG"
grep -Fq 'monitor start collect soak ' "$HARNESS_LOG"
grep -Fq 'npm run soak:epochs -- --epoch-cycles 2' "$HARNESS_LOG"
grep -Fq 'stop' "$HARNESS_LOG"

: > "$HARNESS_LOG"
if MOCK_EPOCH_STATUS=1 SOAK_DEADLINE_EPOCH_MS="$deadline" \
  "$tmp/repo/clones/scripts/run-clone-epoch-soak.sh" 2; then
  echo "failed epoch coverage unexpectedly succeeded" >&2
  exit 1
fi
grep -Fq 'stop' "$HARNESS_LOG"

: > "$HARNESS_LOG"
if MOCK_MONITOR_FAIL=true SOAK_DEADLINE_EPOCH_MS="$deadline" \
  "$tmp/repo/clones/scripts/run-clone-epoch-soak.sh" 2; then
  echo "failed soak block monitor unexpectedly succeeded" >&2
  exit 1
fi
grep -Fq 'stop' "$HARNESS_LOG"

echo "clone epoch soak workflow contract tests passed"
