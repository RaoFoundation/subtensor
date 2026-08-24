#!/bin/bash

# The genesis is not allowed to change. Since the wasm genesis will change
# depending on the system architecture used, we need to extract the genesis from
# the old chain specs and insert them into the new chain specs to ensure there
# are no genesis mismatch issues.

# This script updates the chain spec files while keeping genesis and trusted light-sync
# checkpoints unchanged.

set -e

raw_finney="chainspecs/raw_spec_finney.json"
plain_finney="chainspecs/plain_spec_finney.json"
raw_testfinney="chainspecs/raw_spec_testfinney.json"
plain_testfinney="chainspecs/plain_spec_testfinney.json"
raw_devnet="chainspecs/raw_spec_devnet.json"
plain_devnet="chainspecs/plain_spec_devnet.json"

save_genesis() {
  jq -r ".genesis" "$1" >"$2"
}

save_light_sync_state() {
  jq ".lightSyncState" "$1" >"$2"
}

buildspec() {
  local chain="$1"
  shift
  ./target/debug/node-subtensor build-spec --chain "$chain" --disable-default-bootnode "$@"
}

# Restore fields that are sourced from the live chain rather than the static config builders.
restore_preserved_fields() {
  jq --slurpfile genesis "$1" --slurpfile light_sync_state "$2" \
    '.genesis = $genesis[0] | .lightSyncState = $light_sync_state[0]' \
    "$3" >"$4"
}

update_spec() {
  local chain="$1"
  local raw_path="$2"
  local plain_path="$3"

  raw_genesis_temp=$(mktemp)
  plain_genesis_temp=$(mktemp)
  raw_light_sync_state_temp=$(mktemp)
  plain_light_sync_state_temp=$(mktemp)
  raw_spec_temp=$(mktemp)
  plain_spec_temp=$(mktemp)

  echo "*** Backing up genesis for '$chain'..."

  save_genesis "$raw_path" "$raw_genesis_temp"
  save_genesis "$plain_path" "$plain_genesis_temp"
  save_light_sync_state "$raw_path" "$raw_light_sync_state_temp"
  save_light_sync_state "$plain_path" "$plain_light_sync_state_temp"

  echo "*** Building new chainspec for '$chain'..."

  # Build new chainspecs
  buildspec "$chain" >"$plain_spec_temp"
  buildspec "$chain" --raw >"$raw_spec_temp"

  echo "*** Restoring genesis in '$chain'..."

  restore_preserved_fields "$raw_genesis_temp" "$raw_light_sync_state_temp" \
    "$raw_spec_temp" "$raw_path"
  restore_preserved_fields "$plain_genesis_temp" "$plain_light_sync_state_temp" \
    "$plain_spec_temp" "$plain_path"

  # cleanup
  rm -f "$raw_genesis_temp" "$plain_genesis_temp" \
    "$raw_light_sync_state_temp" "$plain_light_sync_state_temp" \
    "$raw_spec_temp" "$plain_spec_temp"
}

# SCRIPT

echo "*** Building node..."
cargo build -p node-subtensor

update_spec finney "$raw_finney" "$plain_finney"
update_spec test_finney "$raw_testfinney" "$plain_testfinney"
update_spec devnet "$raw_devnet" "$plain_devnet"

echo "*** Done!"
