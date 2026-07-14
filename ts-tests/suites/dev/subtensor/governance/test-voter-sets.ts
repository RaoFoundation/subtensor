import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import {
    addMembers,
    bootstrapMembership,
    castVote,
    DEV_TRACK,
    fundAccounts,
    type GovernanceMembership,
    getTally,
    lastModuleError,
    nudge,
    submitOnTrack,
    sudoInBlock,
    systemEvents,
} from "../../../../utils/governance";

describeSuite({
    id: "DEV_SUB_GOV_VOTER_SETS_01",
    title: "Governance — runtime voter-set wiring",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;

        const latecomer = generateKeyringPair("sr25519");
        const overlap = generateKeyringPair("sr25519");
        const beneficiary = generateKeyringPair("sr25519");
        const remark = (amount: bigint) => api.tx.balances.forceSetBalance(beneficiary.address, amount);

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;
            gov = await bootstrapMembership(api, context, sudoer, {
                proposers: 4,
                triumvirate: 3,
                economic: 1,
                building: 1,
            });
            await fundAccounts(api, context, sudoer, [latecomer.address, overlap.address]);
            await addMembers(api, context, sudoer, [
                { collective: "Economic", account: overlap },
                { collective: "Building", account: overlap },
            ]);
        });

        it({
            id: "T01",
            title: "runtime voter snapshots survive a Triumvirate membership swap",
            test: async () => {
                const index = await submitOnTrack(api, context, gov.proposers[0], DEV_TRACK.TRIUMVIRATE, remark(208n));

                const frozenSet = (await api.query.signedVoting.voterSetOf(index)).toJSON() as string[];
                expect(frozenSet).to.have.length(3);
                expect(frozenSet).to.not.include(latecomer.address);

                await sudoInBlock(
                    api,
                    context,
                    sudoer,
                    api.tx.multiCollective.swapMember("Triumvirate", gov.triumvirate[2].address, latecomer.address)
                );
                expect(await lastModuleError(api)).to.be.null;

                await castVote(api, context, latecomer, index, true);
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "signedVoting",
                    name: "NotInVoterSet",
                });

                await sudoInBlock(
                    api,
                    context,
                    sudoer,
                    api.tx.multiCollective.swapMember("Triumvirate", latecomer.address, gov.triumvirate[2].address)
                );
            },
        });

        it({
            id: "T02",
            title: "Triumvirate members cannot vote on the Track 1 review child",
            test: async () => {
                const parent = await submitOnTrack(api, context, gov.proposers[1], DEV_TRACK.TRIUMVIRATE, remark(214n));
                await castVote(api, context, gov.triumvirate[0], parent, true);
                await castVote(api, context, gov.triumvirate[1], parent, true);
                await nudge(context);

                const delegated = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                const data = delegated?.event.data.toJSON() as { review?: number } & Array<number>;
                const child = data.review ?? data[1];

                await castVote(api, context, gov.triumvirate[0], child, true);
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "signedVoting",
                    name: "NotInVoterSet",
                });
            },
        });

        it({
            id: "T03",
            title: "Economic/Building members cannot vote on the Track 0 parent",
            test: async () => {
                const index = await submitOnTrack(api, context, gov.proposers[2], DEV_TRACK.TRIUMVIRATE, remark(215n));
                await castVote(api, context, gov.economic[0], index, true);
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "signedVoting",
                    name: "NotInVoterSet",
                });
            },
        });

        it({
            id: "T04",
            title: "runtime Economic ∪ Building review voters dedupe overlapping accounts",
            test: async () => {
                const parent = await submitOnTrack(api, context, gov.proposers[3], DEV_TRACK.TRIUMVIRATE, remark(216n));
                await castVote(api, context, gov.triumvirate[0], parent, true);
                await castVote(api, context, gov.triumvirate[1], parent, true);
                await nudge(context);

                const delegated = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                expect(delegated, "Delegated event").to.exist;
                const data = delegated?.event.data.toJSON() as { review?: number } & Array<number>;
                const child = data.review ?? data[1];

                const voterSet = (await api.query.signedVoting.voterSetOf(child)).toJSON() as string[];
                expect(voterSet).to.have.length(3);
                expect(voterSet.filter((a) => a === overlap.address)).to.have.length(1);
                expect((await getTally(api, child))?.total).to.equal(3);
            },
        });
    },
});
