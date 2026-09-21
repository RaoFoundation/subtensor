#!/usr/bin/env bash
# Local CI gate. Runs the checks that CI requires (and the drift gates that
# surface late in clone-upgrade) with the same commands CI uses, so a push
# never burns a CI cycle on a failure that was checkable here.
#
#   scripts/preflight.sh            full gate (may build the release node)
#   scripts/preflight.sh --fast     skip the node / wasm builds (SDK regen, try-runtime)
#   scripts/preflight.sh --all      run every gate regardless of what changed
#   scripts/preflight.sh --rev SHA  gate that exact commit in a detached worktree
#                                   (what the pre-push hook does for every pushed ref)
#
# Change detection compares the tree with the merge-base against origin/main
# (override with PREFLIGHT_BASE) and reuses CI's own path classifiers under
# .github/scripts/. Cheap gates run first; compile-heavy gates are skipped
# while any cheap gate is red.
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"
export SKIP_WASM_BUILD=1 # as CI's lint/test jobs; the node and try-runtime wasm builds unset it
FAST=false
ALL=false
REV=''
PASSTHRU=()
while (($#)); do
  case "$1" in
    --fast) FAST=true; PASSTHRU+=("$1") ;;
    --all) ALL=true; PASSTHRU+=("$1") ;;
    --rev) REV=${2:?--rev needs a commit}; shift ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
  shift
done

# --rev: check the commit out into a throwaway worktree and run the gate
# there, so the verdict is about the pushed revision, not the working tree.
# The main checkout lends its cargo target dir, sdk/python venv, and
# ts-tests/node_modules so the run is not cold.
if [[ -n "$REV" ]]; then
  REV=$(git rev-parse --verify "$REV^{commit}")
  WT=$(mktemp -d "${TMPDIR:-/tmp}/preflight-wt.XXXXXX")
  trap 'git worktree remove --force "$WT" 2>/dev/null || rm -rf "$WT"' EXIT
  git worktree add --detach -q "$WT" "$REV"
  export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}
  export UV_PROJECT_ENVIRONMENT=${UV_PROJECT_ENVIRONMENT:-$ROOT/sdk/python/.venv}
  export PYTHONPATH=$WT/sdk/python${PYTHONPATH:+:$PYTHONPATH} # editable install points at $ROOT
  [[ ! -d $ROOT/ts-tests/node_modules || -e $WT/ts-tests/node_modules ]] || ln -s "$ROOT/ts-tests/node_modules" "$WT/ts-tests/node_modules"
  gate=$WT/scripts/preflight.sh
  [[ -x "$gate" ]] || gate=$ROOT/scripts/preflight.sh
  echo "gating commit ${REV:0:9} in worktree $WT"
  (cd "$WT" && "$gate" ${PASSTHRU[@]:+"${PASSTHRU[@]}"})
  exit
fi

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
  RED=$'\e[31m' GREEN=$'\e[32m' YELLOW=$'\e[33m' BOLD=$'\e[1m' DIM=$'\e[2m' RESET=$'\e[0m'
else
  RED='' GREEN='' YELLOW='' BOLD='' DIM='' RESET=''
fi
NAMES=() STATES=() NOTES=()
FAILED=0
START=$SECONDS

record() { NAMES+=("$2"); STATES+=("$1"); NOTES+=("${3:-}"); [[ $1 != FAIL ]] || FAILED=$((FAILED + 1)); }
skip() { record SKIP "$1" "$2"; }
fail() { record FAIL "$1" "$2"; printf '%sFAIL%s  %s\n      %s\n' "$RED" "$RESET" "$1" "$2"; }
in_dir() { local dir=$1; shift; (cd "$dir" && "$@"); }
changed() { grep -qE "$1" "$CHANGED"; }

# step NAME CMD...: stream output, record PASS/FAIL, re-print the tail on failure.
step() {
  local name=$1 log started=$SECONDS
  shift
  log=$(mktemp)
  printf '\n%s▶ %s%s\n%s$ %s%s\n' "$BOLD" "$name" "$RESET" "$DIM" "$*" "$RESET"
  if "$@" 2>&1 | tee "$log"; then
    record PASS "$name" "$((SECONDS - started))s"
    printf '%sPASS%s  %s (%ss)\n' "$GREEN" "$RESET" "$name" "$((SECONDS - started))"
  else
    record FAIL "$name" "$((SECONDS - started))s"
    printf '%sFAIL%s  %s (%ss) — last lines:\n' "$RED" "$RESET" "$name" "$((SECONDS - started))"
    tail -n 40 "$log" | sed 's/^/      /'
  fi
  rm -f "$log"
}

need() { # need TOOL "install hint" -> 0 when present, else records a FAIL
  command -v "$1" >/dev/null 2>&1 && return 0
  fail "tool: $1" "missing. Install: $2"
  return 1
}

# ---------------------------------------------------------------- changes ---
BASE_REF=${PREFLIGHT_BASE:-origin/main}
git rev-parse -q --verify "$BASE_REF^{commit}" >/dev/null ||
  { echo "base $BASE_REF not found; run: git fetch origin main" >&2; exit 2; }
BASE=$(git merge-base "$BASE_REF" HEAD)
CHANGED=$(mktemp)
trap 'rm -f "$CHANGED"' EXIT
{ git diff --name-only "$BASE"; git ls-files --others --exclude-standard; } | sort -u >"$CHANGED"

rust=false runtime=false docs=false python_sdk=false sdk_drift=false snapshot_ci=false
if [[ $ALL == true ]]; then
  rust=true runtime=true docs=true python_sdk=true sdk_drift=true
else
  classes=$(mktemp)
  .github/scripts/classify-rust-changes.sh "$classes" <"$CHANGED"
  bash .github/scripts/classify-runtime-changes.sh "$classes" <"$CHANGED" >/dev/null
  # shellcheck disable=SC1090
  source "$classes"
  rm -f "$classes"
fi
printf '%s%d changed path(s) vs %s (%s)%s\n' "$DIM" "$(wc -l <"$CHANGED" | tr -d ' ')" "$BASE_REF" "${BASE:0:9}" "$RESET"
printf '%srust=%s runtime=%s docs=%s python_sdk=%s sdk_drift=%s fast=%s all=%s%s\n' \
  "$DIM" "$rust" "$runtime" "$docs" "$python_sdk" "$sdk_drift" "$FAST" "$ALL" "$RESET"

# ------------------------------------------------------------ cheap gates ---
# Verify who the push credential belongs to, not what the URL says. The hook
# passes the destination remote and URL; token and login never reach argv or
# stdout. Fails closed when no credential can be resolved.
push_actor_check() {
  local want=${PREFLIGHT_PUSH_ACTOR:-unarbos} remote=${PREFLIGHT_PUSH_REMOTE:-origin} url userinfo token='' login=''
  url=${PREFLIGHT_PUSH_URL:-$(git remote get-url --push "$remote")}
  if [[ "$url" =~ ^(ssh://)?git@github\.com[:/] ]]; then
    login=$(ssh -o BatchMode=yes -T git@github.com 2>&1 | sed -n 's/^Hi \([^!]*\)!.*/\1/p')
  else
    if [[ "$url" =~ ^https?://([^@/]+)@ ]]; then
      userinfo=${BASH_REMATCH[1]}
      [[ "$userinfo" != *:* ]] || token=${userinfo#*:}
    fi
    [[ -n "$token" ]] || token=$(printf 'url=%s\n' "$url" |
      GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true git credential fill 2>/dev/null | sed -n 's/^password=//p')
    [[ -n "$token" ]] || { echo "no credential resolvable for remote '$remote' (${url%%:*}://…); cannot verify the push actor"; }
    [[ -z "$token" ]] || login=$(printf 'header = "Authorization: Bearer %s"\n' "$token" |
      curl -sS -m 20 -K - https://api.github.com/user | jq -r '.login // empty')
  fi
  if [[ "$login" != "$want" ]]; then
    echo "push credential for '$remote' belongs to '${login:-nobody}', expected '$want'"
    echo "fix: git remote set-url --push $remote \"https://${want}:\$(op read 'op://Arbos/vvnyarkwampjl3diocn7n6vcqe/credential')@github.com/RaoFoundation/subtensor.git\""
    return 1
  fi
  echo "push actor: $login (credential owner verified via api.github.com/user)"
}
step "push actor is ${PREFLIGHT_PUSH_ACTOR:-unarbos}" push_actor_check

step "git diff --check (whitespace)" git diff --check "$BASE"

untracked_generated() {
  local hits
  hits=$(git ls-files --others --exclude-standard -- \
    sdk/python/bittensor/_generated docs/tx docs/query docs/errors \
    website/apps/bittensor-website/public/catalog)
  [[ -z "$hits" ]] || { echo "untracked generated files (commit or delete them):"; echo "$hits"; return 1; }
  echo "no untracked generated files"
}
step "no untracked generated files" untracked_generated

fmt_check() { # CI also treats a rustfmt ICE that exits 0 as a failure
  local out status=0
  out=$(cargo fmt --check --all 2>&1) || status=$?
  out=$(grep -v "can't set \`imports_granularity" <<<"$out" || true) # harmless vendor/frontier noise
  grep -qiE "panicked at|internal compiler error" <<<"$out" && status=1
  [[ -z "$out" ]] || echo "$out"
  (( status == 0 )) && echo "rustfmt clean"
  return "$status"
}
if [[ $rust == true ]]; then
  step "cargo fmt --check --all" fmt_check
  step "rust CI path ownership" .github/scripts/test-rust-ci-paths.sh
  if need zepter "cargo install --locked zepter"; then
    step "zepter feature propagation" zepter run check
  fi
else
  skip "cargo fmt / zepter" "no Rust paths changed"
fi

SDK=sdk/python
uv_ready() {
  need uv "curl -LsSf https://astral.sh/uv/0.11.28/install.sh | sh" || return 1
  [[ -d ${UV_PROJECT_ENVIRONMENT:-$SDK/.venv} ]] && return 0
  fail "sdk/python locked env" "missing. Run: (cd $SDK && uv sync --python 3.14 --locked --all-extras --dev)"
  return 1
}
if [[ $runtime == true || $python_sdk == true || $docs == true ]]; then
  if uv_ready; then
    step "ruff check (sdk/python)" in_dir $SDK uv run --no-sync ruff check .
    step "ruff format --check (sdk/python)" in_dir $SDK uv run --no-sync ruff format --check .
    step "codegen.check --coverage" in_dir $SDK uv run --no-sync python -m codegen.check --coverage
    step "codegen.check --names" in_dir $SDK uv run --no-sync python -m codegen.check --names
    step "beta baseline table in sync" in_dir $SDK uv run --no-sync python scripts/export_beta_baselines_rs.py --check
    # CI only runs this when docs=true, so Rust-only PRs leak drift into the
    # next docs PR. Run it here whenever a generator input may have moved.
    step "generated docs drift (generate.py --check)" in_dir $SDK \
      uv run --no-sync python ../../website/apps/bittensor-website/scripts/generate.py --check
  fi
else
  skip "sdk/python + docs drift" "no runtime, sdk/python, or docs paths changed"
fi

if [[ $ALL == true ]] || changed '^ts-tests/'; then
  if [[ -d ts-tests/node_modules ]]; then
    step "pnpm run fmt (ts-tests)" in_dir ts-tests pnpm run fmt
  else
    fail "ts-tests locked env" "missing. Run: (cd ts-tests && pnpm install --frozen-lockfile)"
  fi
else
  skip "pnpm run fmt (ts-tests)" "no ts-tests paths changed"
fi

# ------------------------------------------------------------ heavy gates ---
summary() {
  local i total=$(( SECONDS - START ))
  printf '\n%s──── preflight summary (%s mode, %dm%02ds) ────%s\n' "$BOLD" \
    "$([[ $FAST == true ]] && echo fast || echo full)" $((total / 60)) $((total % 60)) "$RESET"
  for i in "${!NAMES[@]}"; do
    case "${STATES[$i]}" in
      PASS) printf '%sPASS%s  %-48s %s\n' "$GREEN" "$RESET" "${NAMES[$i]}" "${NOTES[$i]}" ;;
      FAIL) printf '%sFAIL%s  %-48s %s\n' "$RED" "$RESET" "${NAMES[$i]}" "${NOTES[$i]}" ;;
      SKIP) printf '%sSKIP%s  %-48s %s%s%s\n' "$YELLOW" "$RESET" "${NAMES[$i]}" "$DIM" "${NOTES[$i]}" "$RESET" ;;
    esac
  done
  echo
  git status --short | head -n 30
  if (( FAILED > 0 )); then
    printf '%s%d gate(s) failed. Fix them before pushing. git push --no-verify is forbidden.%s\n' "$RED" "$FAILED" "$RESET"
    exit 1
  fi
  printf '%sAll gates passed.%s\n' "$GREEN" "$RESET"
}

if (( FAILED > 0 )); then
  skip "compile-heavy gates" "not run while cheap gates are red"
  summary
fi

if [[ $rust == true ]]; then
  step "cargo clippy (default)" cargo clippy --workspace --all-targets -- -D warnings
  step "cargo clippy (all)" cargo clippy --workspace --all-targets --all-features -- -D warnings
  crates=()
  if [[ $ALL == true ]] || changed '^(Cargo\.toml|Cargo\.lock|rust-toolchain\.toml)$'; then
    step "cargo test --workspace --all-features" cargo test --workspace --all-features
  else
    while IFS=$'\t' read -r name manifest; do
      dir=${manifest%/Cargo.toml}
      dir=${dir#"$ROOT"/}
      [[ "$dir" != "$ROOT" ]] && changed "^${dir}/" && crates+=(-p "$name")
    done < <(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | "\(.name)\t\(.manifest_path)"')
    if (( ${#crates[@]} > 0 )); then
      step "cargo test (changed crates)" cargo test --all-features "${crates[@]}"
    else
      skip "cargo test (changed crates)" "no workspace crate changed"
    fi
    if changed '^(pallets|runtime)/' && [[ " ${crates[*]:-} " != *" node-subtensor-runtime "* ]]; then
      step "runtime fee_baseline + claim_root_weight tests" \
        cargo test --all-features -p node-subtensor-runtime --test fee_baseline --test claim_root_weight
    fi
  fi
else
  skip "cargo clippy / cargo test" "no Rust paths changed"
fi

# SDK bindings drift: needs a node built from this tree. Trigger on a
# spec_version change or any production Rust path that shapes metadata.
metadata_surface_changed() {
  git diff "$BASE" -- runtime/src/lib.rs | grep -qE '^[+-][[:space:]]*spec_version:' && return 0
  grep -E '^(pallets/[^/]+/src/|runtime/src/|common/src/|primitives/)' "$CHANGED" |
    grep -vqE '/(tests?|mock|benchmarking|benchmarks|weights)(/|\.rs$)'
}
NODE_PORT=${PREFLIGHT_RPC_PORT:-9977}
NODE_PIDFILE=$(mktemp)
stop_node() { # the step runs in a pipeline subshell, so the PID travels via a file
  local pid; pid=$(cat "$NODE_PIDFILE" 2>/dev/null || true)
  [[ -z "$pid" ]] || { kill "$pid" 2>/dev/null || true; : >"$NODE_PIDFILE"; }
}
trap 'stop_node; rm -f "$CHANGED" "$NODE_PIDFILE"' EXIT
sdk_regen_check() {
  local want got='' i pid
  want=$(grep -oE '^[[:space:]]*spec_version: [0-9]+,' runtime/src/lib.rs | grep -oE '[0-9]+')
  target/release/node-subtensor --chain local --tmp --alice --validator --rpc-port "$NODE_PORT" \
    --rpc-cors all --rpc-methods unsafe --unsafe-force-node-key-generation >/tmp/preflight-node.log 2>&1 &
  pid=$!
  echo "$pid" >"$NODE_PIDFILE"
  for i in $(seq 1 60); do
    got=$(curl -sS -m 5 -H 'Content-Type: application/json' \
      -d '{"id":1,"jsonrpc":"2.0","method":"state_getRuntimeVersion","params":[]}' \
      "http://127.0.0.1:$NODE_PORT" 2>/dev/null | jq -r '.result.specVersion // empty') && [[ -n "$got" ]] && break
    kill -0 "$pid" 2>/dev/null || { tail -n 20 /tmp/preflight-node.log; echo "node exited"; return 1; }
    sleep 2
  done
  [[ "$got" == "$want" ]] || { echo "node serves spec_version ${got:-none}, tree has $want (stale binary?)"; return 1; }
  echo "node up on :$NODE_PORT with spec_version $got"
  if ! in_dir $SDK uv run --no-sync python -m codegen.check --drift "ws://127.0.0.1:$NODE_PORT"; then
    echo "regenerating sdk/python/bittensor/_generated from this node ..."
    in_dir $SDK uv run --no-sync python -m codegen "ws://127.0.0.1:$NODE_PORT"
    echo "DRIFT — regenerated files (review, then commit them):"
    git status --porcelain -- sdk/python/bittensor/_generated
    return 1
  fi
  echo "committed bindings match this runtime's metadata"
}
if [[ $sdk_drift == true ]] && { [[ $ALL == true ]] || metadata_surface_changed; }; then
  if [[ $FAST == true ]]; then
    skip "SDK bindings drift (release node)" "--fast; run without --fast before pushing runtime changes"
  elif uv_ready; then
    step "cargo build --release -p node-subtensor" env -u SKIP_WASM_BUILD cargo build --release -p node-subtensor
    step "SDK bindings drift (codegen.check --drift)" sdk_regen_check
    stop_node
  fi
else
  skip "SDK bindings drift (release node)" "no spec_version / metadata surface change"
fi

# try-runtime: replay on_runtime_upgrade against the nightly mainnet snapshot
# CI uses, when a migration or the runtime Migrations tuple changed.
TRY_RUNTIME_VERSION=0.10.1
migrations_changed() {
  changed '^(pallets|runtime)/.*/migrations/' && return 0
  [[ "$(git show "$BASE:runtime/src/lib.rs" | awk '/^type Migrations = \(/,/^\);/')" != \
     "$(awk '/^type Migrations = \(/,/^\);/' runtime/src/lib.rs)" ]]
}
fetch_snapshot() {
  local cache=${PREFLIGHT_CACHE_DIR:-$HOME/.cache/subtensor-preflight} repo=${PREFLIGHT_REPO:-RaoFoundation/subtensor} id
  mkdir -p "$cache"
  if [[ -s $cache/mainnet.snap && -n "$(find "$cache/mainnet.snap" -mmin -$((72 * 60)))" ]]; then return 0; fi
  id=$(gh api "repos/$repo/actions/artifacts?name=try-runtime-snap-v$TRY_RUNTIME_VERSION-mainnet&per_page=10" \
    --jq '[.artifacts[] | select(.expired == false and .workflow_run.head_branch == "main")] | sort_by(.created_at) | last | .id')
  [[ -n "$id" && "$id" != null ]] || { echo "no try-runtime mainnet snapshot artifact found" >&2; return 1; }
  echo "downloading try-runtime snapshot artifact $id ..." >&2
  gh api "repos/$repo/actions/artifacts/$id/zip" >"$cache/mainnet.zip"
  unzip -oq "$cache/mainnet.zip" -d "$cache" && rm -f "$cache/mainnet.zip"
  [[ -s $cache/mainnet.snap ]]
}
try_runtime_check() {
  local wasm=target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm mbm=()
  local cache=${PREFLIGHT_CACHE_DIR:-$HOME/.cache/subtensor-preflight}
  [[ "$(grep -cE '^[[:space:]]*type MultiBlockMigrator[[:space:]]*=' runtime/src/lib.rs)" == 1 ]] ||
    { echo "expected exactly one MultiBlockMigrator definition"; return 1; }
  grep -qE '^[[:space:]]*type MultiBlockMigrator[[:space:]]*=[[:space:]]*\(\)[[:space:]]*;' runtime/src/lib.rs && mbm=(--disable-mbm-checks)
  fetch_snapshot || return 1
  RUST_LOG=remote-ext=debug,runtime=debug try-runtime --runtime "$wasm" on-runtime-upgrade \
    --checks=all --blocktime 12000 --disable-spec-version-check --no-weight-warnings \
    ${mbm[@]:+"${mbm[@]}"} snap --path "$cache/mainnet.snap"
}
if [[ $runtime == true ]] && { [[ $ALL == true ]] || migrations_changed; }; then
  if [[ $FAST == true ]]; then
    skip "try-runtime on-runtime-upgrade (mainnet)" "--fast; run without --fast before pushing migrations"
  elif need try-runtime "curl -sSfL -o ~/.local/bin/try-runtime https://github.com/paritytech/try-runtime-cli/releases/download/v$TRY_RUNTIME_VERSION/try-runtime-x86_64-unknown-linux-musl && chmod +x ~/.local/bin/try-runtime" &&
       need gh "https://cli.github.com (needed to fetch the nightly snapshot artifact)"; then
    step "build try-runtime wasm (production)" env -u SKIP_WASM_BUILD \
      cargo build --profile production -p node-subtensor-runtime --features try-runtime -q --locked
    step "try-runtime on-runtime-upgrade (mainnet snapshot)" try_runtime_check
  fi
else
  skip "try-runtime on-runtime-upgrade (mainnet)" "no migration change"
fi

summary
