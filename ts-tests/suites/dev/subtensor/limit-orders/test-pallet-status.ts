import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { tao } from "../../../../utils/balance.js";
import { devForceSetBalance } from "../../../../utils/dev-helpers.js";
import { buildSignedOrder, FAR_FUTURE, filterEvents, registerLimitOrderTypes } from "../../../../utils/limit-orders.js";

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_STATUS",
    title: "set_pallet_status",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            registerLimitOrderTypes(polkadotJs);
            await devForceSetBalance(polkadotJs, context, alice.address, tao(1_000));
        });

        it({
            id: "T01",
            title: "root can disable the pallet",
            test: async () => {
                await context.createBlock([
                    await polkadotJs.tx.sudo.sudo(polkadotJs.tx.limitOrders.setPalletStatus(false)).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                const statusEvent = filterEvents(events, "LimitOrdersPalletStatusChanged");
                expect(statusEvent.length).toBe(1);
                expect(statusEvent[0].event.data[0].isTrue).toBe(false);
            },
        });

        it({
            id: "T02",
            title: "execute_orders is blocked when pallet is disabled",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: alice.address,
                    netuid: 1,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: BigInt(2_000_000_000),
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([signed], false).signAsync(alice),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("LimitOrdersDisabled");
            },
        });

        it({
            id: "T03",
            title: "execute_batched_orders is blocked when pallet is disabled",
            test: async () => {
                const signed = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: alice.address,
                    netuid: 1,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: BigInt(2_000_000_000),
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeBatchedOrders(1, [signed]).signAsync(alice),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("LimitOrdersDisabled");
            },
        });

        it({
            id: "T04",
            title: "root can re-enable the pallet",
            test: async () => {
                await context.createBlock([
                    await polkadotJs.tx.sudo.sudo(polkadotJs.tx.limitOrders.setPalletStatus(true)).signAsync(alice),
                ]);

                const events = await polkadotJs.query.system.events();
                const statusEvent = filterEvents(events, "LimitOrdersPalletStatusChanged");
                expect(statusEvent.length).toBe(1);
                expect(statusEvent[0].event.data[0].isTrue).toBe(true);
            },
        });
    },
});
