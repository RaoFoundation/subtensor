/**
 * Golden transcript replay: the envelope wire vectors pinned in
 * fixtures/golden-envelopes.json (and mirrored byte-for-byte in the Rust
 * test `envelope_golden_vectors`) must be reproduced exactly by the TS
 * encoder, and the rig quote surface must satisfy the constant-product
 * pool invariants the CLI transcripts rely on.
 */
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import { type EnvelopeAction, type RailsRig, connectRig, encodeEnvelope } from "../../utils/rails.js";
import golden from "./fixtures/golden-envelopes.json";

describeSuite({
    id: "R04",
    title: "Rails golden fixtures: wire format and quote surface",
    foundationMethods: "read_only",
    testCases: ({ it, log }) => {
        let rig: RailsRig;

        async function rpc<T>(method: string, params: unknown[]): Promise<T> {
            const response = await fetch(rig.manifest.chains.btlocal.rpcHttp, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
            });
            const json = (await response.json()) as { result?: T; error?: { message: string } };
            if (json.result === undefined) {
                throw new Error(`${method} failed: ${JSON.stringify(json.error)}`);
            }
            return json.result;
        }

        beforeAll(() => {
            rig = connectRig();
        });

        it({
            id: "C01",
            title: "encoder reproduces every golden envelope byte-for-byte",
            test: () => {
                for (const testCase of golden.cases) {
                    const input = testCase.input as {
                        usdAssetId?: number;
                        amount: string;
                        dest: string;
                        action: Record<string, unknown> & { kind: string };
                        nonce: string;
                    };
                    const action = {
                        ...input.action,
                        ...(input.action.minAlpha !== undefined
                            ? { minAlpha: BigInt(input.action.minAlpha as string) }
                            : {}),
                        ...(input.action.minUsd !== undefined
                            ? { minUsd: BigInt(input.action.minUsd as string) }
                            : {}),
                    } as EnvelopeAction;
                    const wire = encodeEnvelope({
                        usdAssetId: input.usdAssetId,
                        amount: BigInt(input.amount),
                        dest: input.dest,
                        action,
                        nonce: BigInt(input.nonce),
                    });
                    log(`${testCase.name}: ${wire.length / 2 - 1} bytes`);
                    expect(wire).toBe(testCase.wire);
                }
            },
        });

        it({
            id: "C02",
            title: "quote surface is consistent with pool state (constant product)",
            test: async () => {
                const pool = await rpc<{ tao_reserve: number; tusd_reserve: number; fee_bps: number }>(
                    "rails_poolState",
                    []
                );
                expect(pool.tao_reserve).toBeGreaterThan(0);
                expect(pool.tusd_reserve).toBeGreaterThan(0);

                const usdIn = 1_000_000_000; // 1 USD
                const quoted = await rpc<number | null>("rails_quoteUsdToTao", [usdIn]);
                expect(quoted).not.toBeNull();

                // Expected constant-product output with the pool fee applied.
                const inAfterFee = (BigInt(usdIn) * BigInt(10_000 - pool.fee_bps)) / 10_000n;
                const expected = (BigInt(pool.tao_reserve) * inAfterFee) / (BigInt(pool.tusd_reserve) + inAfterFee);
                log(`1 USD -> ${quoted} rao (expected ${expected})`);
                expect(BigInt(quoted as number)).toBe(expected);

                // Round-trip quotes never create value: 1 USD -> TAO -> USD < 1 USD.
                const back = await rpc<number | null>("rails_quoteTaoToUsd", [quoted]);
                expect(back).not.toBeNull();
                expect(BigInt(back as number)).toBeLessThan(BigInt(usdIn));
            },
        });

        it({
            id: "C03",
            title: "registry views resolve the rig's canonical facts",
            test: async () => {
                const gateway = await rpc<string | null>("rails_gateway", []);
                expect(gateway?.toLowerCase()).toBe(rig.manifest.chains.btlocal.gateway.toLowerCase());

                const assets = await rpc<{ asset_id: number; erc20: string; enabled: boolean }[]>("rails_assets", []);
                const canonical = assets.find(
                    (a) => a.erc20.toLowerCase() === rig.manifest.chains.btlocal.canonicalUsd.toLowerCase()
                );
                expect(canonical).toBeDefined();
                expect(canonical?.enabled).toBe(true);

                const hub = await rpc<{ hub_sender: string; mailbox: string | null }>("rails_hubInfo", []);
                expect(hub.hub_sender.toLowerCase()).toBe(rig.manifest.chains.btlocal.hubSender.toLowerCase());
                expect(hub.mailbox?.toLowerCase()).toBe(rig.manifest.chains.btlocal.mailbox.toLowerCase());
            },
        });
    },
});
