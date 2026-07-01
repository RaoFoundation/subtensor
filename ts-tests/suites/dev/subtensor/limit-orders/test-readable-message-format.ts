import { describeSuite, expect } from "@moonwall/cli";
import { Keyring } from "@polkadot/keyring";
import { encodeAddress } from "@polkadot/util-crypto";
import {
    type Order,
    type OrderType,
    formatOrderMessage,
    READABLE_SS58_PREFIX,
} from "../../../../utils/limit-orders.js";

// Byte-parity anchor for the canonical human-readable ("clear-signing") message.
//
// The runtime rebuilds this exact string in `render_order` and verifies the
// signature over `<Bytes>` ++ utf8(message) ++ `</Bytes>`.  If the TS formatter
// drifts from the Rust one by a single byte, every readable-signed order breaks.
// These assertions pin the TS output against FULLY HARDCODED literals derived
// with a well-known dev key (sr25519 `//Alice` at prefix 42 is the canonical
// `5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY`), so the expected string is
// NOT derived from the same `encodeAddress` call it is testing.
//
// The field VALUES mirror the Rust golden vectors in
// `pallets/limit-orders/src/tests/readable.rs` so both suites pin identical
// output.

// Known dev keys, sr25519, rendered at prefix 42.
const KR = new Keyring({ type: "sr25519" });
const ALICE = KR.addFromUri("//Alice").address; // 5Grwva...
const BOB = KR.addFromUri("//Bob").address; // 5FHneW...
const CHARLIE = KR.addFromUri("//Charlie").address; // 5FLSig...
const DAVE = KR.addFromUri("//Dave").address; // 5DAAnr...

// Hardcoded SS58 (prefix 42) of the dev keys — independent of the formatter.
const ALICE_SS58 = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
const BOB_SS58 = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";
const CHARLIE_SS58 = "5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y";
const DAVE_SS58 = "5DAAnrj7VHTznn2AWBemMuyBwZWs6FNFjdyVXUeYum3PTXFy";

function makeOrder(overrides: Partial<Order>): Order {
    return {
        signer: ALICE,
        hotkey: BOB,
        netuid: 7,
        order_type: "LimitBuy" as OrderType,
        amount: 1_234_567n,
        limit_price: 2_000_000_000n,
        expiry: 9_999_999n,
        fee_rate: 5_000_000,
        fee_recipient: DAVE,
        relayer: null,
        max_slippage: null,
        chain_id: 945n,
        partial_fills_enabled: false,
        ...overrides,
    };
}

function assertAllPrintableAscii(s: string): void {
    for (let i = 0; i < s.length; i++) {
        const code = s.charCodeAt(i);
        expect(
            code >= 0x20 && code <= 0x7e,
            `char ${i} = 0x${code.toString(16)} (${JSON.stringify(s[i])}) is not printable ASCII`
        ).toBe(true);
    }
}

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_READABLE_FORMAT",
    title: "limit-orders — canonical human-readable message formatter parity",
    foundationMethods: "dev",
    testCases: ({ it }) => {
        it({
            id: "T01",
            title: "the well-known //Alice SS58 anchor is prefix 42",
            test: () => {
                // Sanity: the dev keyring already renders at prefix 42, and an
                // explicit re-encode is idempotent — so both routes must equal
                // the hardcoded literal.
                expect(ALICE).toBe(ALICE_SS58);
                expect(encodeAddress(ALICE, READABLE_SS58_PREFIX)).toBe(ALICE_SS58);
            },
        });

        it({
            id: "T02",
            title: "LimitBuy with relayer none renders the exact golden string",
            test: () => {
                const msg = formatOrderMessage(makeOrder({}));
                const expected =
                    "TAO.com order v1: Limit buy 1234567 on subnet 7, " +
                    "limit price 2000000000, expiry 9999999, " +
                    `hotkey ${BOB_SS58}, ` +
                    `fee 5000000 to ${DAVE_SS58}, ` +
                    "relayer none, max slippage none, chain 945, " +
                    `partial fills false, signer ${ALICE_SS58}`;
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });

        it({
            id: "T03",
            title: "StopLoss with max_slippage renders Stop-loss / trigger price",
            test: () => {
                const msg = formatOrderMessage(
                    makeOrder({
                        signer: CHARLIE,
                        hotkey: DAVE,
                        netuid: 2,
                        order_type: "StopLoss",
                        amount: 500n,
                        limit_price: 750_000_000n,
                        expiry: 42n,
                        fee_rate: 0,
                        fee_recipient: ALICE,
                        relayer: null,
                        max_slippage: 10_000_000, // 1% in ppb
                        partial_fills_enabled: true,
                    })
                );
                const expected =
                    "TAO.com order v1: Stop-loss 500 on subnet 2, " +
                    "trigger price 750000000, expiry 42, " +
                    `hotkey ${DAVE_SS58}, ` +
                    `fee 0 to ${ALICE_SS58}, ` +
                    "relayer none, max slippage 10000000, chain 945, " +
                    `partial fills true, signer ${CHARLIE_SS58}`;
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });

        it({
            id: "T04",
            title: "TakeProfit with two relayers renders '+'-joined list",
            test: () => {
                const msg = formatOrderMessage(
                    makeOrder({
                        signer: ALICE,
                        hotkey: DAVE,
                        netuid: 1,
                        order_type: "TakeProfit",
                        amount: 88n,
                        limit_price: 1_000_000_000n,
                        expiry: 100_000n,
                        fee_rate: 1,
                        fee_recipient: DAVE,
                        relayer: [BOB, CHARLIE],
                        max_slippage: null,
                        partial_fills_enabled: false,
                    })
                );
                const expected =
                    "TAO.com order v1: Take-profit 88 on subnet 1, " +
                    "trigger price 1000000000, expiry 100000, " +
                    `hotkey ${DAVE_SS58}, ` +
                    `fee 1 to ${DAVE_SS58}, ` +
                    `relayer ${BOB_SS58}+${CHARLIE_SS58}, ` +
                    "max slippage none, chain 945, " +
                    `partial fills false, signer ${ALICE_SS58}`;
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });

        it({
            id: "T05",
            title: "empty relayer array renders '[]' (distinct from none)",
            test: () => {
                const msg = formatOrderMessage(
                    makeOrder({
                        order_type: "LimitBuy",
                        amount: 1_000n,
                        limit_price: 18_446_744_073_709_551_615n, // u64::MAX
                        expiry: 18_446_744_073_709_551_615n, // u64::MAX
                        fee_rate: 0,
                        relayer: [],
                        max_slippage: null,
                    })
                );
                const expected =
                    "TAO.com order v1: Limit buy 1000 on subnet 7, " +
                    "limit price 18446744073709551615, expiry 18446744073709551615, " +
                    `hotkey ${BOB_SS58}, ` +
                    `fee 0 to ${DAVE_SS58}, ` +
                    "relayer [], max slippage none, chain 945, " +
                    `partial fills false, signer ${ALICE_SS58}`;
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });
    },
});
