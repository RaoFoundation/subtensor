import { BINDING_VERSION, Client, Keypair, blake2_256, storage } from "@bittensor/sdk";
import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import { tao } from "../../../utils";
import { devForceSetBalance } from "../../../utils/dev-helpers.js";

function getSdkEndpoint(): string | undefined {
    if (process.env.BT_CHAIN_ENDPOINT) {
        return process.env.BT_CHAIN_ENDPOINT;
    }
    if (process.env.WSS_URL) {
        return process.env.WSS_URL;
    }
    if (process.env.MOONWALL_RPC_PORT) {
        return `ws://127.0.0.1:${process.env.MOONWALL_RPC_PORT}`;
    }
}

describeSuite({
    id: "DEV_TYPESCRIPT_SDK_01",
    title: "Rust-backed bittensor-ts integration",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let polkadotJs: ApiPromise;

        beforeAll(() => {
            polkadotJs = context.polkadotJs();
        });

        it({
            id: "T01",
            title: "connects, constructs, submits, and reads with the SDK chain client",
            test: async () => {
                const endpoint = getSdkEndpoint();
                expect(endpoint).to.be.a("string");
                if (!endpoint) {
                    throw new Error("Moonwall did not expose an SDK websocket endpoint");
                }

                const client = await new Client(endpoint).connect();
                const signer = Keypair.fromUri("//Ferdie");
                await devForceSetBalance(polkadotJs, context, signer.ss58Address, tao(1_000));
                const remark = blake2_256(Buffer.from(`bittensor-ts:${BINDING_VERSION}`));
                const call = await client.composeCall("System", "remark", { remark });

                try {
                    await client.assertDescriptorSchema();
                    const includedPromise = client.submit(call, signer, {
                        allowRawCall: true,
                        waitForInclusion: true,
                        timeoutMs: 30_000,
                    });

                    let included: Awaited<typeof includedPromise> | undefined;
                    for (let attempt = 0; attempt < 10 && included === undefined; attempt++) {
                        await context.createBlock();
                        const raced = await Promise.race([
                            includedPromise.then((result) => ({ result })),
                            new Promise<null>((resolve) => setTimeout(() => resolve(null), 500)),
                        ]);
                        if (raced !== null) {
                            included = raced.result;
                        }
                    }

                    included ??= await includedPromise;
                    expect(included.success, included.message).to.be.true;
                    expect(included.blockHash).to.not.be.undefined;
                    expect(included.extrinsicIndex).to.be.a("number");

                    const events = (await client.query(storage.System.Events, [], included.blockHash)) as
                        | Array<{ module_id?: string; event_id?: string }>
                        | undefined;
                    expect(
                        events?.some(
                            (event) =>
                                (event as { module_id?: string; event_id?: string }).module_id === "System" &&
                                (event as { module_id?: string; event_id?: string }).event_id === "ExtrinsicSuccess"
                        )
                    ).to.be.true;
                } finally {
                    await client.close();
                }
            },
        });
    },
});
