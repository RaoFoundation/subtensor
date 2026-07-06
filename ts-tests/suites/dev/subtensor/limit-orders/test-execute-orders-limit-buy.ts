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
    devExecuteOrders,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    FAR_FUTURE,
    fetchChainId,
    filterEvents,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// One subnet per file — this test submits a real buy order that hits the pool.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BUY",
    title: "execute_orders — LimitBuy execution",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let bob: KeyringPair;
        let bobHotKey: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();

            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");
            bob = context.keyring.bob;
            bobHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));

            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            // ENable subtoken
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            // associate hotkeys
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, bob, bobHotKey.address);
        });

        it({
            id: "T01",
            title: "LimitBuy executes when price condition is met",
            test: async () => {
                const stakeBefore = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                const taoBalanceBefore = (await polkadotJs.query.system.account(alice.address)).data.free.toBigInt();

                // TODO: why here far future?
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(100),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });

                await devExecuteOrders(polkadotJs, context, alice, [signed]);

                const events = await polkadotJs.query.system.events();
                const executed = filterEvents(events, "OrderExecuted");
                expect(executed.length).toBe(1);

                // OrderId should be stored as Fulfilled
                const id = orderId(polkadotJs, signed.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Fulfilled");

                // Alpha stake should have increased
                const stakeAfter = await devGetAlphaStake(polkadotJs, aliceHotKey.address, alice.address, netuid);
                expect(stakeAfter).toBeGreaterThan(stakeBefore);

                // TAO balance should have decreased
                const taoBalanceAfter = (await polkadotJs.query.system.account(alice.address)).data.free.toBigInt();
                expect(taoBalanceAfter).toBeLessThan(taoBalanceBefore);
            },
        });
    },
});
