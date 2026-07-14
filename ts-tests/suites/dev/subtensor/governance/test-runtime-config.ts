import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import {
    addMembers,
    castVote,
    type Collective,
    DEFAULT_FUND,
    DEV_TRACK,
    fundAccounts,
    getMembers,
    getStatusKind,
    inBlock,
    lastModuleError,
    nudge,
    referendumCount,
    submitOnTrack,
    sudoInBlock,
    systemEvents,
} from "../../../../utils/governance";

const fresh = (n: number): KeyringPair[] => Array.from({ length: n }, () => generateKeyringPair("sr25519"));

describeSuite({
    id: "DEV_SUB_GOV_RUNTIME_CONFIG_01",
    title: "Governance — runtime configuration and submission guardrails",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;

        const proposers = fresh(1);
        const triumvirate = fresh(4);
        const economicEligible = fresh(2);
        const beneficiary = generateKeyringPair("sr25519");

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;

            await fundAccounts(
                api,
                context,
                sudoer,
                [...proposers, ...triumvirate, ...economicEligible].map((kp) => kp.address),
                DEFAULT_FUND
            );
            await addMembers(api, context, sudoer, [{ collective: "Proposers", account: proposers[0] }]);
        });

        it({
            id: "T01",
            title: "all runtime collective enum variants are addressable through metadata",
            test: async () => {
                const allCollectives: Collective[] = [
                    "Proposers",
                    "Triumvirate",
                    "Economic",
                    "Building",
                    "EconomicEligible",
                ];

                for (const collective of allCollectives) {
                    const members = await api.query.multiCollective.members(collective);
                    expect(members.toJSON()).to.be.an("array");
                }
            },
        });

        it({
            id: "T02",
            title: "Track 0 submission fails when the runtime Triumvirate voter set is empty",
            test: async () => {
                expect((await api.query.multiCollective.members("Triumvirate")).toJSON()).to.have.length(0);

                await inBlock(
                    context,
                    proposers[0],
                    api.tx.referenda.submit(DEV_TRACK.TRIUMVIRATE, api.tx.system.remark("attempted-with-no-voters"))
                );
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "referenda",
                    name: "EmptyVoterSet",
                });
                expect(await referendumCount(api)).to.equal(0);
            },
        });

        it({
            id: "T03",
            title: "Track 1 is not directly submittable in the runtime",
            test: async () => {
                await inBlock(
                    context,
                    proposers[0],
                    api.tx.referenda.submit(DEV_TRACK.REVIEW, api.tx.system.remark("direct-track-1"))
                );
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "referenda",
                    name: "TrackNotSubmittable",
                });
            },
        });

        it({
            id: "T04",
            title: "Triumvirate is runtime-configured as exactly three seats",
            test: async () => {
                await addMembers(api, context, sudoer, [
                    { collective: "Triumvirate", account: triumvirate[0] },
                    { collective: "Triumvirate", account: triumvirate[1] },
                    { collective: "Triumvirate", account: triumvirate[2] },
                ]);
                expect(await getMembers(api, "Triumvirate")).to.have.length(3);

                await sudoInBlock(
                    api,
                    context,
                    sudoer,
                    api.tx.multiCollective.addMember("Triumvirate", triumvirate[3].address)
                );
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "multiCollective",
                    name: "TooManyMembers",
                });

                await sudoInBlock(
                    api,
                    context,
                    sudoer,
                    api.tx.multiCollective.removeMember("Triumvirate", triumvirate[0].address)
                );
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "multiCollective",
                    name: "TooFewMembers",
                });
            },
        });

        it({
            id: "T05",
            title: "Proposers is not rotatable in the runtime",
            test: async () => {
                await sudoInBlock(api, context, sudoer, api.tx.multiCollective.forceRotate("Proposers"));
                expect(await lastModuleError(api)).to.deep.equal({
                    section: "multiCollective",
                    name: "CollectiveDoesNotRotate",
                });
            },
        });

        it({
            id: "T06",
            title: "EconomicEligible permits an empty runtime membership set",
            test: async () => {
                await sudoInBlock(
                    api,
                    context,
                    sudoer,
                    api.tx.multiCollective.setMembers(
                        "EconomicEligible",
                        economicEligible.map((kp) => kp.address)
                    )
                );
                expect(await lastModuleError(api)).to.be.null;
                expect(await getMembers(api, "EconomicEligible")).to.have.length(2);

                await sudoInBlock(api, context, sudoer, api.tx.multiCollective.setMembers("EconomicEligible", []));
                expect(await lastModuleError(api)).to.be.null;
                expect(await getMembers(api, "EconomicEligible")).to.have.length(0);
            },
        });

        it({
            id: "T07",
            title: "approval with empty review voter set emits ReviewSchedulingFailed; parent stays Ongoing",
            test: async () => {
                expect((await api.query.multiCollective.members("Economic")).toJSON()).to.have.length(0);
                expect((await api.query.multiCollective.members("Building")).toJSON()).to.have.length(0);

                const countBefore = await referendumCount(api);
                const index = await submitOnTrack(
                    api,
                    context,
                    proposers[0],
                    DEV_TRACK.TRIUMVIRATE,
                    api.tx.balances.forceSetBalance(beneficiary.address, 7n)
                );

                await castVote(api, context, triumvirate[0], index, true);
                await castVote(api, context, triumvirate[1], index, true);
                await nudge(context);

                const events = await systemEvents(api);
                const failed = events.find(
                    (e) => e.event.section === "referenda" && e.event.method === "ReviewSchedulingFailed"
                );
                expect(failed, "ReviewSchedulingFailed event").to.exist;
                const data = failed?.event.data.toJSON() as { index?: number; track?: number } | [number, number];
                if (Array.isArray(data)) {
                    expect(data[0]).to.equal(index);
                    expect(data[1]).to.equal(1);
                } else {
                    expect(data.index).to.equal(index);
                    expect(data.track).to.equal(1);
                }

                const delegated = events.find((e) => e.event.section === "referenda" && e.event.method === "Delegated");
                expect(delegated, "no Delegated when review scheduling fails").to.be.undefined;

                expect(await getStatusKind(api, index)).to.equal("ongoing");
                expect(await referendumCount(api)).to.equal(countBefore + 1);
            },
        });
    },
});
