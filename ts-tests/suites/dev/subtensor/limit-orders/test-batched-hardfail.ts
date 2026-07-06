import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import { buildSignedOrder, FAR_FUTURE, registerLimitOrderTypes } from "../../../../utils/limit-orders.js";

// Hard-fail cases for execute_batched_orders — no pool interaction needed,
// all batches fail before reaching the swap step.  Single subnet is fine.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_BATCH_HARDFAIL",
    title: "execute_batched_orders — hard-fail conditions",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            aliceHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);

            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
        });

        it({
            id: "T01",
            title: "batch fails entirely when one order has an invalid signature",
            test: async () => {
                const valid = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const badSig = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(2),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });
                // Tamper after signing — signature now covers different bytes
                const tampered = {
                    ...badSig,
                    order: { V1: { ...badSig.order.V1, amount: tao(999) } },
                };

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeBatchedOrders(netuid, [valid, tampered]).signAsync(alice),
                ]);

                // The whole extrinsic should fail — hard-fail on invalid signature
                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("InvalidSignature");
            },
        });

        it({
            id: "T02",
            title: "batch fails when one order targets a different netuid",
            test: async () => {
                const correct = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const wrongNetuid = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: netuid + 1, // different subnet
                    orderType: "LimitBuy",
                    amount: tao(2),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders
                        .executeBatchedOrders(netuid, [correct, wrongNetuid])
                        .signAsync(alice),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("OrderNetUidMismatch");
            },
        });

        it({
            id: "T03",
            title: "root netuid (0) as batch parameter fails immediately",
            test: async () => {
                const order = buildSignedOrder(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: 0,
                    orderType: "LimitBuy",
                    amount: tao(1),
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeBatchedOrders(0, [order]).signAsync(alice),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("RootNetUidNotAllowed");
            },
        });
    },
});
