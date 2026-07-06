import { describeSuite } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import { hexToU8a } from "@polkadot/util";
import type { PolkadotClient, TypedApi } from "polkadot-api";
import { beforeAll, expect } from "vitest";
import {
    checkRuntime,
    getAccountNonce,
    getBalance,
    getNextKey,
    getSignerFromKeypair,
    submitEncrypted,
    waitForFinalizedBlocks,
} from "../../utils";

describeSuite({
    id: "01_scaling",
    title: "MEV Shield — 6 node scaling",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let client: PolkadotClient;

        let alice: KeyringPair;
        let bob: KeyringPair;
        let charlie: KeyringPair;

        beforeAll(async () => {
            const keyring = new Keyring({ type: "sr25519" });
            alice = keyring.addFromUri("//Alice");
            bob = keyring.addFromUri("//Bob");
            charlie = keyring.addFromUri("//Charlie");

            client = context.papi("Node");
            api = client.getTypedApi(subtensor);

            await checkRuntime(api);
        }, 120000);

        it({
            id: "T01",
            title: "Network scales to 6 nodes with full peering",
            test: async () => {
                // We run 6 nodes: 3 validators and 3 full nodes (5 peers + self)
                expect((await client._request("system_peers", [])).length + 1).toBe(6);

                // Verify the network is healthy by checking finalization continues.
                await waitForFinalizedBlocks(api, 2);
            },
        });

        it({
            id: "T02",
            title: "Key rotation continues with more peers",
            test: async () => {
                const key1 = await getNextKey(api);
                expect(key1).toBeDefined();

                await waitForFinalizedBlocks(api, 2);

                const key2 = await getNextKey(api);
                expect(key2).toBeDefined();
                expect(key2.length).toBe(1184);
            },
        });

        it({
            id: "T03",
            title: "Encrypted tx works with 6 nodes",
            test: async () => {
                const nextKey = await getNextKey(api);
                expect(nextKey).toBeDefined();

                const balanceBefore = await getBalance(api, bob.address);

                const nonce = await getAccountNonce(api, alice.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(bob.address),
                    value: 5_000_000_000n,
                }).sign(getSignerFromKeypair(alice), { nonce: nonce + 1 });

                await submitEncrypted(api, alice, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, bob.address);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
            },
        });

        it({
            id: "T04",
            title: "Multiple encrypted txs in same block with 6 nodes",
            test: async () => {
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
    },
});
