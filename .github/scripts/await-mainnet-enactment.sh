#!/usr/bin/env bash
# Wait until mainnet's finalized runtime is this release train's runtime.
#
# release-train.yml runs this inside the jobs that are approved together with
# propose-mainnet, so the one `mainnet` approval covers the proposal and the
# publications that follow once the triumvirate executes it.
#
# Required environment:
#   SPEC              spec_version this train built and proposed
#   WASM              path to this train's srtool subtensor.wasm
#   RELEASE_SHA       commit this train runs (GITHUB_SHA)
#   MAINNET_HTTP      mainnet JSON-RPC endpoint
#   GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_OUTPUT, GH_TOKEN
# Optional:
#   WAIT_MINUTES      give up after this long (default 1200, 20 hours; the
#                     job's GITHUB_TOKEN cannot outlive 24 hours)
#   POLL_SECONDS      delay between mainnet checks (default 120)
#   MAIN_EVERY        check main's spec_version every Nth poll (default 5);
#                     GITHUB_TOKEN shares a per-repository hourly API budget
#   PROPOSE_JOB       name of the proposal job in this run
#   SUBMIT_STEP       name of its step that puts the multisig on chain
#   RUNTIME_PROBE     command printing finalized spec_version and code_hash
#
# Writes code_hash=<blake2_256(WASM)> and live=true|false to $GITHUB_OUTPUT
# and exits 0 when:
#   live=true   mainnet finalized SPEC with code hash blake2_256(WASM), and
#               the immutable tag vSPEC names RELEASE_SHA.
#   live=false  this train has nothing to publish: mainnet moved past SPEC,
#               main now carries a newer spec (a newer train is queued behind
#               this one), propose-mainnet was skipped, or vSPEC names another
#               commit.
# Exits non-zero when this run's proposal job failed before submitting the
# multisig, when mainnet runs a different runtime under SPEC with vSPEC on
# RELEASE_SHA, or when SPEC is not live within WAIT_MINUTES.
#
# Giving up never makes the publication impossible. When SPEC goes live and
# no train job is publishing it, the scheduled watch-mainnet-release.yml run
# asks for the PyPI and docs approval itself, and while SPEC is the runtime on
# mainnet a dispatch of that workflow rebuilds from the v<spec> tag and
# publishes it (one `mainnet` approval), with no deadline.

set -euo pipefail

: "${SPEC:?SPEC is required}"
: "${WASM:?WASM is required}"
: "${RELEASE_SHA:?RELEASE_SHA is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${GITHUB_RUN_ID:?GITHUB_RUN_ID is required}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
[[ "$SPEC" =~ ^[0-9]+$ ]] || { echo "SPEC must be an integer" >&2; exit 2; }
[[ "$RELEASE_SHA" =~ ^[0-9a-f]{40}$ ]] || { echo "RELEASE_SHA must be a commit sha" >&2; exit 2; }
[ -s "$WASM" ] || { echo "runtime wasm $WASM is missing" >&2; exit 2; }

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
wait_minutes="${WAIT_MINUTES:-1200}"
poll_seconds="${POLL_SECONDS:-120}"
propose_job="${PROPOSE_JOB:-Propose mainnet upgrade (multisig)}"
submit_step="${SUBMIT_STEP:-Submit multisig proposal (CI half of deployment multisig)}"
runtime_probe="${RUNTIME_PROBE:-$script_dir/mainnet-finalized-runtime.sh}"

# A hung GitHub call must fail one poll, not stall the loop past its
# deadline.
gh_api() {
  if command -v timeout >/dev/null 2>&1; then
    timeout 60 gh api "$@"
  else
    gh api "$@"
  fi
}

expected_hash="0x$(python3 -c '
import hashlib, sys
with open(sys.argv[1], "rb") as wasm:
    print(hashlib.blake2b(wasm.read(), digest_size=32).hexdigest())
' "$WASM")"
echo "waiting for spec $SPEC with code hash $expected_hash from $RELEASE_SHA"

finish() {
  {
    echo "code_hash=$expected_hash"
    echo "live=$1"
  } >> "$GITHUB_OUTPUT"
  echo "$2"
  exit 0
}

# Prints the commit vSPEC names, nothing when the tag does not exist, and
# fails when GitHub cannot say.
tag_commit() {
  local json err
  err=$(mktemp)
  if json=$(gh_api "repos/$GITHUB_REPOSITORY/git/ref/tags/v$SPEC" 2>"$err"); then
    rm -f "$err"
    jq -er 'select(.object.type == "commit") | .object.sha' <<<"$json"
    return
  fi
  if grep -q 'HTTP 404' "$err"; then
    rm -f "$err"
    return 0
  fi
  cat "$err" >&2
  rm -f "$err"
  return 1
}

# Prints "<status> <conclusion> <submit-step conclusion>" for the proposal
# job of this run's latest attempt, or nothing when the job is not listed.
# The step matters because propose-mainnet can fail after the multisig is
# already on chain (recording, manifest, artifact or pre-release steps), and
# re-running it then cannot submit the same approval again.
propose_state() {
  gh_api "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/jobs?filter=latest&per_page=100" \
    | jq -r --arg name "$propose_job" --arg step "$submit_step" \
      'first(.jobs[] | select(.name == $name)
        | "\(.status) \(.conclusion // "-") \(first(.steps[]? | select(.name == $step) | .conclusion // "-") // "-")")
       // ""'
}

main_spec() {
  gh_api -H "Accept: application/vnd.github.raw+json" \
    "repos/$GITHUB_REPOSITORY/contents/runtime/src/lib.rs?ref=main" \
    | grep -Eo 'spec_version: *[0-9]+' | head -n 1 | grep -Eo '[0-9]+'
}

deadline=$(( $(date +%s) + wait_minutes * 60 ))
main_every="${MAIN_EVERY:-5}"
[[ "$main_every" =~ ^[1-9][0-9]*$ ]] || { echo "MAIN_EVERY must be a positive integer" >&2; exit 2; }
propose_succeeded=false
polls=0
while :; do
  if runtime=$("$runtime_probe"); then
    chain_spec=$(sed -n 's/^spec_version=//p' <<<"$runtime")
    chain_hash=$(sed -n 's/^code_hash=//p' <<<"$runtime")
    if [[ "$chain_spec" =~ ^[0-9]+$ ]]; then
      if [ "$chain_spec" -gt "$SPEC" ]; then
        finish false "mainnet runs spec $chain_spec, past $SPEC; this train has nothing to publish."
      fi
      if [ "$chain_spec" -eq "$SPEC" ]; then
        # The tag decides whose release this is. A stale train at the live
        # spec (same spec_version, different wasm, no bump) is not an error.
        if tag=$(tag_commit); then
          [ "$tag" = "$RELEASE_SHA" ] \
            || finish false "v$SPEC names ${tag:-no commit}, not $RELEASE_SHA; this train is not the release."
          if [ "$chain_hash" != "$expected_hash" ]; then
            echo "::error::mainnet runs spec $SPEC with code hash $chain_hash, not this train's $expected_hash; refusing to publish." >&2
            exit 1
          fi
          finish true "mainnet finalized spec $SPEC ($chain_hash) built from $RELEASE_SHA."
        fi
        echo "could not read tag v$SPEC; retrying." >&2
      fi
    else
      echo "mainnet probe returned no spec_version; retrying." >&2
    fi
  else
    echo "could not read mainnet's finalized runtime; retrying." >&2
  fi

  # Once the proposal job has succeeded it cannot change again in this
  # attempt, so stop spending API calls on it.
  if [ "$propose_succeeded" != true ]; then
    if state=$(propose_state); then
      read -r job_status job_conclusion submit_conclusion <<<"${state:-- - -}"
      case "$job_status $job_conclusion" in
        "completed success") propose_succeeded=true ;;
        "completed skipped")
          finish false "propose-mainnet was skipped; this train proposed nothing."
          ;;
        completed*)
          if [ "$submit_conclusion" = success ]; then
            echo "::warning::propose-mainnet finished as '$job_conclusion' after the multisig was submitted; still waiting for it to execute. Do not re-run propose-mainnet: the CI approval is already on chain."
            propose_succeeded=true
          else
            echo "::error::propose-mainnet finished as '$job_conclusion' before submitting the multisig; there is no proposal to wait for." >&2
            exit 1
          fi
          ;;
      esac
    else
      echo "could not read this run's jobs; retrying." >&2
    fi
  fi

  if (( polls % main_every == 0 )) \
    && newer=$(main_spec) && [[ "$newer" =~ ^[0-9]+$ ]] && [ "$newer" -gt "$SPEC" ]; then
    echo "::warning::main now carries spec $newer, so a newer train is queued behind this one and this job stops waiting. If spec $SPEC still goes live on mainnet, the scheduled watcher asks for the PyPI and docs approval itself."
    finish false "stopped waiting for spec $SPEC."
  fi
  polls=$((polls + 1))

  if [ "$(date +%s)" -ge "$deadline" ]; then
    echo "::error::spec $SPEC is not live on mainnet after $wait_minutes minutes. When it goes live, the scheduled watcher asks for the PyPI and docs approval itself; you can also dispatch watch-mainnet-release.yml (for as long as spec $SPEC is the mainnet runtime) or re-run the failed jobs of this run (within 30 days, when no newer train is running)." >&2
    exit 1
  fi
  sleep "$poll_seconds"
done
