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
    buildSignedOrderV2,
    FAR_FUTURE,
    fetchChainId,
    filterEvents,
    expectLinkedOutput,
    getLinkedOutput,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// The mirror of the rotation: a BUY provider.  Its output is alpha on one specific
// `(netuid, hotkey)` position, so the only order that can draw against it is a sell
// from that same position — "take profit on exactly the alpha this buy produced,
// and no more".
//
// This is the shape that persisting the record across blocks unlocks: the sell is
// price-triggered and may fire an arbitrary number of blocks later.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_V2_BUY_PROVIDER",
    title: "limit-orders v2 — buy provider then linked sell",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let otherHotKey: KeyringPair;
        let bob: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            bob = context.keyring.bob;
            aliceHotKey = generateKeyringPair("sr25519");
            otherHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, alice, otherHotKey.address);

            // Seed the pool so buys and sells both have liquidity to work against.
            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(1000));
        });

        it({
            id: "T01",
            title: "a buy provider records alpha on its own position, and a linked sell drains it",
            test: async () => {
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: otherHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: { Fixed: tao(100) },
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: true,
                });
                const providerId = orderId(polkadotJs, provider.order);

                await devExecuteOrders(polkadotJs, context, bob, [provider], true);

                // A buy's output is alpha, tagged with the exact position it landed in.
                const record = await expectLinkedOutput(polkadotJs, providerId);
                expect(record.asset).toEqual({ netuid, hotkey: otherHotKey.address });
                expect(record.total).toBeGreaterThan(0n);

                const recordedTotal = record.total;
                const stakeBefore = await devGetAlphaStake(polkadotJs, otherHotKey.address, alice.address, netuid);

                // The take-profit sells 100% of what that buy produced — a separate
                // dispatch, which is the point of the record persisting.
                const linked = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: otherHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: { LinkedPercentage: { provider: providerId, pct: 1_000_000_000 } },
                    limitPrice: 0n, // no floor
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });
                const linkedId = orderId(polkadotJs, linked.order);

                await devExecuteOrders(polkadotJs, context, bob, [linked], true);

                const events = await polkadotJs.query.system.events();
                expect(filterEvents(events, "OrderSkipped").length).toBe(0);
                expect(await getOrderStatus(polkadotJs, linkedId)).toBe("Fulfilled");

                const consumed = filterEvents(events, "LinkedOutputConsumed");
                expect(consumed.length).toBe(1);
                // Sold exactly the recorded alpha, not the whole position.
                expect(BigInt(consumed[0].event.data.amount.toString())).toBe(recordedTotal);
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();

                const stakeAfter = await devGetAlphaStake(polkadotJs, otherHotKey.address, alice.address, netuid);
                expect(stakeAfter).toBeLessThan(stakeBefore);
            },
        });

        it({
            id: "T02",
            title: "a linked sell from a different hotkey is rejected as an asset mismatch",
            test: async () => {
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: otherHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: { Fixed: tao(50) },
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: true,
                });
                const providerId = orderId(polkadotJs, provider.order);

                await devExecuteOrders(polkadotJs, context, bob, [provider], true);
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();

                // Same subnet, DIFFERENT hotkey — alpha is only fungible within one
                // (netuid, hotkey) position, so this is not the asset that was recorded.
                const wrongPosition = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: { LinkedPercentage: { provider: providerId, pct: 1_000_000_000 } },
                    limitPrice: 0n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([wrongPosition], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("LinkedOutputAssetMismatch");

                // The rejected draw left the record untouched.
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();
            },
        });

        it({
            id: "T03",
            title: "a linked BUY cannot draw against an alpha record",
            test: async () => {
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: otherHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: { Fixed: tao(50) },
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: true,
                });
                const providerId = orderId(polkadotJs, provider.order);
                await devExecuteOrders(polkadotJs, context, bob, [provider], true);

                // A buy spends TAO; the record holds alpha. Only a sell can consume it.
                const wrongSide = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: otherHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: { LinkedPercentage: { provider: providerId, pct: 1_000_000_000 } },
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([wrongSide], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("LinkedOutputAssetMismatch");
            },
        });
    },
});
