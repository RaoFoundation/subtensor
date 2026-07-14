import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../utils/account";
import {
    bootstrapMembership,
    castVote,
    DEV_TRACK,
    freeBalance,
    type GovernanceMembership,
    getStatusKind,
    nudge,
    referendumStatusFor,
    submitOnTrack,
    systemEvents,
} from "../../../utils/governance";

/**
 * Reachable only with `--features fast-runtime`:
 *   REVIEW_INITIAL_DELAY = prod_or_fast!(7_200, 30)
 *
 * On delegation, a Track 1 child is born with its enactment task already
 * scheduled at `submitted + initial_delay`. If voters do nothing (no
 * fast-track and no cancel), the wrapper task fires naturally and runs the
 * inner call. This locks in the "Adjustable defaults to executing"
 * contract: an approved Triumvirate proposal will eventually dispatch even
 * without any review activity.
 */
describeSuite({
    id: "DEV_FAST_GOV_TRACK1_NATURAL_01",
    title: "Governance (fast-runtime) — Track 1 natural enactment at initial_delay",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;
        const target = generateKeyringPair("sr25519");
        const targetAmount = 555_000_000n;

        // Mirrors `runtime/src/governance/tracks.rs` under fast-runtime.
        const REVIEW_INITIAL_DELAY = 30;

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
            title: "delegated child enacts at submitted + initial_delay with no Track 1 votes",
            test: async () => {
                const parent = await submitOnTrack(
                    api,
                    context,
                    gov.proposer,
                    DEV_TRACK.TRIUMVIRATE,
                    api.tx.balances.forceSetBalance(target.address, targetAmount)
                );

                await castVote(api, context, gov.triumvirate[0], parent, true);
                await castVote(api, context, gov.triumvirate[1], parent, true);
                await nudge(context);

                const delegated = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Delegated"
                );
                expect(delegated, "Delegated event").to.exist;
                const arr = delegated?.event.data.toJSON() as Array<number>;
                const child = arr[1];
                expect(await getStatusKind(api, child)).to.equal("ongoing");

                // Without any votes on the child, the scheduled enactment
                // task fires at submitted + initial_delay. Use submitted from
                // the child's status (set at delegation, not at parent
                // submission).
                const childStatus = (await referendumStatusFor(api, child)).toJSON() as {
                    ongoing: { submitted: number };
                } | null;
                const childSubmitted = childStatus?.ongoing?.submitted;
                expect(childSubmitted, "child submitted block").to.be.a("number");

                const targetBlock = (childSubmitted as number) + REVIEW_INITIAL_DELAY + 2;
                while (((await api.query.system.number()).toJSON() as number) < targetBlock) {
                    await nudge(context);
                }

                const enacted = (await systemEvents(api)).find(
                    (e) => e.event.section === "referenda" && e.event.method === "Enacted"
                );
                // The Enacted event may have fired in an earlier block within
                // the polling loop; if so, also accept the terminal status.
                expect(await getStatusKind(api, child)).to.equal("enacted");
                if (enacted) {
                    const data = enacted.event.data.toJSON() as { error?: unknown } | Array<unknown>;
                    const errorField = Array.isArray(data) ? data[2] : data.error;
                    expect(errorField, "Enacted carries no error").to.be.null;
                }

                expect(await freeBalance(api, target.address)).to.equal(targetAmount);
            },
        });
    },
});
