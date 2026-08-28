/**
 * Failure drills against the live rig: buy fallback to tUSD when the action
 * cannot run, revoked minter bounce + recovery, and relayer outage
 * (InTransit) + recovery. Nonces are portal-assigned and sequential; each
 * drill reads its assigned nonce from the portal's Deposited/Bought event.
 */
import { execSync } from "node:child_process";
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import { Keyring } from "@polkadot/keyring";
import type { KeyringPair } from "@polkadot/keyring/types";
import {
    type EnvelopeInput,
    type RailsRig,
    connectRig,
    encodeEnvelope,
    h160ToSS58,
    waitForReceipt,
} from "../../utils/rails.js";

describeSuite({
    id: "R03",
    title: "Rails failure drills: fallback, revocation, outage",
    foundationMethods: "read_only",
    testCases: ({ it, context, log }) => {
        let api: ApiPromise;
        let rig: RailsRig;

        async function tusdOf(address: string): Promise<bigint> {
            return BigInt((await api.query.usdPsm.tUsdBalances(address)).toPrimitive() as number | string);
        }

        /**
         * Lock USD on Base through the generic deposit door. The portal
         * appends its own sequential nonce, so we encode with nonce 0, strip
         * the trailing 8 nonce bytes, and read the assigned nonce back from
         * the Deposited event.
         */
        async function depositFromBase(input: Omit<EnvelopeInput, "nonce">): Promise<bigint> {
            const wire = encodeEnvelope({ ...input, nonce: 0n });
            const prefix = wire.slice(0, wire.length - 16);
            await (await rig.mockUsdc.mint(rig.baseDeployer.address, input.amount)).wait();
            await (await rig.mockUsdc.approve(await rig.portal.getAddress(), input.amount)).wait();
            const receipt = await (await rig.portal.deposit(input.amount, prefix)).wait();
            for (const logEntry of receipt.logs) {
                try {
                    const parsed = rig.portal.interface.parseLog(logEntry);
                    if (parsed?.name === "Deposited") {
                        return BigInt(parsed.args.nonce);
                    }
                } catch {
                    // Not a portal event.
                }
            }
            throw new Error("no Deposited event");
        }

        function freshUser(label: string): KeyringPair {
            return new Keyring({ type: "sr25519" }).addFromUri(`//RailsFail/${label}/${Date.now()}`);
        }

        beforeAll(() => {
            api = context.polkadotJs();
            rig = connectRig();
        });

        it({
            id: "C01",
            title: "failed stake action falls back to a tUSD credit",
            test: async () => {
                const user = freshUser("fallback");
                const amount = 50n * 10n ** 9n;
                const bogusHotkey = new Keyring({ type: "sr25519" }).addFromUri("//RailsNobody");

                const nonce = await depositFromBase({
                    amount,
                    dest: user.address,
                    // Netuid 20000 does not exist: the stake leg must fail.
                    action: { kind: "stake", netuid: 20000, hotkey: bogusHotkey.address, minAlpha: 1n },
                });

                const receipt = await waitForReceipt(api, nonce);
                expect(receipt.fallback).toBe("StakeFailed");
                // Funds are not lost: the deposit stayed as tUSD.
                expect(await tusdOf(user.address)).toBe(amount);
            },
        });

        it({
            id: "C02",
            title: "failed buy falls back to a mirror tUSD credit, never a share mint",
            test: async () => {
                const amount = 20n * 10n ** 9n;
                const buyer = rig.baseDeployer.address;
                const sharesBefore = BigInt(await rig.chutes.sharesOf(buyer));
                const mirror = h160ToSS58(buyer);
                const mirrorTusdBefore = await tusdOf(mirror);

                // Netuid 20001 has no escrow hotkey or route: BuyShares must
                // fail on the hub and credit the buyer's EVM mirror instead.
                await (await rig.mockUsdc.mint(buyer, amount)).wait();
                await (await rig.mockUsdc.approve(await rig.portal.getAddress(), amount)).wait();
                const receipt = await (await rig.portal.buy(amount, 20001, 1n)).wait();
                let nonce = -1n;
                for (const logEntry of receipt.logs) {
                    try {
                        const parsed = rig.portal.interface.parseLog(logEntry);
                        if (parsed?.name === "Bought") {
                            nonce = BigInt(parsed.args.nonce);
                        }
                    } catch {
                        // Not a portal event.
                    }
                }
                expect(nonce).toBeGreaterThanOrEqual(0n);

                const hubReceipt = await waitForReceipt(api, nonce);
                expect(hubReceipt.fallback).toBe("BuyFailed");
                // No shares were minted; the funds landed as tUSD on the
                // buyer's EVM-mirror account instead.
                expect(BigInt(await rig.chutes.sharesOf(buyer))).toBe(sharesBefore);
                expect(await tusdOf(mirror)).toBe(mirrorTusdBefore + amount);
            },
        });

        it({
            id: "C03",
            title: "revoked minter bounces the deposit; re-granting recovers it",
            test: async () => {
                const user = freshUser("revoked");
                const amount = 25n * 10n ** 9n;
                const gatewayAddr = rig.manifest.chains.btlocal.gateway;

                await (await rig.canonicalUsd.removeMinter(gatewayAddr)).wait();
                let nonce: bigint;
                try {
                    nonce = await depositFromBase({
                        amount,
                        dest: user.address,
                        action: { kind: "credit" },
                    });

                    // The backing mint reverts, so delivery cannot complete.
                    await new Promise((resolve) => setTimeout(resolve, 20_000));
                    const processed = (await api.query.usdPsm.processedNonces(nonce)).toJSON();
                    expect(processed).toBeNull();
                    expect(await tusdOf(user.address)).toBe(0n);
                } finally {
                    // Restore mint rights: 1M USD window, 100/s refill.
                    await (
                        await rig.canonicalUsd.setMinterLimits(gatewayAddr, 1_000_000n * 10n ** 9n, 100n * 10n ** 9n)
                    ).wait();
                }

                // The relayer retries the stuck message and it lands.
                log("minter restored; waiting for relayer retry...");
                const receipt = await waitForReceipt(api, nonce, 420_000);
                expect(receipt.fallback).toBeNull();
                expect(await tusdOf(user.address)).toBe(amount);
            },
        });

        it({
            id: "C04",
            title: "relayer outage leaves the transfer in transit; restart recovers it",
            test: async () => {
                const user = freshUser("outage");
                const amount = 15n * 10n ** 9n;

                execSync("docker stop rails-relayer", { stdio: "pipe" });
                let nonce: bigint;
                try {
                    nonce = await depositFromBase({
                        amount,
                        dest: user.address,
                        action: { kind: "credit" },
                    });

                    // In transit: locked at origin, not processed on the hub.
                    await new Promise((resolve) => setTimeout(resolve, 15_000));
                    const processed = (await api.query.usdPsm.processedNonces(nonce)).toJSON();
                    expect(processed).toBeNull();
                } finally {
                    execSync("docker start rails-relayer", { stdio: "pipe" });
                }

                log("relayer restarted; waiting for recovery...");
                const receipt = await waitForReceipt(api, nonce, 300_000);
                expect(receipt.fallback).toBeNull();
                expect(await tusdOf(user.address)).toBe(amount);
            },
        });
    },
});
