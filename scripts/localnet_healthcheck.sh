#!/bin/sh

set -eu

rpc_port="${1:-9944}"
response=$(
  curl --fail --silent --show-error --max-time 2 \
    -H 'content-type: application/json' \
    --data '{"id":1,"jsonrpc":"2.0","method":"chain_getHeader","params":[]}' \
    "http://127.0.0.1:${rpc_port}"
)

# A listening RPC endpoint is not enough for CI: require at least one authored
# block so callers do not race the chain during startup.
printf '%s\n' "$response" \
  | grep -Eq '"number":"0x[0-9a-fA-F]*[1-9a-fA-F][0-9a-fA-F]*"'
