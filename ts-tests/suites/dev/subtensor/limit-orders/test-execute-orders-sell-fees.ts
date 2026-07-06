import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { generateKeyringPair, tao } from "../../../../utils";
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
    PERBILL_ONE_PERCENT,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Sell order with fee — separate file, hits pool.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_FEE_SELL",
    title: "execute_orders — sell order fee collection",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let bob: KeyringPair;
        let feeRecipient: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair();
            bob = context.keyring.bob;
            feeRecipient = generateKeyringPair();
            registerLimitOrderTypes(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));

            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            // ENable subtoken
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            // associate hotkeys
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);

            // Give Alice some alpha stake to sell
            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(1000));
        });

        it({
            id: "T01",
            title: "fee recipient receives TAO from sell order output with 1% fee",
            test: async () => {
                const recipientBefore = (
                    await polkadotJs.query.system.account(feeRecipient.address)
                ).data.free.toBigInt();

                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: tao(100),
                    limitPrice: 1_000_000_000n, // always met when price >= 1 TAO/alpha (×10⁹ scale)
                    expiry: FAR_FUTURE,
                    feeRate: PERBILL_ONE_PERCENT,
                    feeRecipient: feeRecipient.address,
                });

                await devExecuteOrders(polkadotJs, context, alice, [signed]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderExecuted").length).toBe(1);

                const recipientAfter = (
                    await polkadotJs.query.system.account(feeRecipient.address)
                ).data.free.toBigInt();

                // Fee recipient must have received something > 0
                expect(recipientAfter).toBeGreaterThan(recipientBefore);
            },
        });
    },
});
