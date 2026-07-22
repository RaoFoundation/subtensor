import { expect, beforeAll, describeSuite } from "@moonwall/cli";
import {
    checkRuntime,
    encryptTransaction,
    getAccountNonce,
    getBalance,
    getNextKey,
    getSignerFromKeypair,
    submitEncrypted,
    waitForFinalizedBlocks,
} from "../../utils";
import { Binary } from "@polkadot-api/substrate-bindings";
import { Keyring } from "@polkadot/keyring";
import type { KeyringPair } from "@moonwall/util";
import { hexToU8a } from "@polkadot/util";
import { subtensor, MultiAddress } from "@polkadot-api/descriptors";
import type { PolkadotClient, TypedApi } from "polkadot-api";

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
        let dave: KeyringPair;
        let eve: KeyringPair;
        let ferdie: KeyringPair;
        let one: KeyringPair;
        let two: KeyringPair;
        let t04Recipient: KeyringPair;

        beforeAll(async () => {
            client = context.papi("Node");
            api = client.getTypedApi(subtensor);

            const keyring = new Keyring({ type: "sr25519" });
            alice = keyring.addFromUri("//Alice");
            bob = keyring.addFromUri("//Bob");
            charlie = keyring.addFromUri("//Charlie");
            dave = keyring.addFromUri("//Dave");
            eve = keyring.addFromUri("//Eve");
            ferdie = keyring.addFromUri("//Ferdie");
            one = keyring.addFromUri("//One");
            two = keyring.addFromUri("//Two");
            t04Recipient = keyring.addFromUri("//ShieldT04Recipient");

            await checkRuntime(api);

            await waitForFinalizedBlocks(api, 3);
        }, 120000);

        // The environment runs these cases concurrently. Each case has a
        // distinct sender nonce and successful recipient; the rejected T05/T06
        // calls share Dave only because neither is allowed to change his balance.

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

                const balanceBefore = await getBalance(api, dave.address);

                // Encrypt a transfer of more than Charlie has.
                // The wrapper is valid (correct key_hash, valid encryption), but the
                // inner transfer should fail at dispatch with InsufficientBalance.
                const nonce = await getAccountNonce(api, charlie.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(dave.address),
                    value: 9_000_000_000_000_000_000n,
                }).sign(getSignerFromKeypair(charlie), { nonce: nonce + 1 });

                await submitEncrypted(api, charlie, hexToU8a(innerTxHex), nextKey, nonce);

                // The inner transfer failed, so Dave's balance should not increase.
                const balanceAfter = await getBalance(api, dave.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });

        it({
            id: "T03",
            title: "Malformed ciphertext is rejected at pool level",
            test: async () => {
                const nonce = await getAccountNonce(api, eve.address);

                // 5 bytes of garbage — not valid ciphertext at all.
                const garbage = new Uint8Array([0x01, 0x02, 0x03, 0x04, 0x05]);

                const tx = api.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(garbage),
                });

                // Pool validation rejects with FailedShieldedTxParsing (Custom code 23).
                await expect(
                    tx.signAndSubmit(getSignerFromKeypair(eve), { nonce, mortality: { mortal: true, period: 8 } })
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

                const balanceBefore = await getBalance(api, t04Recipient.address);

                const senders = [ferdie, one];
                const amount = 1_000_000_000n;
                const txPromises = [];

                for (const sender of senders) {
                    const nonce = await getAccountNonce(api, sender.address);

                    const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                        dest: MultiAddress.Id(t04Recipient.address),
                        value: amount,
                    }).sign(getSignerFromKeypair(sender), { nonce: nonce + 1 });

                    txPromises.push(submitEncrypted(api, sender, hexToU8a(innerTxHex), nextKey, nonce));
                }

                await Promise.all(txPromises);

                const balanceAfter = await getBalance(api, t04Recipient.address);
                expect(balanceAfter).toBe(balanceBefore + amount * BigInt(senders.length));
            },
        });

        it({
            id: "T05",
            title: "Wrong key hash is not included by the block proposer",
            test: async () => {
                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, dave.address);

                const nonce = await getAccountNonce(api, bob.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(dave.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(bob), { nonce: nonce + 1 });

                const ciphertext = await encryptTransaction(hexToU8a(innerTxHex), nextKey!);

                // Tamper the first 16 bytes (key_hash).
                const tampered = new Uint8Array(ciphertext);
                for (let i = 0; i < 16; i++) tampered[i] = 0xff;

                const tx = api.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(tampered),
                });
                const signedHex = await tx.sign(getSignerFromKeypair(bob), {
                    nonce,
                    mortality: { mortal: true, period: 8 },
                });
                // Send without waiting — the tx enters the pool but the block
                // proposer will skip it because the key_hash doesn't match.
                client.submit(signedHex).catch(() => {});

                await waitForFinalizedBlocks(api, 3);

                // The inner transfer should NOT have executed.
                const balanceAfter = await getBalance(api, dave.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });

        it({
            id: "T06",
            title: "Stale key is not included after rotation",
            test: async () => {
                const staleKey = await getNextKey(api);
                expect(staleKey).toBeDefined();

                // Wait for enough blocks that the key has rotated past both
                // currentKey and nextKey positions.
                await waitForFinalizedBlocks(api, 3);

                const balanceBefore = await getBalance(api, dave.address);

                const nonce = await getAccountNonce(api, two.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(dave.address),
                    value: 1_000_000_000n,
                }).sign(getSignerFromKeypair(two), { nonce: nonce + 1 });

                const ciphertext = await encryptTransaction(hexToU8a(innerTxHex), staleKey!);

                const tx = api.tx.MevShield.submit_encrypted({
                    ciphertext: Binary.fromBytes(ciphertext),
                });
                const signedHex = await tx.sign(getSignerFromKeypair(two), {
                    nonce,
                    mortality: { mortal: true, period: 8 },
                });
                // Send without waiting — the block proposer will reject because
                // key_hash no longer matches currentKey or nextKey.
                client.submit(signedHex).catch(() => {});

                await waitForFinalizedBlocks(api, 3);

                // The inner transfer should NOT have executed.
                const balanceAfter = await getBalance(api, dave.address);
                expect(balanceAfter).toBe(balanceBefore);
            },
        });
    },
});
