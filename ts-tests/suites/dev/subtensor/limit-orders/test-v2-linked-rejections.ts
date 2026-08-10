import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAddStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
    devExecuteOrders,
} from "../../../../utils/dev-helpers.js";
import {
    type SignedOrder,
    buildSignedOrderV2,
    FAR_FUTURE,
    fetchChainId,
    filterEvents,
    getLinkedOutput,
    getOrderStatus,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// Every way a linked order can be refused.  All assertions read the extrinsic's own
// outcome via the `createBlock` result rather than scanning events, so a failure is
// attributed to a specific pallet error rather than to "something did not happen".
//
// `should_fail = true` throughout: these are hard rejections, and best-effort mode
// would swallow them into `OrderSkipped`.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_V2_REJECTIONS",
    title: "limit-orders v2 — linked order rejection paths",
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
            bob = context.keyring.bob;
            aliceHotKey = generateKeyringPair("sr25519");
            bobHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);
            await devAssociateHotKey(polkadotJs, context, bob, bobHotKey.address);

            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(1000));
        });

        // An `order_id` is `blake2_256(SCALE(payload))`: no nonce, and the signature is
        // not part of the preimage.  Every `it` in this file shares one chain, so two
        // tests that build the same payload collide on `order_id` and the second
        // execution is refused `OrderAlreadyProcessed` — quietly, because the earlier
        // test's record is still sitting at that key and satisfies a "the record exists"
        // assertion.  Salting the expiry keeps every payload distinct; nothing here
        // asserts on expiry beyond it being in the future.
        //
        // T06/T07 are the deliberate exception: they must share one payload, so it is
        // built once in `providerWithPartialFillsEnabled`.
        let salt = 0n;
        function uniqueExpiry(): bigint {
            salt += 1n;
            return FAR_FUTURE - salt;
        }

        /**
         * A sell that records its proceeds, executed so its record exists.
         *
         * Asserts the record was written by THIS call, via the `LinkedOutputRecorded`
         * event in the block just produced.  Reading `LinkedOutputs` instead would also
         * be satisfied by a record an earlier test left at the same key, which is exactly
         * how a collision hides itself.
         */
        async function executeProvider(amount: bigint): Promise<`0x${string}`> {
            const provider = buildSignedOrderV2(polkadotJs, {
                signer: alice,
                hotkey: aliceHotKey.address,
                netuid,
                orderType: "TakeProfit",
                amount: { Fixed: amount },
                limitPrice: 0n,
                expiry: uniqueExpiry(),
                feeRate: 0,
                feeRecipient: alice.address,
                chainId,
                hasLinkedOrder: true,
            });
            const id = orderId(polkadotJs, provider.order);
            await devExecuteOrders(polkadotJs, context, bob, [provider], true);

            const recorded = filterEvents(await polkadotJs.query.system.events(), "LinkedOutputRecorded");
            expect(recorded.map((e) => e.event.data.orderId.toString())).toContain(id);
            return id;
        }

        /**
         * The provider payload shared by T06 and T07 — partial fills enabled AND
         * `has_linked_order` set.
         *
         * Built once and memoised on purpose: the pair only demonstrates something if
         * both tests submit the IDENTICAL payload, since the claim is that T06's
         * rejection is on the submitted fill rather than on the signed flags. Two
         * separately-built payloads would merely show that a *similar* order executes.
         */
        let partialFillProvider: SignedOrder | undefined;
        function providerWithPartialFillsEnabled(): SignedOrder {
            if (partialFillProvider === undefined) {
                partialFillProvider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(50) },
                    limitPrice: 0n,
                    expiry: uniqueExpiry(),
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    relayer: [bob.address],
                    partialFillsEnabled: true,
                    hasLinkedOrder: true,
                });
            }
            return partialFillProvider;
        }

        /** A TAO-spending linked buy against `provider`, signed by `signer`. */
        function linkedBuy(signer: KeyringPair, hotkey: string, provider: `0x${string}`, pct: number) {
            return buildSignedOrderV2(polkadotJs, {
                signer,
                hotkey,
                netuid,
                orderType: "LimitBuy",
                amount: { LinkedPercentage: { provider, pct } },
                limitPrice: FAR_FUTURE,
                expiry: FAR_FUTURE,
                feeRate: 0,
                feeRecipient: signer.address,
                chainId,
            });
        }

        it({
            id: "T01",
            title: "naming a provider with no record fails NoLinkedOutput",
            test: async () => {
                const unknown = `0x${"99".repeat(32)}` as `0x${string}`;
                const linked = linkedBuy(alice, aliceHotKey.address, unknown, 1_000_000_000);

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([linked], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NoLinkedOutput");
            },
        });

        it({
            id: "T02",
            title: "naming an order that never declared has_linked_order fails NoLinkedOutput",
            test: async () => {
                // Executes fine, but records nothing — so nothing can link to it.
                const plain = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(10) },
                    limitPrice: 0n,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });
                const plainId = orderId(polkadotJs, plain.order);
                await devExecuteOrders(polkadotJs, context, bob, [plain], true);
                expect(await getOrderStatus(polkadotJs, plainId)).toBe("Fulfilled");
                expect(await getLinkedOutput(polkadotJs, plainId)).toBeUndefined();

                const linked = linkedBuy(alice, aliceHotKey.address, plainId, 1_000_000_000);
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([linked], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NoLinkedOutput");
            },
        });

        it({
            id: "T03",
            title: "a second linked order naming the same provider finds nothing",
            test: async () => {
                const providerId = await executeProvider(tao(100));

                // First draw takes 60% and consumes the record.
                const first = linkedBuy(alice, aliceHotKey.address, providerId, 600_000_000);
                await devExecuteOrders(polkadotJs, context, bob, [first], true);
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();

                // A second linked order the user also signed against 60% would have
                // over-drawn. Removing the record on first use makes that state
                // unrepresentable rather than merely rejected.
                const second = linkedBuy(alice, bobHotKey.address, providerId, 600_000_000);
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([second], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NoLinkedOutput");
            },
        });

        it({
            id: "T04",
            title: "a linked order signed by a different coldkey fails LinkedOutputSignerMismatch",
            test: async () => {
                const providerId = await executeProvider(tao(50));

                // Bob signs a buy against Alice's recorded proceeds. The TAO sits in
                // Alice's account, so Bob would be spending his own funds on Alice's
                // authorisation.
                const foreign = linkedBuy(bob, bobHotKey.address, providerId, 1_000_000_000);
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([foreign], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("LinkedOutputSignerMismatch");
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();
            },
        });

        it({
            id: "T05",
            title: "a partial fill against a linked order is rejected",
            test: async () => {
                const providerId = await executeProvider(tao(50));

                const linked = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "LimitBuy",
                    amount: { LinkedPercentage: { provider: providerId, pct: 1_000_000_000 } },
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    relayer: [bob.address],
                    partialFillsEnabled: true,
                });
                // Inject a fill into the envelope (not part of the signed payload).
                const withFill = { ...linked, partial_fill: Number(tao(1)) };

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([withFill], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("PartialFillNotSupportedForLinkedAmount");
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();
            },
        });

        it({
            id: "T06",
            title: "a partial fill against a provider is rejected",
            test: async () => {
                const provider = providerWithPartialFillsEnabled();
                const providerId = orderId(polkadotJs, provider.order);
                const withFill = { ...provider, partial_fill: Number(tao(10)) };

                // Filling in instalments would make the recorded total depend on how the
                // relayer sliced the fills, so a provider must execute in one shot.
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeOrders([withFill], true).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("PartialFillNotSupportedForProvider");
                expect(await getOrderStatus(polkadotJs, providerId)).toBeUndefined();
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();
            },
        });

        it({
            id: "T07",
            title: "the same provider payload executes in full even with partial fills enabled",
            test: async () => {
                // Byte-for-byte the payload T06 submitted — only the injected fill is
                // gone. That is what makes T06's rejection attributable to the fill
                // rather than to the signed `partial_fills_enabled` / `has_linked_order`
                // combination.
                const provider = providerWithPartialFillsEnabled();
                const providerId = orderId(polkadotJs, provider.order);

                await devExecuteOrders(polkadotJs, context, bob, [provider], true);

                expect(await getOrderStatus(polkadotJs, providerId)).toBe("Fulfilled");
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();
            },
        });

        it({
            id: "T08",
            title: "a provider and its linked order in one batched call fails NoLinkedOutput",
            test: async () => {
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(50) },
                    limitPrice: 0n,
                    expiry: uniqueExpiry(),
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: true,
                });
                const providerId = orderId(polkadotJs, provider.order);
                const linked = linkedBuy(alice, aliceHotKey.address, providerId, 1_000_000_000);

                // Structural, not incidental: `execute_batched_orders` resolves and freezes
                // every amount before the single netted swap that would produce the
                // provider's output even runs. Split them across two calls.
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.executeBatchedOrders(netuid, [provider, linked]).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NoLinkedOutput");
            },
        });
    },
});
