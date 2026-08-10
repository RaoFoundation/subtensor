import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import type { KeyringPair } from "@moonwall/util";
import { tao, generateKeyringPair } from "../../../../utils";
import {
    devForceSetBalance,
    devAddStake,
    devAssociateHotKey,
    devEnableSubtoken,
    devExecuteOrders,
    devPruneLinkedOutput,
    devRegisterSubnet,
    devSudoSetLockReductionInterval,
} from "../../../../utils/dev-helpers.js";
import {
    buildSignedOrderV2,
    FAR_FUTURE,
    fetchChainId,
    filterEvents,
    expectLinkedOutput,
    getLinkedOutput,
    linkedOutputTtl,
    orderId,
    registerLimitOrderTypes,
} from "../../../../utils/limit-orders.js";

// `prune_linked_output` is how a provider record gets reclaimed when the linked order
// never fires.  Two callers are allowed: the record's own signer at any time, and
// anyone once `expires_at` has passed.
//
// The expiry branch is NOT exercisable here — `LinkedOutputTtl` is 180 days in the
// runtime and a dev chain cannot plausibly be advanced that far.  It is covered by
// `a_stranger_can_only_prune_after_expiry` in the Rust unit suite, where `MockTime`
// can be set directly.  What this file pins is the signer branch, the refusal for
// everyone else, and the fact that pruning revokes the authorisation.

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_V2_PRUNE",
    title: "limit-orders v2 — prune_linked_output",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;
        let alice: KeyringPair;
        let aliceHotKey: KeyringPair;
        let bob: KeyringPair;
        let netuid: number;
        let chainId: bigint;

        beforeAll(async () => {
            polkadotJs = context.polkadotJs();
            alice = context.keyring.alice;
            bob = context.keyring.bob;
            aliceHotKey = generateKeyringPair("sr25519");

            registerLimitOrderTypes(polkadotJs);
            chainId = await fetchChainId(polkadotJs);

            await devForceSetBalance(polkadotJs, context, alice.address, tao(10_000));
            await devForceSetBalance(polkadotJs, context, bob.address, tao(10_000));
            await devSudoSetLockReductionInterval(polkadotJs, context, alice, 1);

            netuid = await devRegisterSubnet(polkadotJs, context, alice, aliceHotKey);
            await devEnableSubtoken(polkadotJs, context, alice, netuid);
            await devAssociateHotKey(polkadotJs, context, alice, aliceHotKey.address);

            await devAddStake(polkadotJs, context, alice, aliceHotKey.address, netuid, tao(1000));
        });

        // An `order_id` is `blake2_256(SCALE(payload))`: no nonce, and the signature is
        // not part of the preimage.  Every `it` in this file shares one chain, so two
        // tests that build the same payload collide on `order_id` and the second
        // execution is refused `OrderAlreadyProcessed` — quietly, because the earlier
        // test's record is still sitting at that key and satisfies a "the record exists"
        // assertion.  Salting the expiry keeps every payload distinct; nothing here
        // asserts on expiry beyond it being in the future.
        let salt = 0n;
        function uniqueExpiry(): bigint {
            salt += 1n;
            return FAR_FUTURE - salt;
        }

        /**
         * Execute a sell provider and return its OrderId.
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

        it({
            id: "T01",
            title: "an account that is not the signer cannot prune before expiry",
            test: async () => {
                const providerId = await executeProvider(tao(50));

                // The record's TTL is far in the future, so the open-to-anyone branch is
                // closed and Bob has no standing.
                const record = await expectLinkedOutput(polkadotJs, providerId);
                const now = BigInt((await polkadotJs.query.timestamp.now()).toString());
                expect(record.expires_at).toBeGreaterThan(now);
                expect(linkedOutputTtl(polkadotJs)).toBeGreaterThan(0n);

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.pruneLinkedOutput(providerId).signAsync(bob),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("LinkedOutputNotPrunable");
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeDefined();
            },
        });

        it({
            id: "T02",
            title: "the signer can prune their own record at any time, and it revokes the link",
            test: async () => {
                const providerId = await executeProvider(tao(50));
                const total = (await expectLinkedOutput(polkadotJs, providerId)).total;

                await devPruneLinkedOutput(polkadotJs, context, alice, providerId);

                const events = await polkadotJs.query.system.events();
                const pruned = filterEvents(events, "LinkedOutputPruned");
                expect(pruned.length).toBe(1);
                expect(pruned[0].event.data.orderId.toString()).toBe(providerId);
                // The event reports what was never claimed. It stays with the signer —
                // pruning moves no funds, it only withdraws the authorisation.
                expect(BigInt(pruned[0].event.data.total.toString())).toBe(total);

                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();

                // A linked order signed against the pruned provider is now unexecutable.
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
                });

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
            title: "pruning an absent record fails NoLinkedOutput",
            test: async () => {
                const missing = `0x${"42".repeat(32)}` as `0x${string}`;

                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.pruneLinkedOutput(missing).signAsync(alice),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NoLinkedOutput");
            },
        });

        it({
            id: "T04",
            title: "a drawn-from record is already gone and cannot be pruned again",
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
                });
                await devExecuteOrders(polkadotJs, context, bob, [linked], true);
                expect(await getLinkedOutput(polkadotJs, providerId)).toBeUndefined();

                // Consuming and pruning are the same removal, so there is nothing left.
                const {
                    result: [attempt],
                } = await context.createBlock([
                    await polkadotJs.tx.limitOrders.pruneLinkedOutput(providerId).signAsync(alice),
                ]);

                expect(attempt.successful).toEqual(false);
                expect(attempt.error.name).toEqual("NoLinkedOutput");
            },
        });
    },
});
