import { hexToBytes as hexToU8a } from "@bittensor/sdk";
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import { Binary } from "@polkadot-api/substrate-bindings";
import type { PolkadotClient, TypedApi } from "polkadot-api";
import {
    checkRuntime,
    encryptTransaction,
    getAccountNonce,
    getBalance,
    getNextKey,
    getSignerFromKeypair,
    keyringPairFromUri,
    submitEncrypted,
    waitForFinalizedBlocks,
} from "../../utils";

describeSuite({
    id: "00.01_basic",
    title: "MEV Shield — encrypted transactions",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let client: PolkadotClient;
        let api: TypedApi<typeof subtensor>;
        let alice: KeyringPair;
        let bob: KeyringPair;
        let charlie: KeyringPair;

        beforeAll(async () => {
            client = context.papi("Node");
            api = client.getTypedApi(subtensor);

            alice = keyringPairFromUri("//Alice");
            bob = keyringPairFromUri("//Bob");
            charlie = keyringPairFromUri("//Charlie");

            await checkRuntime(api);

            await waitForFinalizedBlocks(api, 3);
        }, 120000);

        it({
            id: "T01",
            title: "Happy path: wrapper and inner tx are included in the same block",
            test: async () => {
                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, bob.address);

                const nonce = await getAccountNonce(api, alice.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 10_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                await submitEncrypted(api, alice, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, bob.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });

        it({
            id: "T02",
            title: "Failed inner tx: wrapper succeeds but inner transfer has no effect",
            test: async () => {
                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, bob.address);

                // Encrypt a transfer of more than Alice has.
                // The wrapper is valid (correct key_hash, valid encryption), but the
                // inner transfer should fail at dispatch with InsufficientBalance.
                const nonce = await getAccountNonce(api, alice.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 9_000_000_000_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                await submitEncrypted(api, alice, hexToU8a(innerTxHex), nextKey, nonce);

                // The inner transfer failed, so bob's balance should not increase.
                const balanceAfter = await getBalance(api, bob.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });

        it({
            id: "T03",
            title: "Malformed ciphertext is rejected at pool level",
            test: async () => {
                const nonce = await getAccountNonce(api, alice.address);

                // 5 bytes of garbage — not valid ciphertext at all.
                const garbage = new Uint8Array([0x01, 0x02, 0x03, 0x04, 0x05]);

                const tx = api.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(garbage),
                });

                // Pool validation rejects with FailedShieldedTxParsing (Custom code 23).
                await expect(
                    tx.signAndSubmit(getSignerFromKeypair(alice), { nonce, mortality: { mortal: true, period: 8 } })
                ).rejects.toThrow();
            },
        });

        it({
            id: "T04",
            title: "Multiple encrypted txs in same block",
            test: async () => {
                // Use different signers to avoid nonce ordering issues between
                // the outer wrappers and decrypted inner transactions.
                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, charlie.address);

                const senders = [alice, bob];
                const amount = 1_000_000_000n;
                const txPromises = [];

                for (const sender of senders) {
                    const nonce = await getAccountNonce(api, sender.address);

                    const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                        dest: MultiAddress.Id(charlie.address),
                        value: amount,
                    }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                    txPromises.push(submitEncrypted(api, sender, hexToU8a(innerTxHex), nextKey, nonce));
                }

                await Promise.all(txPromises);

                const balanceAfter = await getBalance(api, charlie.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });

        it({
            id: "T05",
            title: "Wrong key hash is not included by the block proposer",
            test: async () => {
                const nextKey = await getNextKey(api);
                if (nextKey === undefined) throw new Error("MEV Shield NextKey is not available");

                const balanceBefore = await getBalance(api, bob.address);

                const nonce = await getAccountNonce(api, alice.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                const ciphertext = await encryptTransaction(hexToU8a(innerTxHex), nextKey);

                // Tamper the first 16 bytes (key_hash).
                const tampered = new Uint8Array(ciphertext);
                for (let i = 0; i < 16; i++) tampered[i] = 0xff;

                const tx = api.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(tampered),
                });
                const signedHex = await tx.sign(getSignerFromKeypair(alice), {
                    nonce,
                    mortality: { mortal: true, period: 8 },
                });
                // Send without waiting — the tx enters the pool but the block
                // proposer will skip it because the key_hash doesn't match.
                client.submit(signedHex).catch(() => {});

                await waitForFinalizedBlocks(api, 3);

                // The inner transfer should NOT have executed.
                const balanceAfter = await getBalance(api, bob.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });

        it({
            id: "T06",
            title: "Stale key is not included after rotation",
            test: async () => {
                const staleKey = await getNextKey(api);
                if (staleKey === undefined) throw new Error("MEV Shield NextKey is not available");

                // Wait for enough blocks that the key has rotated past both
                // currentKey and nextKey positions.
                await waitForFinalizedBlocks(api, 3);

                const balanceBefore = await getBalance(api, bob.address);

                const nonce = await getAccountNonce(api, alice.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                const ciphertext = await encryptTransaction(hexToU8a(innerTxHex), staleKey);

                const tx = api.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(ciphertext),
                });
                const signedHex = await tx.sign(getSignerFromKeypair(alice), {
                    nonce,
                    mortality: { mortal: true, period: 8 },
                });
                // Send without waiting — the block proposer will reject because
                // key_hash no longer matches currentKey or nextKey.
                client.submit(signedHex).catch(() => {});

                await waitForFinalizedBlocks(api, 3);

                // The inner transfer should NOT have executed.
                const balanceAfter = await getBalance(api, bob.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });
    },
});
