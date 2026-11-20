#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
helper="$script_dir/release-train-enactment-state.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
endpoint="${!#}"
case "$endpoint" in
  */actions/workflows/release-train.yml/runs*)
    [ "${MOCK_RUNS_FAIL:-false}" != true ] || { echo 'HTTP 502' >&2; exit 1; }
    cat "$MOCK_RUNS"
    ;;
  */actions/runs/*/jobs*)
    run=$(sed -E 's#.*/actions/runs/([0-9]+)/jobs.*#\1#' <<<"$endpoint")
    cat "$MOCK_DIR/jobs-$run.json"
    ;;
  *) echo "unexpected endpoint: $endpoint" >&2; exit 2 ;;
esac
EOF
chmod +x "$tmp/bin/gh"

export PATH="$tmp/bin:$PATH"
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export MOCK_RUNS="$tmp/runs.json"
export MOCK_DIR="$tmp"
commit=$(printf 'a%.0s' {1..40})

job() { # name status conclusion step step_conclusion
  jq -n --arg name "$1" --arg status "$2" --arg conclusion "$3" --arg step "$4" --arg sc "$5" \
    '{name: $name, status: $status, conclusion: (if $conclusion == "" then null else $conclusion end),
      steps: [{name: $step, conclusion: (if $sc == "" then null else $sc end)}]}'
}
python_job() { job "Publish stable Python once mainnet runs this upgrade" "$1" "$2" "Publish to PyPI" "$3"; }
docs_job() { job "Deploy production docs once mainnet runs this upgrade" "$1" "$2" "Deploy to Vercel (production)" "$3"; }
jobs_file() { # run python-json docs-json
  jq -n --argjson p "$2" --argjson d "$3" '{jobs: [$p, $d]}' > "$tmp/jobs-$1.json"
}
runs() {
  jq -n --args '{workflow_runs: [$ARGS.positional[] | {id: (. | tonumber), head_branch: "main"}]}' "$@" > "$MOCK_RUNS"
}

check() {
  local name="$1" python="$2" docs="$3" out
  out=$("$helper" "$commit" 2>/dev/null)
  if [ "$out" != "$(printf 'train_publishes_python=%s\ntrain_deploys_docs=%s' "$python" "$docs")" ]; then
    echo "FAIL $name: got"; echo "$out"; exit 1
  fi
  echo "ok $name"
}

runs 11
jobs_file 11 "$(python_job in_progress "" "")" "$(docs_job in_progress "" "")"
check still-waiting true true

jobs_file 11 "$(python_job completed success success)" "$(docs_job completed success success)"
check published-and-deployed true true

# Stopped for a newer spec: green jobs whose publish steps were skipped.
jobs_file 11 "$(python_job completed success skipped)" "$(docs_job completed success skipped)"
check stopped-for-newer-spec false false

jobs_file 11 "$(python_job completed failure "")" "$(docs_job completed failure "")"
check gave-up false false

# A later dispatch of the same commit is still waiting.
runs 11 12
jobs_file 12 "$(python_job waiting "" "")" "$(docs_job completed success success)"
check second-run-waiting true true

runs
check deployed-outside-train false false

export MOCK_RUNS_FAIL=true
check api-failure-falls-back false false

echo "all release-train-enactment-state cases passed"
