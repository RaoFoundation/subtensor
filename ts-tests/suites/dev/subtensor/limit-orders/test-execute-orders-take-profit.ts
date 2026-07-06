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
    devExecuteOrders,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    FAR_FUTURE,
    filterEvents,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Separate file because a TakeProfit sell changes pool price.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_TP",
    title: "execute_orders — TakeProfit execution",
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

            // ENable subtoken
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            // associate hotkeys
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, bob, bobHotKey.address);

            // Give Alice some alpha stake to sell
            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(1000));
        });

        it({
            id: "T01",
            title: "TakeProfit executes when price >= limit_price",
            test: async () => {
                const stakeBefore = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const taoBalanceBefore = (await polkadotJs.query.system.account(alice.address)).data.free.toBigInt();

                // limit_price = 1_000_000_000 (1.0 TAO/alpha in ×10⁹ scale) — current price after
                // devAddStake(tao(1000)) is above 1.0 TAO/alpha, so this condition is always met
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: tao(100),
                    limitPrice: 1_000_000_000n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                await devExecuteOrders(polkadotJs, context, alice, [signed]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);
                expect(filterEvents(events, "OrderSkipped").length).toBe(0);

                const id = orderId(polkadotJs, signed.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");

                // Alpha stake should have decreased
                const stakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(stakeAfter).toBeLessThan(stakeBefore);

                // TAO balance should have increased
                const taoBalanceAfter = (await polkadotJs.query.system.account(alice.address)).data.free.toBigInt();
                expect(taoBalanceAfter).toBeGreaterThan(taoBalanceBefore);
            },
        });
    },
});
