import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { generateKeyringPair, tao } from "../../../../utils";
import {
    devForceSetBalance,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    FAR_FUTURE,
    filterEvents,
    PERBILL_ONE_PERCENT,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Batched buy orders with fee recipients — own file, hits pool.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BATCH_FEES",
    title: "execute_batched_orders — fee collection",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let bob: KeyringPair;
        let bobHotKey: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");
            bob = context.keyring.bob;
            bobHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));

            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, bob, bobHotKey.address);
        });

        it({
            id: "T01",
            title: "unique fee recipients each receive their own fee",
            test: async () => {
                const feeRecipient1 = generateKeyringPair();
                const feeRecipient2 = generateKeyringPair();

                const r1Before = (
                    (await polkadotJs.query.system.account(feeRecipient1.address)) as any
                ).data.free.toBigInt();
                const r2Before = (
                    (await polkadotJs.query.system.account(feeRecipient2.address)) as any
                ).data.free.toBigInt();

                const orderAlice = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: PERBILL_ONE_PERCENT,
                    feeRecipient: feeRecipient1.address,
                });

                const orderBob = buildSignedOrder(polkadotJs, {
                    signer: bob,
                    hotkey: bobHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: PERBILL_ONE_PERCENT,
                    feeRecipient: feeRecipient2.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders
                        .executeBatchedOrders(netuid, [orderAlice, orderBob])
                        .signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(2);

                const r1After = (
                    (await polkadotJs.query.system.account(feeRecipient1.address)) as any
                ).data.free.toBigInt();
                const r2After = (
                    (await polkadotJs.query.system.account(feeRecipient2.address)) as any
                ).data.free.toBigInt();

                // Both recipients must have received some fee
                expect(r1After).toBeGreaterThan(r1Before);
                expect(r2After).toBeGreaterThan(r2Before);
            },
        });

        it({
            id: "T02",
            title: "shared fee recipient receives aggregated fee",
            test: async () => {
                const sharedRecipient = generateKeyringPair();

                const recipientBefore = (
                    (await polkadotJs.query.system.account(sharedRecipient.address)) as any
                ).data.free.toBigInt();

                const orderAlice = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: PERBILL_ONE_PERCENT,
                    feeRecipient: sharedRecipient.address,
                });

                const orderBob = buildSignedOrder(polkadotJs, {
                    signer: bob,
                    hotkey: bobHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: PERBILL_ONE_PERCENT,
                    feeRecipient: sharedRecipient.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders
                        .executeBatchedOrders(netuid, [orderAlice, orderBob])
                        .signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(2);

                const recipientAfter = (
                    (await polkadotJs.query.system.account(sharedRecipient.address)) as any
                ).data.free.toBigInt();

                // Should have received fees from both orders in a single transfer
                const expectedFee = tao(100) / 100n + tao(100) / 100n; // 1% * 2
                expect(recipientAfter - recipientBefore).toBe(expectedFee);
            },
        });
    },
});
