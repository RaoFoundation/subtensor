#!/usr/bin/env bash
# Prints the runtime identity of mainnet's latest finalized block as
# key=value lines: finalized_head, spec_version, code_hash.
#
# With EXPECTED_SPEC and EXPECTED_CODE_HASH set, fails unless mainnet still
# runs exactly that runtime. Queued or approval-gated jobs can start long
# after the chain state that scheduled them was read.

set -euo pipefail

: "${MAINNET_HTTP:?MAINNET_HTTP is required}"

rpc() {
  jq -cn --arg method "$1" --argjson params "$2" \
    '{id: 1, jsonrpc: "2.0", method: $method, params: $params}' \
    | curl -sf -H "Content-Type: application/json" -d @- "$MAINNET_HTTP"
}

finalized_head=$(rpc chain_getFinalizedHead '[]' | jq -er \
  '.result | strings | select(test("^0x[0-9a-f]{64}$"))')
spec_version=$(rpc state_getRuntimeVersion \
  "$(jq -cn --arg block "$finalized_head" '[$block]')" \
  | jq -er '.result.specVersion | numbers')
code_hash=$(rpc state_getStorageHash \
  "$(jq -cn --arg block "$finalized_head" '["0x3a636f6465", $block]')" \
  | jq -er '.result | strings | select(test("^0x[0-9a-f]{64}$"))')

if [[ -n "${EXPECTED_SPEC:-}" || -n "${EXPECTED_CODE_HASH:-}" ]]; then
  [[ "$spec_version" == "${EXPECTED_SPEC:?EXPECTED_SPEC is required}" ]] || {
    echo "mainnet runs spec $spec_version; expected $EXPECTED_SPEC" >&2
    exit 1
  }
  [[ "$code_hash" == "${EXPECTED_CODE_HASH:?EXPECTED_CODE_HASH is required}" ]] || {
    echo "mainnet code hash is $code_hash; expected $EXPECTED_CODE_HASH" >&2
    exit 1
  }
fi

printf 'finalized_head=%s\nspec_version=%s\ncode_hash=%s\n' \
  "$finalized_head" "$spec_version" "$code_hash"
