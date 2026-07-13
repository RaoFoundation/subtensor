import { hexToBytes as hexToU8a } from "@bittensor/sdk";
import { describeSuite } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import { sleep } from "@zombienet/utils";
import type { PolkadotClient, TypedApi } from "polkadot-api";
import { Binary } from "polkadot-api";
import { beforeAll, expect } from "vitest";
import {
    checkRuntime,
    encryptTransaction,
    getAccountNonce,
    getBalance,
    getNextKey,
    getSignerFromKeypair,
    keyringPairFromUri,
    waitForFinalizedBlocks,
} from "../../utils";

// MAX_SHIELD_ERA_PERIOD is 8 blocks. With 12s slots, that's ~96s.
const MAX_ERA_BLOCKS = 8;
const SLOT_DURATION_MS = 12_000;
const POLL_INTERVAL_MS = 3_000;

describeSuite({
    id: "04_mortality",
    title: "MEV Shield — mortality eviction",
    foundationMethods: "zombie",
    testCases: ({ it, context, log }) => {
        let apiAuthority: TypedApi<typeof subtensor>;

        let apiFull: TypedApi<typeof subtensor>;
        let clientFull: PolkadotClient;

        let alice: KeyringPair;
        let bob: KeyringPair;

        beforeAll(
            async () => {
                alice = keyringPairFromUri("//Alice");
                bob = keyringPairFromUri("//Bob");

                apiAuthority = context.papi("Node").getTypedApi(subtensor);

                clientFull = context.papi("NodeFull");
                apiFull = clientFull.getTypedApi(subtensor);

                await checkRuntime(apiAuthority);

                // Wait for a fresh finalized block, then immediately read NextKey and submit.
                // This tests the "just after block" boundary where keys just rotated.
                await waitForFinalizedBlocks(apiAuthority, 1);
            },
            (MAX_ERA_BLOCKS + 8) * SLOT_DURATION_MS
        );

        it({
            id: "T01",
            title: "Tx with tampered key_hash submitted to non-authority is evicted within mortality window",
            test: async () => {
                // Read a valid NextKey from an authority node, encrypt a real inner tx.
                const nextKey = await getNextKey(apiAuthority);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(apiFull, bob.address);

                const nonce = await getAccountNonce(apiFull, alice.address);
                const innerTxHex = await apiFull.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                // Encrypt with valid key, then tamper the key_hash so no proposer will include it.
                const ciphertext = await encryptTransaction(hexToU8a(innerTxHex), nextKey);
                const tampered = new Uint8Array(ciphertext);
                for (let i = 0; i < 16; i++) tampered[i] = 0xff;

                const tx = apiFull.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(tampered),
                });

                // Sign with short mortality (must be ≤ MAX_SHIELD_ERA_PERIOD=8 to pass
                // CheckMortality validation). The tx enters the pool but no proposer
                // will include it (tampered key_hash doesn't match PendingKey).
                const signedHex = await tx.sign(getSignerFromKeypair(alice), {
                    nonce,
                    mortality: { mortal: true, period: 8 },
                });

                // Submit via raw RPC to get immediate feedback on pool acceptance.
                let txHash: string;
                try {
                    txHash = await clientFull._request("author_submitExtrinsic", [signedHex]);
                    log(`Tx submitted successfully, hash: ${txHash}`);
                } catch (err: unknown) {
                    throw new Error(`Tx rejected at pool entry: ${err}`);
                }

                // Verify it's in the pool.
                await sleep(1_000);
                const normalizedTx = signedHex.toLowerCase();
                const pending: string[] = await clientFull._request("author_pendingExtrinsics", []);
                const inPool = pending.some((hex) => hex.toLowerCase() === normalizedTx);
                log(`Pool has ${pending.length} pending tx(s), our tx ${inPool ? "found" : "NOT found"}`);
                expect(inPool).toBe(true);

                // Now poll until our specific tx disappears (mortality eviction).
                // Use a generous timeout — CI zombienet nodes can miss AURA slots,
                // so N blocks may take significantly longer than N * 12s.
                const start = Date.now();
                const maxPollMs = MAX_ERA_BLOCKS * 3 * SLOT_DURATION_MS;
                let evicted = false;

                log(`Waiting for mortality eviction (up to ${maxPollMs / 1000}s)...`);

                while (Date.now() - start < maxPollMs) {
                    await sleep(POLL_INTERVAL_MS);

                    const pending: string[] = await clientFull._request("author_pendingExtrinsics", []);
                    const stillPending = pending.some((hex) => hex.toLowerCase() === normalizedTx);

                    if (!stillPending) {
                        evicted = true;
                        break;
                    }
                }

                const elapsed = Date.now() - start;
                log(`Tx ${evicted ? "evicted" : "still in pool"} after ${(elapsed / 1000).toFixed(1)}s`);

                expect(evicted).toBe(true);

                // The inner transfer should NOT have executed.
                const balanceAfter = await getBalance(apiFull, bob.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });
    },
});
