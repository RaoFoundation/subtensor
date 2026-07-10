import { BINDING_VERSION, blake2_256 } from "@bittensor/sdk";
import type { ApiPromise } from "@polkadot/api";
import { describeSuite, expect } from "@moonwall/cli";
import { keyringPairFromUri } from "../../../utils/account.ts";

describeSuite({
    id: "DEV_TYPESCRIPT_SDK_01",
    title: "Rust-backed TypeScript SDK integration",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        it({
            id: "T01",
            title: "signs and submits a transaction with the Rust keypair",
            test: async () => {
                const polkadotJs: ApiPromise = context.polkadotJs();
                const alice = keyringPairFromUri("//Alice");
                const remark = blake2_256(Buffer.from(`typescript-sdk:${BINDING_VERSION}`));
                const tx = polkadotJs.tx.system.remark(remark);

                await context.createBlock([await tx.signAsync(alice)]);

                const events = await polkadotJs.query.system.events();
                expect(events.some(({ event }) => event.method === "ExtrinsicSuccess")).to.be.true;
            },
        });
    },
});
