import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    EXPIRED,
    FAR_FUTURE,
    filterEvents,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Tests in this file cover skip conditions: price-not-met, expired, bad-sig,
// root-netuid, already-processed.  Pool price after devEnableSubtoken is ~1 TAO/alpha,
// so LimitBuy with limitPrice=1n is always skipped and TakeProfit with limitPrice=FAR_FUTURE too.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_SKIP",
    title: "execute_orders — skip conditions",
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
            title: "LimitBuy skipped when limit_price below current price",
            test: async () => {
                // limit_price = 0: current_price (1.0 TAO/alpha) > 0 → condition never met
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: 0n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
                expect(filterEvents(events, "OrderExecuted").length).toBe(0);
            },
        });

        it({
            id: "T02",
            title: "TakeProfit skipped when price below limit_price",
            test: async () => {
                // limit_price = u64::MAX — price can never reach this
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
                expect(filterEvents(events, "OrderExecuted").length).toBe(0);
            },
        });

        it({
            id: "T03",
            title: "expired order is skipped",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: EXPIRED,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
            },
        });

        it({
            id: "T04",
            title: "order with invalid signature is skipped",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                // Tamper: change the amount inside the V1 inner order after signing.
                // The signature now covers different bytes — validation must reject it.
                const tampered = {
                    ...signed,
                    order: { V1: { ...signed.order.V1, amount: tao(999) } },
                };

                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([tampered], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
            },
        });

        it({
            id: "T05",
            title: "order targeting root netuid (0) is skipped",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: 0,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
            },
        });

        it({
            id: "T06",
            title: "already-fulfilled order is skipped on second execution attempt",
            test: async () => {
                // Use a price condition that is always met (limitPrice = u64::MAX for buy)
                // so the first call succeeds and fulfils the order.
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                // First execution — should succeed.
                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                // Second attempt — order already Fulfilled, must be skipped.
                await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
                expect(filterEvents(events, "OrderExecuted").length).toBe(0);
            },
        });

        it({
            id: "T07",
            title: "mixed batch: valid orders execute, invalid ones are skipped",
            test: async () => {
                const valid = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(4), // distinct from T06 to get a different OrderId
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const expired = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(2),
                    limitPrice: FAR_FUTURE,
                    expiry: EXPIRED,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const priceNotMet = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(3),
                    limitPrice: 0n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders
                        .executeOrders([valid, expired, priceNotMet], false)
                        .signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);
                expect(filterEvents(events, "OrderSkipped").length).toBe(2);
            },
        });
    },
});
