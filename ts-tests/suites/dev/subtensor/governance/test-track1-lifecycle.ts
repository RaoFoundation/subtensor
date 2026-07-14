import { beforeAll, type DevModeContext, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../../utils/account";
import {
    bootstrapMembership,
    castVote,
    DEV_TRACK,
    freeBalance,
    type GovernanceMembership,
    getStatusKind,
    getTally,
    isEnactmentTaskNone,
    lastModuleError,
    nudge,
    submitOnTrack,
    sudoInBlock,
    systemEvents,
} from "../../../../utils/governance";

async function delegateToTrack1(
    api: ApiPromise,
    context: DevModeContext,
    gov: GovernanceMembership,
    payload: Parameters<typeof submitOnTrack>[4]
): Promise<{ outer: number; child: number }> {
    const outer = await submitOnTrack(api, context, gov.proposer, DEV_TRACK.TRIUMVIRATE, payload);
    await castVote(api, context, gov.triumvirate[0], outer, true);
    await castVote(api, context, gov.triumvirate[1], outer, true);
    await nudge(context);

    const delegated = (await systemEvents(api)).find(
        (e) => e.event.section === "referenda" && e.event.method === "Delegated"
    );
    if (!delegated) {
        throw new Error("Delegation never fired; the review voter set may be empty");
    }
    const data = delegated.event.data.toJSON() as { review?: number } & Array<number>;
    return { outer, child: data.review ?? data[1] };
}

describeSuite({
    id: "DEV_SUB_GOV_TRACK1_LIFECYCLE_01",
    title: "Governance — Track 1 runtime review path",
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
                economic: 2,
                building: 2,
            });
        });

        it({
            id: "T01",
            title: "delegation creates a Track 1 child with Economic ∪ Building as voters",
            test: async () => {
                const { child } = await delegateToTrack1(api, context, gov, remark(101n));
                expect(await getStatusKind(api, child)).to.equal("ongoing");
                expect(await getTally(api, child)).to.deep.equal({
                    ayes: 0,
                    nays: 0,
                    total: 4,
                });
            },
        });

        it({
            id: "T02",
            title: "3-of-4 runtime review ayes fast-track and dispatch as Root",
            test: async () => {
                const targetAmount = 7_777_777_000n;
                const target = generateKeyringPair("sr25519");
                const { child } = await delegateToTrack1(
                    api,
                    context,
                    gov,
                    api.tx.balances.forceSetBalance(target.address, targetAmount)
                );

                await castVote(api, context, gov.economic[0], child, true);
                await castVote(api, context, gov.economic[1], child, true);
                await castVote(api, context, gov.building[0], child, true);
                await nudge(context);

                const fastTracked = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "FastTracked"
                );
                expect(fastTracked, "FastTracked event").to.exist;
                expect(await getStatusKind(api, child)).to.equal("fastTracked");

                await nudge(context);
                const enacted = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Enacted"
                );
                expect(enacted, "Enacted event").to.exist;
                expect(await freeBalance(api, target.address)).to.equal(targetAmount);
            },
        });

        it({
            id: "T03",
            title: "3-of-4 runtime review nays cancel and clear the enactment task",
            test: async () => {
                const { child } = await delegateToTrack1(api, context, gov, remark(103n));

                await castVote(api, context, gov.economic[0], child, false);
                await castVote(api, context, gov.economic[1], child, false);
                await castVote(api, context, gov.building[0], child, false);
                await nudge(context);

                const cancelled = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Cancelled"
                );
                expect(cancelled, "Cancelled event").to.exist;
                expect(await getStatusKind(api, child)).to.equal("cancelled");
                expect(await isEnactmentTaskNone(api, child), "enactment task cleared").to.be.true;
            },
        });

        it({
            id: "T04",
            title: "Root kill in the fast-track block prevents scheduled dispatch",
            test: async () => {
                const target = generateKeyringPair("sr25519");
                const { child } = await delegateToTrack1(
                    api,
                    context,
                    gov,
                    api.tx.balances.forceSetBalance(target.address, 42n)
                );
                await castVote(api, context, gov.economic[0], child, true);
                await castVote(api, context, gov.economic[1], child, true);
                await castVote(api, context, gov.building[0], child, true);

                await context.createBlock([
                    await api.tx.sudo.sudo(api.tx.referenda.kill(child)).signAsync(sudoer, { era: 0 }),
                ]);

                const events = await systemEvents(api);
                expect(events.find((e) => e.event.section === "referenda" && e.event.method === "FastTracked")).to
                    .exist;
                expect(events.find((e) => e.event.section === "referenda" && e.event.method === "Killed")).to.exist;
                expect(await lastModuleError(api)).to.be.null;

                await nudge(context, 3);
                expect(await freeBalance(api, target.address)).to.equal(0n);
            },
        });

        it({
            id: "T05",
            title: "runtime Root dispatch errors are recorded in the Enacted event",
            test: async () => {
                const recipient = generateKeyringPair("sr25519");
                const { child } = await delegateToTrack1(
                    api,
                    context,
                    gov,
                    api.tx.balances.transferKeepAlive(recipient.address, 100n)
                );
                await castVote(api, context, gov.economic[0], child, true);
                await castVote(api, context, gov.economic[1], child, true);
                await castVote(api, context, gov.building[0], child, true);
                await nudge(context);
                await nudge(context);

                const enacted = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Enacted"
                );
                expect(enacted, "Enacted event").to.exist;
                const data = enacted?.event.data.toJSON() as { error?: unknown } | Array<unknown>;
                const errorField = Array.isArray(data) ? data[2] : data.error;
                expect(errorField, "Enacted carries a non-null error").to.not.be.null;
                expect(await freeBalance(api, recipient.address)).to.equal(0n);
            },
        });

        it({
            id: "T06",
            title: "Root can directly enact an Ongoing runtime review referendum",
            test: async () => {
                const target = generateKeyringPair("sr25519");
                const amount = 12_345_000n;
                const innerCall = api.tx.balances.forceSetBalance(target.address, amount);

                const { child } = await delegateToTrack1(api, context, gov, innerCall);
                expect(await getStatusKind(api, child)).to.equal("ongoing");

                await sudoInBlock(api, context, sudoer, api.tx.referenda.enact(child, innerCall));

                const enacted = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Enacted"
                );
                expect(enacted, "Enacted event").to.exist;
                expect(await getStatusKind(api, child)).to.equal("enacted");
                expect(await freeBalance(api, target.address)).to.equal(amount);
            },
        });
    },
});
