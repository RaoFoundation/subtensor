import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao } from "../../../../utils";
import { devForceSetBalance, devExecuteOrders } from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrder,
    FAR_FUTURE,
    filterEvents,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_CANCEL",
    title: "cancel_order",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let bob: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            bob = context.keyring.bob;

            registerLimitOrderTypes(polkadotJs);
            await devForceSetBalance(polkadotJs, context, alice.address, tao(1_000));
        });

        it({
            id: "T01",
            title: "signer can cancel their own order",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: alice.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: BigInt(2_000_000_000),
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const tx = polkadotJs.tx.limitOrders.cancelOrder(signed.order);
                await context.createBlock([await tx.signAsync(alice)]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderCancelled").length).toBe(1);

                const id = orderId(polkadotJs, signed.order);
                expect(await getOrderStatus(polkadotJs, id)).toBe("Cancelled");
            },
        });

        it({
            id: "T02",
            title: "non-signer cannot cancel another account's order",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: alice.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(2),
                    limitPrice: BigInt(2_000_000_000),
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                // Bob tries to cancel Alice's order
                const tx = polkadotJs.tx.limitOrders.cancelOrder(signed.order);
                const {
                    result: [attempt],
                } = await context.createBlock([await tx.signAsync(bob)]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("Unauthorized");
            },
        });

        it({
            id: "T03",
            title: "cancelling an already-cancelled order fails",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: alice.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(3),
                    limitPrice: BigInt(2_000_000_000),
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const tx = polkadotJs.tx.limitOrders.cancelOrder(signed.order);
                await context.createBlock([await tx.signAsync(alice)]);

                // Second cancel must fail
                const tx2 = polkadotJs.tx.limitOrders.cancelOrder(signed.order);
                await context.createBlock([await tx2.signAsync(alice)]);

                const events = await polkadotJs.query.system.events();
                const cancelled = filterEvents(events, "OrderCancelled");
                expect(cancelled.length).toBe(0);
            },
        });

        it({
            id: "T04",
            title: "executing a cancelled order emits OrderSkipped",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: alice.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(4),
                    limitPrice: BigInt(2_000_000_000),
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                // Cancel first
                await context.createBlock([await polkadotJs.tx.limitOrders.cancelOrder(signed.order).signAsync(alice)]);

                // Now try to execute
                await devExecuteOrders(polkadotJs, context, alice, [signed]);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(1);
                expect(filterEvents(events, "OrderExecuted").length).toBe(0);
            },
        });
    },
});
