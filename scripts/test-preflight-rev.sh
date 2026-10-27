#!/usr/bin/env bash
# Regression for the --rev path of scripts/preflight.sh: when the gate runs a
# pushed commit in a detached worktree it must resolve build artifacts from
# Cargo's real target dir (the one CARGO_TARGET_DIR points the worktree at),
# not from the worktree's own ./target, or a metadata-changing push fails
# with "node exited" right after a successful release build.
#
# Builds a throwaway commit with a metadata-surface change (no checkout
# changes), runs the gate on it with --print-artifacts, and asserts the
# resolved paths and the gate plan. The release node is not built here.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
ROOT=$PWD
fail() { echo "FAIL: $*" >&2; exit 1; }
field() { sed -n "s/^$1=//p" <<<"$2"; }

# Throwaway commit on top of HEAD that touches a pallet source file.
blob=$(printf '%s\n// preflight --rev regression: metadata surface touched\n' \
  "$(git show HEAD:pallets/subtensor/src/lib.rs)" | git hash-object -w --stdin)
index=$(mktemp -u)
GIT_INDEX_FILE=$index git read-tree HEAD
GIT_INDEX_FILE=$index git update-index --cacheinfo "100644,$blob,pallets/subtensor/src/lib.rs"
tree=$(GIT_INDEX_FILE=$index git write-tree)
rm -f "$index"
sha=$(git commit-tree "$tree" -p HEAD -m "scratch: preflight --rev regression")
echo "scratch commit ${sha:0:9}"

expected_target=$(cargo metadata --no-deps --format-version 1 | jq -r .target_directory)

# 1. Default: the worktree borrows this checkout's target dir.
out=$(scripts/preflight.sh --rev "$sha" --print-artifacts)
echo "$out"
wt_root=$(field root "$out")
[[ "$wt_root" != "$ROOT" && -n "$wt_root" ]] || fail "gate did not run in a worktree (root=$wt_root)"
[[ "$(field target_dir "$out")" == "$expected_target" ]] || fail "target_dir != $expected_target"
[[ "$(field node "$out")" == "$expected_target/release/node-subtensor" ]] || fail "node path not under Cargo's target dir"
[[ "$(field wasm "$out")" == "$expected_target/production/wbuild/"* ]] || fail "wasm path not under Cargo's target dir"
[[ "$(field node "$out")" != "$wt_root/"* ]] || fail "node path resolved inside the worktree"
[[ "$(field sdk_drift_gate "$out")" == true ]] || fail "metadata change did not select the SDK drift gate"
[[ "$(field try_runtime_gate "$out")" == false ]] || fail "no migration change, yet try-runtime selected"
[[ ! -d "$wt_root" ]] || fail "worktree $wt_root was not removed"

# 2. An explicit CARGO_TARGET_DIR wins, in the worktree too.
override=$(mktemp -d)
out=$(CARGO_TARGET_DIR=$override scripts/preflight.sh --rev "$sha" --print-artifacts)
[[ "$(field node "$out")" == "$override/release/node-subtensor" ]] || fail "CARGO_TARGET_DIR override ignored"
rm -rf "$override"

# 3. Same commit, no worktree: paths come from cargo metadata as well.
out=$(scripts/preflight.sh --print-artifacts)
[[ "$(field root "$out")" == "$ROOT" && "$(field target_dir "$out")" == "$expected_target" ]] || fail "in-place resolution differs"

echo "PASS: --rev resolves node/wasm artifacts from Cargo's target dir"
