import { hexToBytes as hexToU8a } from "@bittensor/sdk";
import { describeSuite } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import { sleep } from "@zombienet/utils";
import type { TypedApi } from "polkadot-api";
import { beforeAll, expect } from "vitest";
import {
    checkRuntime,
    getAccountNonce,
    getBalance,
    getNextKey,
    getSignerFromKeypair,
    fundShieldAccounts,
    shieldAccount,
    submitEncrypted,
    waitForFinalizedBlocks,
} from "../../utils";

describeSuite({
    id: "03_timing",
    title: "MEV Shield — timing boundaries",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;

        let alice: KeyringPair;
        let bob: KeyringPair;
        let charlie: KeyringPair;
        let dave: KeyringPair;
        let eve: KeyringPair;
        let ferdie: KeyringPair;
        let one: KeyringPair;
        let two: KeyringPair;

        beforeAll(async () => {
            alice = shieldAccount("timing", "Alice");
            bob = shieldAccount("timing", "Bob");
            charlie = shieldAccount("timing", "Charlie");
            dave = shieldAccount("timing", "Dave");
            eve = shieldAccount("timing", "Eve");
            ferdie = shieldAccount("timing", "Ferdie");
            one = shieldAccount("timing", "One");
            two = shieldAccount("timing", "Two");

            api = context.papi("Node").getTypedApi(subtensor);

            await checkRuntime(api);
            await fundShieldAccounts(api, "timing", [alice, bob, charlie, dave, eve, ferdie, one, two]);
        }, 120000);

        // This environment runs the four cases concurrently. Every case uses a
        // distinct pre-funded sender and recipient so nonce and balance state
        // remain independent while their production-time finality waits overlap.

        it({
            id: "T01",
            title: "Submit immediately after a new block",
            test: async () => {
                // Wait for a fresh finalized block, then immediately read NextKey and submit.
                // This tests the "just after block" boundary where keys just rotated.
                await waitForFinalizedBlocks(api, 1);

                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, bob.address);

                const nonce = await getAccountNonce(api, alice.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                await submitEncrypted(api, alice, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, bob.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });

        it({
            id: "T02",
            title: "Submit mid-block (~6s after block)",
            test: async () => {
                // Wait for a block, then sleep 6s (half of 12s slot) before submitting.
                // The key should still be valid — the same NextKey applies until the next block.
                await waitForFinalizedBlocks(api, 1);
                await sleep(6_000);

                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, dave.address);

                const nonce = await getAccountNonce(api, charlie.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(dave.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(charlie), { nonce: nonce + 1 });

                await submitEncrypted(api, charlie, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, dave.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });

        it({
            id: "T03",
            title: "Submit just before next block (~11s after block)",
            test: async () => {
                // Wait for a block, then sleep ~11s to submit right before the next slot.
                // The tx enters the pool just as the next block is about to be produced.
                // It should still be included because the N+2 author hasn't changed yet,
                // and PendingKey will match on the next block's proposer check.
                await waitForFinalizedBlocks(api, 1);
                await sleep(11_000);

                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, ferdie.address);

                const nonce = await getAccountNonce(api, eve.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(ferdie.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(eve), { nonce: nonce + 1 });

                await submitEncrypted(api, eve, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, ferdie.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });

        it({
            id: "T04",
            title: "Read key, wait full slot (12s), then submit",
            test: async () => {
                // Read NextKey, wait a full slot duration, then submit.
                // After one full slot, the key rotates: old NextKey becomes PendingKey.
                // The tx should still be included by the target N+2 author.
                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                await sleep(12_000);

                const balanceBefore = await getBalance(api, two.address);

                const nonce = await getAccountNonce(api, one.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(two.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(one), { nonce: nonce + 1 });

                await submitEncrypted(api, one, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, two.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });
    },
});
