import { BINDING_VERSION, Client, Keypair, blake2_256, storage } from "@bittensor/sdk";
import { describeSuite, expect } from "@moonwall/cli";

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
    title: "Rust-backed TypeScript SDK integration",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
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
                const alice = Keypair.fromUri("//Alice");
                const remark = blake2_256(Buffer.from(`typescript-sdk:${BINDING_VERSION}`));
                const call = await client.composeCall("System", "remark", { remark });

                try {
                    const signed = await client.signExtrinsic(call, alice);
                    const watcher = await client.watchSigned(signed);

                    await context.createBlock();

                    const included = await watcher.result;
                    expect(included.success).to.be.true;
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
