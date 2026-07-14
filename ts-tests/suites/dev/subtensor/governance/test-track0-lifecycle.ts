import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import {
    bootstrapMembership,
    castVote,
    DEV_TRACK,
    type GovernanceMembership,
    getStatusKind,
    getTally,
    nudge,
    referendumCount,
    submitOnTrack,
    systemEvents,
} from "../../../../utils/governance";

describeSuite({
    id: "DEV_SUB_GOV_TRACK0_LIFECYCLE_01",
    title: "Governance — Track 0 runtime thresholds",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;
        const beneficiary = generateKeyringPair("sr25519");
        const remark = (amount: bigint) => api.tx.balances.forceSetBalance(beneficiary.address, amount);

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;
            gov = await bootstrapMembership(api, context, sudoer, {
                triumvirate: 3,
                economic: 1,
                building: 1,
            });
        });

        it({
            id: "T01",
            title: "2-of-3 runtime Triumvirate ayes delegates to the review track",
            test: async () => {
                const index = await submitOnTrack(api, context, gov.proposer, DEV_TRACK.TRIUMVIRATE, remark(2n));

                await castVote(api, context, gov.triumvirate[0], index, true);
                await castVote(api, context, gov.triumvirate[1], index, true);
                await nudge(context);

                const delegated = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                expect(delegated, "Delegated event").to.exist;

                const data = delegated?.event.data.toJSON() as {
                    index?: number;
                    review?: number;
                    track?: number;
                } & Array<number>;
                const childIndex = data.review ?? data[1];
                expect(data.index ?? data[0]).to.equal(index);
                expect(data.track ?? data[2]).to.equal(DEV_TRACK.REVIEW);
                expect(await getStatusKind(api, index)).to.equal("delegated");
                expect(await getStatusKind(api, childIndex)).to.equal("ongoing");
            },
        });

        it({
            id: "T02",
            title: "2-of-3 runtime Triumvirate nays reject without creating a review child",
            test: async () => {
                const index = await submitOnTrack(api, context, gov.proposer, DEV_TRACK.TRIUMVIRATE, remark(3n));
                const countBefore = await referendumCount(api);

                await castVote(api, context, gov.triumvirate[0], index, false);
                await castVote(api, context, gov.triumvirate[1], index, false);
                await nudge(context);

                const rejected = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Rejected"
                );
                expect(rejected, "Rejected event").to.exist;
                expect(await getStatusKind(api, index)).to.equal("rejected");
                expect(await referendumCount(api)).to.equal(countBefore);
            },
        });

        it({
            id: "T03",
            title: "split Triumvirate votes stay below both runtime thresholds",
            test: async () => {
                const index = await submitOnTrack(api, context, gov.proposer, DEV_TRACK.TRIUMVIRATE, remark(4n));
                await castVote(api, context, gov.triumvirate[0], index, true);
                await castVote(api, context, gov.triumvirate[1], index, false);
                await nudge(context, 2);

                expect(await getStatusKind(api, index)).to.equal("ongoing");
                expect(await getTally(api, index)).to.deep.equal({
                    ayes: 1,
                    nays: 1,
                    total: 3,
                });
            },
        });
    },
});
