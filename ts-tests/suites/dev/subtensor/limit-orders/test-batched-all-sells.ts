import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAddStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import { buildSignedOrder, FAR_FUTURE, filterEvents, registerLimitOrderTypes } from "../../../../utils/limit-orders.js";

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BATCH_SELL",
    title: "execute_batched_orders — all-sell batch",
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

            // Stake alpha for both sellers
            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(200));
            await devAddStake(polkadotJs, context, bob, bobHotKey.address, netuid, tao(200));
        });

        it({
            id: "T01",
            title: "all sellers receive TAO and GroupExecutionSummary is emitted",
            test: async () => {
                const aliceTaoBefore = (
                    (await polkadotJs.query.system.account(alice.address)) as any
                ).data.free.toBigInt();
                const bobTaoBefore = ((await polkadotJs.query.system.account(bob.address)) as any).data.free.toBigInt();

                const orderAlice = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: tao(50),
                    limitPrice: 1_000_000_000n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const orderBob = buildSignedOrder(polkadotJs, {
                    signer: bob,
                    hotkey: bobHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: tao(50),
                    limitPrice: 1_000_000_000n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: bob.address,
                });

                await context.createBlock([
                    await polkadotJs.tx.limitOrders
                        .executeBatchedOrders(netuid, [orderAlice, orderBob])
                        .signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(2);
                expect(filterEvents(events, "GroupExecutionSummary").length).toBe(1);

                const aliceTaoAfter = (
                    (await polkadotJs.query.system.account(alice.address)) as any
                ).data.free.toBigInt();
                const bobTaoAfter = ((await polkadotJs.query.system.account(bob.address)) as any).data.free.toBigInt();

                expect(aliceTaoAfter).toBeGreaterThan(aliceTaoBefore);
                expect(bobTaoAfter).toBeGreaterThan(bobTaoBefore);
            },
        });
    },
});
