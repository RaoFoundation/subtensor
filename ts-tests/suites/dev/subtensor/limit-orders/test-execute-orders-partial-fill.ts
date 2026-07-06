import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devGetAlphaStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    FAR_FUTURE,
    filterEvents,
    getOrderStatus,
    getPartiallyFilledAmount,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Tests for partial fill via execute_orders.
// The relayer (alice) submits the same signed payload twice with different
// partial_fill values on the envelope.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_PARTIAL_FILL",
    title: "execute_orders — partial fill",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
        });

        it({
            id: "T01",
            title: "first partial fill sets status to PartiallyFilled",
            test: async () => {
                const orderAmount = tao(100);
                const firstFill = Number(tao(60));

                // Build a partial-fills-enabled order with alice as relayer.
                // The signed VersionedOrder payload is the same for both fills.
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: orderAmount,
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    relayer: [alice.address],
                    partialFillsEnabled: true,
                });

                const id = orderId(polkadotJs, signed.order);

                // Submit first partial fill (60 out of 100 TAO).
                const firstEnvelope = { ...signed, partial_fill: firstFill };
                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([firstEnvelope], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);
                expect(filterEvents(events, "OrderSkipped").length).toBe(0);

                expect(await getOrderStatus(polkadotJs, id)).toBe("PartiallyFilled");
                const filled = await getPartiallyFilledAmount(polkadotJs, id);
                expect(filled).toBe(BigInt(firstFill));

                // Alpha stake should have increased (partial buy occurred).
                const stakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(stakeAfter).toBeGreaterThan(0n);
            },
        });

        it({
            id: "T02",
            title: "second partial fill completing the order sets status to Fulfilled",
            test: async () => {
                const orderAmount = tao(200);
                const firstFill = Number(tao(120));
                const secondFill = Number(tao(80));

                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: orderAmount,
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    relayer: [alice.address],
                    partialFillsEnabled: true,
                });

                const id = orderId(polkadotJs, signed.order);

                // First fill: 120 / 200.
                const firstEnvelope = { ...signed, partial_fill: firstFill };
                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([firstEnvelope], false).signAsync(alice),
                ]);

                expect(await getOrderStatus(polkadotJs, id)).toBe("PartiallyFilled");
                expect(await getPartiallyFilledAmount(polkadotJs, id)).toBe(BigInt(firstFill));

                // Second fill: the remaining 80 — completes the order.
                // The signed VersionedOrder payload is identical; only partial_fill on the
                // envelope changes, per the Rust design.
                const secondEnvelope = { ...signed, partial_fill: secondFill };
                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([secondEnvelope], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);

                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");
            },
        });
    },
});
