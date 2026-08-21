/**
 * R01 — the product, end to end against the live rig (`just rails-up`
 * first): buy CHUTES with USDC on fake Base, watch the rebasing balance
 * grow as the hub escrow earns emissions, and sell back to USDC. This is
 * exactly what the demo page does, minus MetaMask.
 */
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import { type RailsRig, connectRig, waitForReceipt, waitUntil } from "../../utils/rails.js";

describeSuite({
    id: "R01",
    title: "Rails buy/sell: USDC to CHUTES and back, index accrues",
    foundationMethods: "read_only",
    testCases: ({ it, context, log }) => {
        let api: ApiPromise;
        let rig: RailsRig;
        let netuid: number;
        let buyer: string;

        async function attestation(): Promise<{
            escrowed_alpha: number;
            shares_outstanding: number;
            index_e9: number;
        }> {
            const response = await fetch(rig.manifest.chains.btlocal.rpcHttp, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    jsonrpc: "2.0",
                    id: 1,
                    method: "rails_alphaAttestation",
                    params: [netuid],
                }),
            });
            const json = (await response.json()) as {
                result?: { escrowed_alpha: number; shares_outstanding: number; index_e9: number };
                error?: { message: string };
            };
            if (!json.result) {
                throw new Error(`rails_alphaAttestation failed: ${JSON.stringify(json.error)}`);
            }
            return json.result;
        }

        /** Pull the assigned envelope nonce out of a portal/chutes event. */
        function eventArg(receipt: any, contract: any, eventName: string, arg: string): bigint {
            for (const logEntry of receipt.logs) {
                try {
                    const parsed = contract.interface.parseLog(logEntry);
                    if (parsed?.name === eventName) {
                        return BigInt(parsed.args[arg]);
                    }
                } catch {
                    // Not this contract's event.
                }
            }
            throw new Error(`no ${eventName} event in receipt`);
        }

        beforeAll(() => {
            api = context.polkadotJs();
            rig = connectRig();
            netuid = rig.manifest.chains.btlocal.netuid;
            buyer = rig.baseDeployer.address;
            log(`demo netuid=${netuid}, buyer=${buyer}`);
        });

        it({
            id: "C01",
            title: "buy: one approve + one buy() mints CHUTES backed by staked alpha",
            test: async () => {
                const amount = 100n * 10n ** 9n; // 100 USDC
                const sharesBefore = BigInt(await rig.chutes.sharesOf(buyer));
                const escrowBefore = BigInt((await attestation()).escrowed_alpha);

                await (await rig.mockUsdc.mint(buyer, amount)).wait();
                await (await rig.mockUsdc.approve(await rig.portal.getAddress(), amount)).wait();
                const receipt = await (await rig.portal.buy(amount, netuid, 1n)).wait();
                const nonce = eventArg(receipt, rig.portal, "Bought", "nonce");
                log(`buy dispatched (nonce ${nonce}), waiting for hub execution...`);

                const hubReceipt = await waitForReceipt(api, nonce, 180_000);
                expect(hubReceipt.fallback).toBeNull();

                // Real staked alpha now backs the position.
                const view = await attestation();
                expect(BigInt(view.escrowed_alpha)).toBeGreaterThan(escrowBefore);

                // The share mint flows back to Base and lands in the wallet.
                const sharesAfter = await waitUntil(
                    async () => {
                        const shares = BigInt(await rig.chutes.sharesOf(buyer));
                        return shares > sharesBefore ? shares : undefined;
                    },
                    "CHUTES mint on Base",
                    180_000
                );
                const balance = BigInt(await rig.chutes.balanceOf(buyer));
                log(`shares ${sharesBefore} -> ${sharesAfter}, display balance ${balance}`);
                expect(balance).toBeGreaterThan(0n);
            },
        });

        it({
            id: "C02",
            title: "accrue: the heartbeat pushes a rising index and the balance ticks up",
            test: async () => {
                const indexBefore = BigInt(await rig.chutes.indexE9());
                const balanceBefore = BigInt(await rig.chutes.balanceOf(buyer));
                const sharesBefore = BigInt(await rig.chutes.sharesOf(buyer));

                // No transaction from the user: emissions on the hub raise
                // alpha-per-share, the heartbeat pushes it, balanceOf grows.
                const indexAfter = await waitUntil(
                    async () => {
                        const index = BigInt(await rig.chutes.indexE9());
                        return index > indexBefore ? index : undefined;
                    },
                    "index heartbeat on Base",
                    240_000,
                    2_000
                );
                const balanceAfter = BigInt(await rig.chutes.balanceOf(buyer));
                log(`index ${indexBefore} -> ${indexAfter}, balance ${balanceBefore} -> ${balanceAfter}`);
                expect(balanceAfter).toBeGreaterThan(balanceBefore);
                // Shares never moved; only the index did.
                expect(BigInt(await rig.chutes.sharesOf(buyer))).toBe(sharesBefore);
            },
        });

        it({
            id: "C03",
            title: "attestation: hub escrow covers remote share supply at the index",
            test: async () => {
                const view = await attestation();
                log(`attestation: ${JSON.stringify(view)}`);
                expect(BigInt(view.shares_outstanding)).toBe(BigInt(await rig.chutes.totalShares()));
                // Escrow backs shares at the index: escrow >= shares * index.
                const backed =
                    (BigInt(view.shares_outstanding) * BigInt(view.index_e9)) / 10n ** 9n;
                expect(BigInt(view.escrowed_alpha)).toBeGreaterThanOrEqual(backed);
            },
        });

        it({
            id: "C04",
            title: "sell: burning CHUTES unstakes escrow and releases USDC on Base",
            test: async () => {
                const shares = BigInt(await rig.chutes.sharesOf(buyer)) / 2n;
                expect(shares).toBeGreaterThan(0n);
                const usdcBefore = BigInt(await rig.mockUsdc.balanceOf(buyer));
                const escrowBefore = BigInt((await attestation()).escrowed_alpha);

                const receipt = await (await rig.chutes.sell(shares, 1n)).wait();
                const nonce = eventArg(receipt, rig.chutes, "SoldToHub", "nonce");
                log(`sell dispatched (nonce ${nonce}), waiting for hub execution...`);

                const hubReceipt = await waitForReceipt(api, nonce, 180_000);
                expect(hubReceipt.fallback).toBeNull();

                const view = await attestation();
                expect(BigInt(view.escrowed_alpha)).toBeLessThan(escrowBefore);
                expect(BigInt(view.shares_outstanding)).toBe(BigInt(await rig.chutes.totalShares()));

                const usdcAfter = await waitUntil(
                    async () => {
                        const balance = BigInt(await rig.mockUsdc.balanceOf(buyer));
                        return balance > usdcBefore ? balance : undefined;
                    },
                    "USDC release on Base",
                    180_000
                );
                log(`USDC ${usdcBefore} -> ${usdcAfter}`);
            },
        });

        it({
            id: "C05",
            title: "nonce sync: the portal counter matches the hub's NextNonce",
            test: async () => {
                const response = await fetch(rig.manifest.chains.btlocal.rpcHttp, {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "rails_hubInfo", params: [] }),
                });
                const json = (await response.json()) as { result?: { next_nonce: number } };
                expect(json.result).toBeDefined();
                const portalNext = BigInt(await rig.portal.nextNonce());
                log(`portal nextNonce=${portalNext}, hub next_nonce=${json.result?.next_nonce}`);
                expect(BigInt(json.result?.next_nonce ?? -1)).toBe(portalNext);
            },
        });
    },
});
