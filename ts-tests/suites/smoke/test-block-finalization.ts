import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { ApiPromise } from "@polkadot/api";
import { getBlockTime } from "@moonwall/util";

describeSuite({
    id: "S01",
    title: "Smoke test - test block finalization",
    foundationMethods: "read_only",
    testCases: ({ it, context, log }) => {
        let api: ApiPromise;

        beforeAll(() => {
            api = context.polkadotJs();
        });

        it({
            id: "C01",
            title: "Blocks should be finalized",
            test: async () => {
                const head = await api.rpc.chain.getFinalizedHead();
                const block = await api.rpc.chain.getBlock(head);
                const diff = Date.now() - getBlockTime(block);

                log(`Current head block number: ${block.block.header.number.toNumber()}`);
                log(`Last finalized block was ${diff / 1000} seconds ago`);

                expect(diff).to.be.lessThanOrEqual(10 * 60 * 1000); // 10 minutes in milliseconds
                expect(api.runtimeVersion.specVersion.toNumber()).to.be.greaterThan(0);
            },
        });
    },
});
