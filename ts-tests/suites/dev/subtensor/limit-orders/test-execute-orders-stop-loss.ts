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

// Separate file — StopLoss sell changes pool price.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_SL",
    title: "execute_orders — StopLoss execution",
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
            title: "StopLoss executes when price <= limit_price",
            test: async () => {
                const stakeBefore = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const taoBalanceBefore = (await polkadotJs.query.system.account(alice.address)).data.free.toBigInt();

                // limit_price = 100_000_000_000 (100.0 TAO/alpha in ×10⁹ scale) — safely above the
                // actual pool price on the freshly registered dynamic subnet after devAddStake(tao(1000)).
                // max_slippage is unset (None) so the effective AMM floor is 0; the limit_price here
                // only controls the StopLoss trigger condition, not the swap execution price.
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "StopLoss",
                    amount: tao(100),
                    limitPrice: 100_000_000_000n,
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

                const stakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(stakeAfter).toBeLessThan(stakeBefore);

                const taoBalanceAfter = (await polkadotJs.query.system.account(alice.address)).data.free.toBigInt();
                expect(taoBalanceAfter).toBeGreaterThan(taoBalanceBefore);
            },
        });
    },
});
