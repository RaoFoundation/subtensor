#!/usr/bin/env bash
# Walking-skeleton ping (gate G1): lock 1 test USDC through the portal with a
# CreditTUsd envelope and watch it traverse
#   anvil -> validator/relayer agents -> Bittensor mailbox -> Gateway -> runtime.
# Routes through portal.deposit() so the portal's sequential nonce counter
# stays in sync with the hub's NextNonce (a direct mailbox dispatch would
# desync them and wedge the rig).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
# shellcheck source=env.sh
source "$SCRIPT_DIR/env.sh"

[ -f "$RAILS_MANIFEST" ] || { echo "[rails] no manifest; run up.sh first" >&2; exit 1; }

portal="$(jq -r '.chains.baselocal.portal' "$RAILS_MANIFEST")"
usdc="$(jq -r '.chains.baselocal.mockUsdc' "$RAILS_MANIFEST")"

# Alice's well-known sr25519 public key as the destination account.
ALICE_PUB="0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d"
AMOUNT=1000000000 # 1 USDC (9 decimals)

envelope_prefix="$(cd "$RAILS_BASE_DIR/ts-tests" \
    && pnpm exec tsx scripts/rails-envelope.ts credit 0 "$AMOUNT" "$ALICE_PUB")"
echo "[rails] envelope prefix: $envelope_prefix"

echo "[rails] minting + approving 1 test USDC ..."
cast send --rpc-url "$BASE_RPC_HTTP" --private-key "$DEPLOYER_PK" \
    "$usdc" "mint(address,uint256)" "$DEPLOYER_ADDR" "$AMOUNT" >/dev/null
cast send --rpc-url "$BASE_RPC_HTTP" --private-key "$DEPLOYER_PK" \
    "$usdc" "approve(address,uint256)" "$portal" "$AMOUNT" >/dev/null

echo "[rails] dispatching ping deposit through the portal ..."
tx_out="$(cast send --rpc-url "$BASE_RPC_HTTP" --private-key "$DEPLOYER_PK" --json \
    "$portal" "deposit(uint64,bytes)" "$AMOUNT" "$envelope_prefix")"
echo "$tx_out" | jq -r '"[rails] origin tx: " + .transactionHash'

# Deposited(address indexed from, uint64 amount, uint64 indexed nonce,
# bytes32 messageId): the portal-assigned nonce is the second indexed topic.
deposited_topic="$(cast keccak "Deposited(address,uint64,uint64,bytes32)")"
nonce_hex="$(echo "$tx_out" \
    | jq -r --arg t "$deposited_topic" \
        '[.logs[] | select(.topics[0] == $t)][0].topics[2]')"
nonce="$((nonce_hex))"
echo "[rails] portal-assigned nonce: $nonce"

echo "[rails] waiting for runtime execution ..."
for i in $(seq 1 90); do
    processed="$(cd "$RAILS_BASE_DIR/ts-tests" \
        && pnpm exec tsx scripts/rails-check-nonce.ts "$nonce")"
    case "$processed" in
        *block*)
            echo "[rails] $processed"
            echo "[rails] G1 PASS: envelope processed by runtime in ~${i} polls"
            exit 0
            ;;
    esac
    sleep 2
done

echo "[rails] G1 FAIL: nonce $nonce not processed within 180s" >&2
echo "[rails] agent logs:" >&2
"$SCRIPT_DIR/agents.sh" logs >&2
exit 1
