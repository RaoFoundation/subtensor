import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import {
    bootstrapMembership,
    castVote,
    DEV_TRACK,
    fundAccounts,
    type GovernanceMembership,
    getActiveCount,
    getActivePerProposer,
    getStatusKind,
    inBlock,
    lastModuleError,
    nudge,
    submitOnTrack,
    sudoInBlock,
    systemEvents,
} from "../../../../utils/governance";

describeSuite({
    id: "DEV_SUB_GOV_CAPACITY_01",
    title: "Governance — runtime referendum capacity limits",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;
        const idleProposer = generateKeyringPair("sr25519");
        const beneficiary = generateKeyringPair("sr25519");
        const remark = (amount: bigint) => api.tx.balances.forceSetBalance(beneficiary.address, amount);

        const MAX_QUEUED = 20;
        const MAX_ACTIVE_PER_PROPOSER = 5;
        const PROPOSERS_NEEDED = MAX_QUEUED / MAX_ACTIVE_PER_PROPOSER;

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;
            gov = await bootstrapMembership(api, context, sudoer, {
                proposers: PROPOSERS_NEEDED,
                triumvirate: 3,
                economic: 1,
                building: 1,
            });

            await fundAccounts(api, context, sudoer, [idleProposer.address]);
            await inBlock(
                context,
                sudoer,
                api.tx.sudo.sudo(api.tx.multiCollective.addMember("Proposers", idleProposer.address))
            );
            expect(await lastModuleError(api)).to.be.null;
        });

        it({
            id: "T01",
            title: "runtime MaxActivePerProposer is enforced at five active referenda",
            test: async () => {
                const submitted: number[] = [];
                for (let i = 0; i < MAX_ACTIVE_PER_PROPOSER; i++) {
                    submitted.push(
                        await submitOnTrack(api, context, gov.proposer, DEV_TRACK.TRIUMVIRATE, remark(BigInt(300 + i)))
                    );
                    expect(await lastModuleError(api)).to.be.null;
                }
                expect(await getActivePerProposer(api, gov.proposer.address)).to.equal(MAX_ACTIVE_PER_PROPOSER);

                await inBlock(context, gov.proposer, api.tx.referenda.submit(DEV_TRACK.TRIUMVIRATE, remark(399n)));
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "referenda",
                    name: "ProposerQuotaExceeded",
                });

                for (const index of submitted) {
                    await sudoInBlock(api, context, sudoer, api.tx.referenda.kill(index));
                }
                expect(await getActivePerProposer(api, gov.proposer.address)).to.equal(0);
            },
        });

        it({
            id: "T02",
            title: "delegation is quota-neutral in the concrete two-track runtime",
            test: async () => {
                const fresh = gov.proposers[1];
                expect(await getActivePerProposer(api, fresh.address)).to.equal(0);

                const parent = await submitOnTrack(api, context, fresh, DEV_TRACK.TRIUMVIRATE, remark(600n));
                expect(await getActivePerProposer(api, fresh.address)).to.equal(1);

                await castVote(api, context, gov.triumvirate[0], parent, true);
                await castVote(api, context, gov.triumvirate[1], parent, true);
                await nudge(context);

                expect(await getStatusKind(api, parent)).to.equal("delegated");
                expect(await getActivePerProposer(api, fresh.address)).to.equal(1);

                const delegated = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                const data = delegated?.event.data.toJSON() as { review?: number } & Array<number>;
                await sudoInBlock(api, context, sudoer, api.tx.referenda.kill(data.review ?? data[1]));
                expect(await getActivePerProposer(api, fresh.address)).to.equal(0);
            },
        });

        it({
            id: "T03",
            title: "with the queue at capacity, an idle proposer's submit fails with QueueFull",
            test: async () => {
                expect(await getActiveCount(api)).to.equal(0);

                for (let p = 0; p < PROPOSERS_NEEDED; p++) {
                    for (let i = 0; i < MAX_ACTIVE_PER_PROPOSER; i++) {
                        await submitOnTrack(
                            api,
                            context,
                            gov.proposers[p],
                            DEV_TRACK.TRIUMVIRATE,
                            api.tx.system.remark(`fill-${p}-${i}`)
                        );
                        expect(await lastModuleError(api)).to.be.null;
                    }
                }
                expect(await getActiveCount(api)).to.equal(MAX_QUEUED);

                await inBlock(
                    context,
                    idleProposer,
                    api.tx.referenda.submit(DEV_TRACK.TRIUMVIRATE, api.tx.system.remark("21st-attempt"))
                );
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "referenda",
                    name: "QueueFull",
                });
                expect(await getActiveCount(api)).to.equal(MAX_QUEUED);
                expect((await api.query.referenda.activePerProposer(idleProposer.address)).toJSON()).to.equal(0);
            },
        });
    },
});
