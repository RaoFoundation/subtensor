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
import { buildSignedOrder, FAR_FUTURE, filterEvents, registerLimitOrderTypes } from "../../../../utils/limit-orders.js";

// execute_batched_orders — all-buy batch.  Own subnet, own file.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BATCH_BUY",
    title: "execute_batched_orders — all-buy batch",
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
            title: "all buyers receive alpha and GroupExecutionSummary is emitted",
            test: async () => {
                const aliceStakeBefore = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const bobStakeBefore = await devGetAlphaStake(polkadotJs, bobHotKey.address, bob.address, netuid);

                const orderAlice = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(50),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const orderBob = buildSignedOrder(polkadotJs, {
                    signer: bob,
                    hotkey: bobHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(50),
                    limitPrice: FAR_FUTURE,
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

                const aliceStakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(aliceStakeAfter).toBeGreaterThan(aliceStakeBefore);

                const bobStakeAfter = await devGetAlphaStake(polkadotJs, bobHotKey.address, bob.address, netuid);
                expect(bobStakeAfter).toBeGreaterThan(bobStakeBefore);
            },
        });
    },
});
