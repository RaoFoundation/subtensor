import { describeSuite } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import { hexToU8a } from "@polkadot/util";
import type { PolkadotClient, TypedApi } from "polkadot-api";
import { beforeAll, expect } from "vitest";
import { sleep } from "@zombienet/utils";
import {
    checkRuntime,
    getAccountNonce,
    getBalance,
    getNextKey,
    getSignerFromKeypair,
    submitEncrypted,
    waitForFinalizedBlocks,
} from "../../utils";

type IndexedEvmBlock = {
    hash: string;
    number: string;
};

async function waitForIndexedEvmBlock(client: PolkadotClient, blockNumber: number): Promise<IndexedEvmBlock> {
    const deadline = Date.now() + 60_000;
    const blockTag = `0x${blockNumber.toString(16)}`;

    while (Date.now() < deadline) {
        try {
            const block = (await client._request("eth_getBlockByNumber", [blockTag, false])) as IndexedEvmBlock | null;
            if (block) return block;
        } catch {
            // The node may know the finalized Substrate block just before its
            // asynchronous Frontier mapping is queryable.
        }
        await sleep(1_000);
    }

    throw new Error(`Frontier did not index finalized block #${blockNumber}`);
}

describeSuite({
    id: "01_scaling",
    title: "MEV Shield — 6 node scaling",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let apiFull: TypedApi<typeof subtensor>;
        let client: PolkadotClient;
        let clientFull: PolkadotClient;

        let alice: KeyringPair;
        let bob: KeyringPair;
        let charlie: KeyringPair;
        let t03Sender: KeyringPair;
        let t03Recipient: KeyringPair;

        beforeAll(async () => {
            const keyring = new Keyring({ type: "sr25519" });
            alice = keyring.addFromUri("//Alice");
            bob = keyring.addFromUri("//Bob");
            charlie = keyring.addFromUri("//Charlie");
            t03Sender = keyring.addFromUri("//Dave");
            t03Recipient = keyring.addFromUri("//Eve");

            client = context.papi("Node");
            api = client.getTypedApi(subtensor);
            clientFull = context.papi("NodeFull");
            apiFull = clientFull.getTypedApi(subtensor);

            await checkRuntime(api);
        }, 120000);

        // This shard runs all six read/write cases concurrently. T03 uses
        // Dave/Eve while T04 uses Alice/Bob/Charlie, so the writers never
        // share a nonce or a recipient balance. The key and topology cases
        // are read-only.

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

                const amount = 5_000_000_000n;
                const balanceBefore = await getBalance(api, t03Recipient.address);

                const nonce = await getAccountNonce(api, t03Sender.address);
                const innerTxHex = await api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(t03Recipient.address),
                    value: amount,
                }).sign(getSignerFromKeypair(t03Sender), { nonce: nonce + 1 });

                await submitEncrypted(api, t03Sender, hexToU8a(innerTxHex), nextKey, nonce);

                const balanceAfter = await getBalance(api, t03Recipient.address);
                expect(balanceAfter).toBe(balanceBefore + amount);

                // The state-oriented suites run one immediately-finalized node,
                // so retain explicit GRANDPA propagation and Frontier indexing
                // assertions on the production-like Shield topology. Pin every
                // read to the same finalized block to avoid comparing independently
                // advancing latest-state views.
                const finalizedHash = (await client._request("chain_getFinalizedHead", [])) as string;
                const finalizedNumber = await api.query.System.Number.getValue({ at: finalizedHash });
                const authorityAccount = await api.query.System.Account.getValue(t03Recipient.address, {
                    at: finalizedHash,
                });
                expect(authorityAccount.data.free).toBe(balanceAfter);
                const deadline = Date.now() + 60_000;
                let fullNodeBalance: bigint | undefined;
                while (Date.now() < deadline) {
                    try {
                        const fullAccount = await apiFull.query.System.Account.getValue(t03Recipient.address, {
                            at: finalizedHash,
                        });
                        fullNodeBalance = fullAccount.data.free;
                        break;
                    } catch {
                        await sleep(1_000);
                    }
                }
                expect(fullNodeBalance).toBe(authorityAccount.data.free);

                const [authorityEvmBlock, fullNodeEvmBlock] = await Promise.all([
                    waitForIndexedEvmBlock(client, finalizedNumber),
                    waitForIndexedEvmBlock(clientFull, finalizedNumber),
                ]);
                expect(fullNodeEvmBlock.number).toBe(authorityEvmBlock.number);
                expect(fullNodeEvmBlock.hash).toBe(authorityEvmBlock.hash);
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
                    }).sign(getSignerFromKeypair(sender), { nonce: nonce + 1 });

                    txPromises.push(submitEncrypted(api, sender, hexToU8a(innerTxHex), nextKey, nonce));
                }

                await Promise.all(txPromises);

                const balanceAfter = await getBalance(api, charlie.address);
                expect(balanceAfter).toBe(balanceBefore + BigInt(senders.length) * amount);
            },
        });
    },
});
