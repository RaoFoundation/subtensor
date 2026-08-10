import { describeSuite, expect } from "@moonwall/cli";
import { Keyring } from "@polkadot/keyring";
import {
    type OrderV2,
    type OrderType,
    formatOrderAmount,
    formatOrderMessage,
    formatOrderMessageV2,
} from "../../../../utils/limit-orders.js";

// Byte-parity anchor for the v2 canonical human-readable ("clear-signing") message.
//
// The runtime rebuilds this exact string in `render_order` and verifies the signature
// over `<Bytes>` ++ utf8(message) ++ `</Bytes>`.  If the TS formatter drifts from the
// Rust one by a single byte, every readable-signed v2 order breaks.  As in the v1
// suite, the expected strings are FULLY HARDCODED so they are not derived from the
// same code under test.
//
// v2 differs from v1 in exactly three places, and each gets its own assertion below:
//   1. the `v2` version tag,
//   2. the amount slot, which a linked order fills with its provider and fraction,
//   3. the `, has-linked-order {bool}` tail.
//
// The v1 suite (`test-readable-message-format.ts`) pins the v1 form; T07 here is the
// cross-check that v2 did not disturb it.

const KR = new Keyring({ type: "sr25519" });
const ALICE = KR.addFromUri("//Alice").address;
const BOB = KR.addFromUri("//Bob").address;
const CHARLIE = KR.addFromUri("//Charlie").address;
const DAVE = KR.addFromUri("//Dave").address;

// Hardcoded SS58 (prefix 42) of the dev keys — independent of the formatter.
const ALICE_SS58 = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
const BOB_SS58 = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";
const CHARLIE_SS58 = "5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y";
const DAVE_SS58 = "5DAAnrj7VHTznn2AWBemMuyBwZWs6FNFjdyVXUeYum3PTXFy";

// A fixed 32-byte provider id. Hardcoded rather than derived so the expected string
// below is a literal.
const PROVIDER = `0x${"2c".repeat(32)}` as `0x${string}`;
const PROVIDER_HEX = "2c".repeat(32);

function makeOrderV2(overrides: Partial<OrderV2>): OrderV2 {
    return {
        signer: ALICE,
        hotkey: BOB,
        netuid: 7,
        order_type: "LimitBuy" as OrderType,
        amount: { Fixed: 1_234_567n },
        limit_price: 2_000_000_000n,
        expiry: 9_999_999n,
        fee_rate: 5_000_000,
        fee_recipient: DAVE,
        relayer: null,
        max_slippage: null,
        chain_id: 945n,
        partial_fills_enabled: false,
        has_linked_order: false,
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
    id: "DEV_SUB_LIMIT_ORDERS_V2_READABLE_FORMAT",
    title: "limit-orders v2 — canonical human-readable message formatter parity",
    foundationMethods: "dev",
    testCases: ({ it }) => {
        it({
            id: "T01",
            title: "Fixed amount renders as bare digits; linked renders provider and ppb",
            test: () => {
                expect(formatOrderAmount({ Fixed: 1_000_000_000n })).toBe("1000000000");
                expect(formatOrderAmount({ LinkedPercentage: { provider: PROVIDER, pct: 250_000_000 } })).toBe(
                    `250000000 ppb of order 0x${PROVIDER_HEX} output`
                );
            },
        });

        it({
            id: "T02",
            title: "the two amount variants are injective — neither can render as the other",
            test: () => {
                const fixed = formatOrderAmount({ Fixed: 250_000_000n });
                const linked = formatOrderAmount({ LinkedPercentage: { provider: PROVIDER, pct: 250_000_000 } });

                // Same numeric value, different rendering: the ` ppb of order …` suffix
                // is what a bare amount can never produce, so a fixed-amount signature
                // can never be replayed as a linked one.
                expect(fixed).not.toBe(linked);
                expect(fixed.includes(" ppb of order ")).toBe(false);
                expect(linked.endsWith(" output")).toBe(true);
            },
        });

        it({
            id: "T03",
            title: "provider id renders as 64 lowercase hex chars regardless of input case",
            test: () => {
                const upper = `0x${"AB".repeat(32)}` as `0x${string}`;
                const rendered = formatOrderAmount({ LinkedPercentage: { provider: upper, pct: 1 } });
                expect(rendered).toBe(`1 ppb of order 0x${"ab".repeat(32)} output`);
                // Full 64 characters — a truncated reference would reopen the provider
                // substitution gap the id exists to close.
                expect(rendered.match(/0x([0-9a-f]+) output$/)?.[1].length).toBe(64);
            },
        });

        it({
            id: "T04",
            title: "v2 Fixed order renders the exact golden string with the has-linked-order tail",
            test: () => {
                const msg = formatOrderMessageV2(makeOrderV2({}));
                const expected =
                    "TAO.com order v2: Limit buy 1234567 on subnet 7, " +
                    "limit price 2000000000, expiry 9999999, " +
                    `hotkey ${BOB_SS58}, ` +
                    `fee 5000000 to ${DAVE_SS58}, ` +
                    "relayer none, max slippage none, chain 945, " +
                    `partial fills false, signer ${ALICE_SS58}, ` +
                    "has-linked-order false";
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });

        it({
            id: "T05",
            title: "a provider (has_linked_order = true) renders the tail as true",
            test: () => {
                const msg = formatOrderMessageV2(
                    makeOrderV2({
                        signer: CHARLIE,
                        hotkey: DAVE,
                        netuid: 2,
                        order_type: "TakeProfit",
                        amount: { Fixed: 500n },
                        limit_price: 750_000_000n,
                        expiry: 42n,
                        fee_rate: 0,
                        fee_recipient: ALICE,
                        max_slippage: 10_000_000,
                        has_linked_order: true,
                    })
                );
                const expected =
                    "TAO.com order v2: Take-profit 500 on subnet 2, " +
                    "trigger price 750000000, expiry 42, " +
                    `hotkey ${DAVE_SS58}, ` +
                    `fee 0 to ${ALICE_SS58}, ` +
                    "relayer none, max slippage 10000000, chain 945, " +
                    `partial fills false, signer ${CHARLIE_SS58}, ` +
                    "has-linked-order true";
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });

        it({
            id: "T06",
            title: "a linked buy renders its provider and fraction in the amount slot",
            test: () => {
                const msg = formatOrderMessageV2(
                    makeOrderV2({
                        netuid: 12,
                        amount: { LinkedPercentage: { provider: PROVIDER, pct: 250_000_000 } },
                        relayer: [BOB, CHARLIE],
                        fee_rate: 0,
                        fee_recipient: ALICE,
                    })
                );
                const expected =
                    `TAO.com order v2: Limit buy 250000000 ppb of order 0x${PROVIDER_HEX} output on subnet 12, ` +
                    "limit price 2000000000, expiry 9999999, " +
                    `hotkey ${BOB_SS58}, ` +
                    `fee 0 to ${ALICE_SS58}, ` +
                    `relayer ${BOB_SS58}+${CHARLIE_SS58}, ` +
                    "max slippage none, chain 945, " +
                    `partial fills false, signer ${ALICE_SS58}, ` +
                    "has-linked-order false";
                expect(msg).toBe(expected);
                assertAllPrintableAscii(msg);
            },
        });

        it({
            id: "T07",
            title: "v1 renders no tail and differs from v2 on identical fields",
            test: () => {
                const v2 = makeOrderV2({});
                const v1Msg = formatOrderMessage({
                    signer: v2.signer,
                    hotkey: v2.hotkey,
                    netuid: v2.netuid,
                    order_type: v2.order_type,
                    amount: 1_234_567n,
                    limit_price: v2.limit_price,
                    expiry: v2.expiry,
                    fee_rate: v2.fee_rate,
                    fee_recipient: v2.fee_recipient,
                    relayer: v2.relayer,
                    max_slippage: v2.max_slippage,
                    chain_id: v2.chain_id,
                    partial_fills_enabled: v2.partial_fills_enabled,
                });

                // v1 has no linking concept and must render no trace of one, or every
                // pre-v2 signature would break.
                expect(v1Msg.includes("has-linked-order")).toBe(false);
                expect(v1Msg.endsWith(`signer ${ALICE_SS58}`)).toBe(true);

                // The version tag alone already separates them, so the conditional tail
                // can never create a collision between the two versions.
                expect(v1Msg).not.toBe(formatOrderMessageV2(v2));
                expect(v1Msg.startsWith("TAO.com order v1: ")).toBe(true);
                expect(formatOrderMessageV2(v2).startsWith("TAO.com order v2: ")).toBe(true);
            },
        });

        it({
            id: "T08",
            title: "the readable payload stays past Ledger's raw-signing limit",
            test: () => {
                // `verify_readable` relies on the payload always being hashed on-device.
                // The 64-hex provider id lengthens the message; it must not push it out
                // of that branch, and the shortest possible v2 message must still be in
                // it.
                const shortest = formatOrderMessageV2(
                    makeOrderV2({ netuid: 0, amount: { Fixed: 0n }, limit_price: 0n, expiry: 0n, fee_rate: 0 })
                );
                const wrapped = `<Bytes>${shortest}</Bytes>`;
                expect(wrapped.length).toBeGreaterThan(256);
            },
        });
    },
});
