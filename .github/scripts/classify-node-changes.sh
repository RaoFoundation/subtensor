#!/usr/bin/env bash
# Decide whether a pull request may run its E2E suites on the base commit's
# prebuilt node binary with the pull request's runtime wasm, instead of
# building a new node. Reads changed paths (one per line) on stdin and appends
# `node_reuse=true|false` to OUTPUT_FILE.
#
# Reuse is allowed only when every changed path is runtime code (compiled into
# the wasm the job still builds) or is not compiled into the node at all.
# Anything else fails closed to a full node build, including every manifest,
# lockfile, build script, toolchain setting, CI file, and unknown root.
#
# Runtime-side crates the node also links natively are covered as follows:
# - Runtime API traits (`pallets/*/runtime-api`) and the custom RPC crates
#   (`pallets/*/rpc`) are node code: any change forces a full build.
# - Types that runtime API methods take or return may live anywhere in the
#   runtime crates. verify-base-node-runtime.py compares every runtime API
#   method signature of the base and PR wasm and rejects reuse on any change.
# - `pallet_drand::KEY_TYPE` is read natively by the node's offchain keystore
#   setup, so `pallets/drand` is node code.
# - The staged BABE constants (`BABE_GENESIS_EPOCH_CONFIG`,
#   `EPOCH_DURATION_IN_SLOTS`) are read natively only after the runtime
#   reports BABE authorities; the verifier requires `BabeApi_configuration`
#   to be unchanged.
# - `TransactionConverter` is used only when `ConvertTransactionRuntimeApi`
#   is missing; the verifier requires every runtime API version to match.
# - `EXISTENTIAL_DEPOSIT` and `pallet_subtensor` are used natively only by the
#   `benchmark` subcommand, which no E2E suite runs.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: classify-node-changes.sh OUTPUT_FILE" >&2
  exit 2
fi

output_file="$1"
node_reuse=true
reason=

while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  case "$path" in
    pallets/*/rpc/*|pallets/*/runtime-api/*|pallets/drand/*)
      verdict=node ;;
    */Cargo.toml|*/build.rs)
      verdict=node ;;
    pallets/*|runtime/*|precompiles/*|chain-extensions/*|primitives/*)
      verdict=runtime ;;
    *.md|LICENSE|docs/*|website/*|sdk/*|ts-tests/*|eco-tests/*|clones/*|ink-contract/*|.maintain/*|.vscode/*|.agents/*|.claude/*)
      verdict=unlinked ;;
    *)
      verdict=node ;;
  esac
  if [[ "$verdict" == node && "$node_reuse" == true ]]; then
    node_reuse=false
    reason="$path"
  fi
done

if [[ "$node_reuse" == true ]]; then
  echo "node_reuse=true: no changed path is compiled into the node natively"
else
  echo "node_reuse=false: $reason can change the node binary"
fi
echo "node_reuse=$node_reuse" >> "$output_file"
