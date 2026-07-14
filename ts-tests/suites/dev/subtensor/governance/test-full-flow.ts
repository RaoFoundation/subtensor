import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import { freeBalance, referendumCount, referendumStatusFor, systemEvents } from "../../../../utils/governance";

describeSuite({
    id: "DEV_SUB_GOV_FULLFLOW_01",
    title: "Governance — full two-phase flow (track 0 + track 1)",
    foundationMethods: "dev",
    testCases: ({ it, context, log }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;

        const proposer = generateKeyringPair("sr25519");
        const triumvirate1 = generateKeyringPair("sr25519");
        const triumvirate2 = generateKeyringPair("sr25519");
        const triumvirate3 = generateKeyringPair("sr25519");
        const economic1 = generateKeyringPair("sr25519");
        const economic2 = generateKeyringPair("sr25519");
        const building1 = generateKeyringPair("sr25519");
        const building2 = generateKeyringPair("sr25519");
        const target = generateKeyringPair("sr25519");

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;

            const fund = 1_000_000_000_000n;
            for (const inner of [
                api.tx.balances.forceSetBalance(proposer.address, fund),
                api.tx.balances.forceSetBalance(triumvirate1.address, fund),
                api.tx.balances.forceSetBalance(triumvirate2.address, fund),
                api.tx.balances.forceSetBalance(triumvirate3.address, fund),
                api.tx.balances.forceSetBalance(economic1.address, fund),
                api.tx.balances.forceSetBalance(economic2.address, fund),
                api.tx.balances.forceSetBalance(building1.address, fund),
                api.tx.balances.forceSetBalance(building2.address, fund),
                api.tx.multiCollective.addMember("Proposers", proposer.address),
                api.tx.multiCollective.addMember("Triumvirate", triumvirate1.address),
                api.tx.multiCollective.addMember("Triumvirate", triumvirate2.address),
                api.tx.multiCollective.addMember("Triumvirate", triumvirate3.address),
                api.tx.multiCollective.addMember("Economic", economic1.address),
                api.tx.multiCollective.addMember("Economic", economic2.address),
                api.tx.multiCollective.addMember("Building", building1.address),
                api.tx.multiCollective.addMember("Building", building2.address),
            ]) {
                await context.createBlock([await api.tx.sudo.sudo(inner).signAsync(sudoer)]);
            }
            const economic = await api.query.multiCollective.members("Economic");
            const building = await api.query.multiCollective.members("Building");
            log(`Economic: ${economic.toJSON()}`);
            log(`Building: ${building.toJSON()}`);
            expect(economic.toJSON()).to.have.length(2);
            expect(building.toJSON()).to.have.length(2);
        });

        it({
            id: "T01",
            title: "proposer submits; triumvirate delegates; collective fast-tracks; balance changes",
            test: async () => {
                const targetAmount = 2_000_000_000n;
                const countBefore = await referendumCount(api);

                const payload = api.tx.balances.forceSetBalance(target.address, targetAmount);

                await context.createBlock([await api.tx.referenda.submit(0, payload).signAsync(proposer)]);
                const outerPoll = countBefore;

                // Triumvirate reaches 2/3 aye.
                await context.createBlock([await api.tx.signedVoting.vote(outerPoll, true).signAsync(triumvirate1)]);
                await context.createBlock([await api.tx.signedVoting.vote(outerPoll, true).signAsync(triumvirate2)]);

                // The 2nd vote schedules a `nudge` for the next block, so need to create 1 block
                await context.createBlock([]);

                const approveEvents = await systemEvents(api);
                const delegated = approveEvents.find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                expect(delegated, "Delegated").to.exist;

                const delegatedData = delegated?.event.data as unknown as {
                    review: any;
                    track: any;
                };
                expect(delegatedData.track.toString()).to.equal("1");

                const innerPoll = outerPoll + 1;
                expect(delegatedData.review.toString()).to.equal(innerPoll.toString());

                const innerStatus = await referendumStatusFor(api, innerPoll);
                expect(innerStatus.isSome, "inner poll stored").to.be.true;
                expect(innerStatus.toJSON()).to.have.property("ongoing");

                // Track 1 voter_set = Union(Economic, Building) → 4 voters total.
                // 3 ayes (3/4 = 75% ≥ 67% fast_track threshold) is enough.
                await context.createBlock([await api.tx.signedVoting.vote(innerPoll, true).signAsync(economic1)]);
                await context.createBlock([await api.tx.signedVoting.vote(innerPoll, true).signAsync(economic2)]);
                await context.createBlock([await api.tx.signedVoting.vote(innerPoll, true).signAsync(building1)]);

                // Same nudge pattern: 3rd vote schedules nudge → next block fast-tracks.
                await context.createBlock([]);

                const fastTrackEvents = await systemEvents(api);
                const fastTracked = fastTrackEvents.find(
                    (e) => e.event.section === "referenda" && e.event.method === "FastTracked"
                );
                expect(fastTracked, "inner FastTracked").to.exist;

                await context.createBlock([]);

                const finalEvents = await systemEvents(api);
                const dispatched = finalEvents.find(
                    (e) => e.event.section === "scheduler" && e.event.method === "Dispatched"
                );
                expect(dispatched, "scheduler.Dispatched").to.exist;

                const targetFinal = await freeBalance(api, target.address);
                expect(targetFinal).to.equal(targetAmount);
            },
        });
    },
});
