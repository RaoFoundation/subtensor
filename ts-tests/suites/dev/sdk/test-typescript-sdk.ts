import { BINDING_VERSION, Client, Keypair, blake2_256, storage } from "@bittensor/sdk";
import { describeSuite, expect } from "@moonwall/cli";

describeSuite({
    id: "DEV_TYPESCRIPT_SDK_01",
    title: "Rust-backed TypeScript SDK integration",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        it({
            id: "T01",
            title: "connects, constructs, submits, and reads with the SDK chain client",
            test: async () => {
                const client = await new Client("ws://127.0.0.1:9947").connect();
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
                                (event as { module_id?: string; event_id?: string }).module_id ===
                                    "System" &&
                                (event as { module_id?: string; event_id?: string }).event_id ===
                                    "ExtrinsicSuccess",
                        ),
                    ).to.be.true;
                } finally {
                    await client.close();
                }
            },
        });
    },
});
