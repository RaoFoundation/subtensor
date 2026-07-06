import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAddStake,
    devGetAlphaStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    computeNetAmount,
    FAR_FUTURE,
    filterEvents,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Buy-dominant mixed batch — net buy hits the pool.  Own file.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BATCH_MIX_BUY",
    title: "execute_batched_orders — buy-dominant mixed batch",
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

            // Bob sells, needs alpha
            await devAddStake(polkadotJs, context, bob, bobHotKey.address, netuid, tao(200));
        });

        it({
            id: "T01",
            title: "buy side dominates: both orders fulfilled, net buy hits pool",
            test: async () => {
                const aliceStakeBefore = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const bobTaoBefore = ((await polkadotJs.query.system.account(bob.address)) as any).data.free.toBigInt();

                // Alice buys 200 TAO worth, Bob sells 10 alpha (~10 TAO equiv)
                // → net buy ~190 TAO hits the pool
                const buyOrder = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(200),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const sellOrder = buildSignedOrder(polkadotJs, {
                    signer: bob,
                    hotkey: bobHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: tao(10),
                    limitPrice: 1_000_000_000n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: bob.address,
                });

                // Read price before the swap — pallet uses pre-swap price for netting
                const expectedNetAmount = await computeNetAmount(polkadotJs, netuid, tao(200), tao(10), "Buy");

                await context.createBlock([
                    await polkadotJs.tx.limitOrders
                        .executeBatchedOrders(netuid, [buyOrder, sellOrder])
                        .signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(2);

                const summary = filterEvents(events, "GroupExecutionSummary");
                expect(summary.length).toBe(1);
                const summaryData = summary[0].event.data;
                // net_side should be Buy (residual TAO sent to pool)
                expect(summaryData[1].type).toBe("Buy");
                // net_amount matches buy_tao - alpha_to_tao(sell_alpha, price)
                const netAmountDiff = summaryData[2].toBigInt() - expectedNetAmount;
                expect(netAmountDiff < 0n ? -netAmountDiff : netAmountDiff).toBeLessThanOrEqual(10n);
                // actual_out > 0 proves the pool returned alpha
                expect(summaryData[3].toBigInt()).toBeGreaterThan(0n);

                const aliceStakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(aliceStakeAfter).toBeGreaterThan(aliceStakeBefore);

                const bobTaoAfter = ((await polkadotJs.query.system.account(bob.address)) as any).data.free.toBigInt();
                expect(bobTaoAfter).toBeGreaterThan(bobTaoBefore);
            },
        });
    },
});
