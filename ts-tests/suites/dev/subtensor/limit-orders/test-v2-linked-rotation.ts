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
    linkedOutputTtl,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// The driving use case for v2: sell alpha on one subnet, then put the TAO that
// produced into another — sized off the realised proceeds rather than off a number
// the user had to guess at signing time.
//
// Two subnets, because a rotation that stayed on one subnet would not exercise the
// interesting part.  The provider and the linked order go in ONE `execute_orders`
// call: each order runs to completion before the next is validated, so the record is
// already on chain when the linked order resolves against it.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_V2_ROTATION",
    title: "limit-orders v2 — sell provider then linked buy",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let secondHotKey: KeyringPair;
        let bob: KeyringPair;
        let sourceNetuid: number;
        let targetNetuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            bob = context.keyring.bob;
            aliceHotKey = generateKeyringPair("sr25519");
            secondHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            // This suite needs TWO subnets, which is what makes it the only limit-orders
            // dev suite that has to deal with the register-network rate limit.
            //
            // `NetworkRateLimit` defaults to `InitialNetworkRateLimit` = 7200 blocks in a
            // plain release build (it is only 0 under `pow-faucet`, which the dev binary
            // is not built with). The limit is compared against ONE chain-wide slot,
            // `LastRateLimitedBlock[NetworkLastRegistered]`, which is 0 on a fresh chain —
            // so the first `register_network` passes via the `last_block == 0`
            // short-circuit and stamps the slot, and the second is then refused for the
            // next 7200 blocks. Under `--sealing=manual` those blocks only exist if a test
            // mines them, so the wait is not survivable.
            //
            // The refusal comes from a transaction extension rather than the dispatch, so
            // it surfaces as `Invalid Transaction: Custom error: 6` at pool admission —
            // an RpcError with no extrinsic and no event to inspect. Dropping the limit to
            // zero is what `test-transfer-stake-rate-limit.ts` does for the same reason.
            //
            // Note this is a different throttle from the lock-cost reduction set above.
            await context.createBlock([
                await polkadotJs.tx.sudo.sudo(polkadotJs.tx.adminUtils.sudoSetNetworkRateLimit(0)).signAsync(alice),
            ]);

            // Source subnet: Alice holds alpha here and sells it.
            sourceNetuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);
            await devEnableSubtoken(polkadotJs, context, alice, sourceNetuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);

            // Target subnet: the proceeds get bought into here.
            targetNetuid = await devRegisterSubnet(polkadotJs, context, alice, secondHotKey);
            await devEnableSubtoken(polkadotJs, context, alice, targetNetuid);
            await devAssociateHotKey(polkadotJs, context, alice, secondHotKey.address);

            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, sourceNetuid, tao(1000));
        });

        // An `order_id` is `blake2_256(SCALE(payload))`: no nonce, and the signature is
        // not part of the preimage.  Every `it` in this file shares one chain, so two
        // tests that build the same payload collide on `order_id` and the second
        // execution is refused `OrderAlreadyProcessed`.  T01 and T04 both sell
        // `tao(100)` off the same position, which is exactly such a pair.  Salting the
        // expiry keeps every payload distinct; nothing here asserts on expiry beyond it
        // being in the future.
        let salt = 0n;
        function uniqueExpiry(): bigint {
            salt += 1n;
            return FAR_FUTURE - salt;
        }

        it({
            id: "T01",
            title: "a provider records its post-fee output and a linked buy spends it",
            test: async () => {
                const sourceStakeBefore = await devGetAlphaStake(
                    polkadotJs,
                    aliceHotKey.address,
                    alice.address,
                    sourceNetuid
                );
                const targetStakeBefore = await devGetAlphaStake(
                    polkadotJs,
                    secondHotKey.address,
                    alice.address,
                    targetNetuid
                );

                // Leg 1: sell source alpha, and declare that the proceeds are drawable.
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: sourceNetuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(100) },
                    limitPrice: 0n, // no floor — this test is about linking, not the trigger
                    expiry: uniqueExpiry(),
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: true,
                });

                // The provider's OrderId must be known BEFORE the linked order is signed:
                // that two-phase flow is what binds the link to one specific order rather
                // than to whatever the relayer puts in front of it.
                const providerId = orderId(polkadotJs, provider.order);

                // Leg 2: buy on the target subnet with 100% of leg 1's proceeds.
                const linked = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: secondHotKey.address,
                    netuid: targetNetuid,
                    orderType: "LimitBuy",
                    amount: { LinkedPercentage: { provider: providerId, pct: 1_000_000_000 } },
                    limitPrice: FAR_FUTURE, // no ceiling
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });
                const linkedId = orderId(polkadotJs, linked.order);

                await devExecuteOrders(polkadotJs, context, bob, [provider, linked], true);

                const events = await polkadotJs.query.system.events();
                const executed = filterEvents(events, "OrderExecuted");
                expect(filterEvents(events, "OrderSkipped").length).toBe(0);
                expect(executed.length).toBe(2);

                // Both legs are ordinary, independently-tracked orders.
                expect(await getOrderStatus(polkadotJs, providerId)).toBe("Fulfilled");
                expect(await getOrderStatus(polkadotJs, linkedId)).toBe("Fulfilled");

                // The recorded total must equal the TAO the sell actually delivered —
                // the same figure `OrderExecuted` reports as amount_out.
                const recorded = filterEvents(events, "LinkedOutputRecorded");
                expect(recorded.length).toBe(1);
                const recordedTotal = BigInt(recorded[0].event.data.total.toString());
                const providerExecuted = executed.find((e: any) => e.event.data.orderId.toString() === providerId);
                expect(providerExecuted).toBeDefined();
                expect(BigInt(providerExecuted.event.data.amountOut.toString())).toBe(recordedTotal);
                expect(recordedTotal).toBeGreaterThan(0n);

                // The linked buy spent exactly the recorded proceeds — 100% of them.
                const consumed = filterEvents(events, "LinkedOutputConsumed");
                expect(consumed.length).toBe(1);
                expect(consumed[0].event.data.provider.toString()).toBe(providerId);
                expect(consumed[0].event.data.consumer.toString()).toBe(linkedId);
                expect(BigInt(consumed[0].event.data.amount.toString())).toBe(recordedTotal);
                expect(BigInt(consumed[0].event.data.undrawn.toString())).toBe(0n);

                // Drawing consumes the record, which is also what stops it being reused.
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();

                // Stake moved out of the source subnet and into the target.
                const sourceStakeAfter = await devGetAlphaStake(
                    polkadotJs,
                    aliceHotKey.address,
                    alice.address,
                    sourceNetuid
                );
                const targetStakeAfter = await devGetAlphaStake(
                    polkadotJs,
                    secondHotKey.address,
                    alice.address,
                    targetNetuid
                );
                expect(sourceStakeAfter).toBeLessThan(sourceStakeBefore);
                expect(targetStakeAfter).toBeGreaterThan(targetStakeBefore);
            },
        });

        it({
            id: "T02",
            title: "a provider left undrawn keeps its record, stamped with signer, asset and TTL",
            test: async () => {
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: sourceNetuid,
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

                await devExecuteOrders(polkadotJs, context, bob, [provider], true);

                const record = await expectLinkedOutput(polkadotJs, providerId);
                expect(record.signer).toBe(alice.address);
                // A sell produces TAO, so only a LimitBuy can draw against it.
                expect(record.asset).toBe("Tao");
                expect(record.total).toBeGreaterThan(0n);

                // expires_at is stamped from the block timestamp plus the configured TTL.
                const now = BigInt((await polkadotJs.query.timestamp.now()).toString());
                expect(record.expires_at).toBe(now + linkedOutputTtl(polkadotJs));
            },
        });

        it({
            id: "T03",
            title: "a v2 order without has_linked_order records nothing",
            test: async () => {
                const plain = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: sourceNetuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(10) },
                    limitPrice: 0n,
                    expiry: uniqueExpiry(),
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    // hasLinkedOrder defaults to false
                });
                const plainId = orderId(polkadotJs, plain.order);

                await devExecuteOrders(polkadotJs, context, bob, [plain], true);

                expect(await getOrderStatus(polkadotJs, plainId)).toBe("Fulfilled");
                // Recording is opt-in and signed: without the flag nothing can link here.
                expect(await getLinkedOutput(polkadotJs, plainId)).toBeUndefined();
            },
        });

        it({
            id: "T04",
            title: "a linked buy drawing part of the proceeds leaves the rest with the signer",
            test: async () => {
                const provider = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: aliceHotKey.address,
                    netuid: sourceNetuid,
                    orderType: "TakeProfit",
                    amount: { Fixed: tao(100) },
                    limitPrice: 0n,
                    expiry: uniqueExpiry(),
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                    hasLinkedOrder: true,
                });
                const providerId = orderId(polkadotJs, provider.order);

                // 30% of the proceeds; the other 70% stays liquid with Alice.
                const linked = buildSignedOrderV2(polkadotJs, {
                    signer: alice,
                    hotkey: secondHotKey.address,
                    netuid: targetNetuid,
                    orderType: "LimitBuy",
                    amount: { LinkedPercentage: { provider: providerId, pct: 300_000_000 } },
                    limitPrice: FAR_FUTURE,
                    expiry: FAR_FUTURE,
                    feeRate: 0,
                    feeRecipient: alice.address,
                    chainId,
                });

                await devExecuteOrders(polkadotJs, context, bob, [provider, linked], true);

                const events = await polkadotJs.query.system.events();
                const recorded = filterEvents(events, "LinkedOutputRecorded");
                const consumed = filterEvents(events, "LinkedOutputConsumed");
                expect(recorded.length).toBe(1);
                expect(consumed.length).toBe(1);

                const total = BigInt(recorded[0].event.data.total.toString());
                const drawn = BigInt(consumed[0].event.data.amount.toString());
                const undrawn = BigInt(consumed[0].event.data.undrawn.toString());

                // pct is applied to the full recorded total, floored.
                expect(drawn).toBe((total * 300_000_000n) / 1_000_000_000n);
                expect(undrawn).toBe(total - drawn);
                expect(undrawn).toBeGreaterThan(0n);

                // The record is single-use whatever fraction was drawn: the remaining 70%
                // was credited to Alice by the sell itself and simply stays there.
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();
            },
        });
    },
});
