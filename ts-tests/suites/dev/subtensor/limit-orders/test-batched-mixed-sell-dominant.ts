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

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BATCH_MIX_SELL",
    title: "execute_batched_orders — sell-dominant mixed batch",
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

            // Bob sells a large amount, needs alpha
            await devAddStake(polkadotJs, context, bob, bobHotKey.address, netuid, tao(500));
        });

        it({
            id: "T01",
            title: "sell side dominates: both orders fulfilled, net sell hits pool",
            test: async () => {
                const aliceStakeBefore = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const bobTaoBefore = ((await polkadotJs.query.system.account(bob.address)) as any).data.free.toBigInt();

                // Alice buys 10 TAO, Bob sells 200 alpha (~200 TAO equiv)
                // → net sell ~190 alpha hits the pool
                const buyOrder = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(10),
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
                    amount: tao(200),
                    limitPrice: 1_000_000_000n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: bob.address,
                });

                // Read price before the swap — pallet uses pre-swap price for netting
                const expectedNetAmount = await computeNetAmount(polkadotJs, netuid, tao(10), tao(200), "Sell");

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
                // net_side should be Sell (residual alpha sent to pool)
                expect(summaryData[1].type).toBe("Sell");
                // net_amount matches sell_alpha - tao_to_alpha(buy_tao, price)
                const netAmountDiff = summaryData[2].toBigInt() - expectedNetAmount;
                expect(netAmountDiff < 0n ? -netAmountDiff : netAmountDiff).toBeLessThanOrEqual(10n);
                // actual_out > 0 proves the pool returned TAO
                expect(summaryData[3].toBigInt()).toBeGreaterThan(0n);

                const aliceStakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(aliceStakeAfter).toBeGreaterThan(aliceStakeBefore);

                const bobTaoAfter = ((await polkadotJs.query.system.account(bob.address)) as any).data.free.toBigInt();
                expect(bobTaoAfter).toBeGreaterThan(bobTaoBefore);
            },
        });
    },
});
