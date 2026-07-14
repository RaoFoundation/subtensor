import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../utils/account";
import {
    bootstrapMembership,
    castVote,
    DEV_TRACK,
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
 *   REVIEW_MAX_DELAY     = prod_or_fast!(14_400, 60)
 *
 * `do_adjust_delay` interpolates the enactment task's dispatch time
 * between `submitted` (under full net approval) and `submitted + max_delay`
 * (under full net rejection), shaped by the runtime's ease-out
 * `AdjustmentCurve` (`1 - (1 - p)^3`). The exact mapping with a 4-voter set:
 *
 *   - 0 votes              → enacts at submitted + initial_delay (30)
 *   - 1 aye  (1/4)         → enacts at submitted + 8
 *                            progress = 25%/75% = 33%, curved = 1 - (2/3)^3,
 *                            delay = floor(0.296 * 30) = 8
 *   - 1 nay  (1/4)         → enacts at submitted + 56
 *                            progress = 25%/51% = 49%, curved ~= 86.7%,
 *                            delay = 30 + floor(0.867 * 30) = 56
 *
 * Three tests exercise the three regimes (net approval, net rejection,
 * net zero from cancellation) by observing the actual block at which
 * `Enacted` fires.
 */
describeSuite({
    id: "DEV_FAST_GOV_TRACK1_DELAY_CURVE_01",
    title: "Governance (fast-runtime) — Track 1 enactment delay adjustment curve",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;
        const beneficiary = generateKeyringPair("sr25519");

        const REVIEW_INITIAL_DELAY = 30;

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;
            gov = await bootstrapMembership(api, context, sudoer, {
                proposers: 3,
                triumvirate: 3,
                economic: 2,
                building: 2,
            });
        });

        const delegateToChild = async (
            proposer: KeyringPair
        ): Promise<{
            child: number;
            childSubmitted: number;
        }> => {
            const parent = await submitOnTrack(
                api,
                context,
                proposer,
                DEV_TRACK.TRIUMVIRATE,
                api.tx.balances.forceSetBalance(beneficiary.address, 1n)
            );
            await castVote(api, context, gov.triumvirate[0], parent, true);
            await castVote(api, context, gov.triumvirate[1], parent, true);
            await nudge(context);
            const arr = (await systemEvents(api))
                .find((e) => e.event.section === "referenda" && e.event.method === "Delegated")
                ?.event.data.toJSON() as Array<number>;
            const child = arr[1];
            const status = (await referendumStatusFor(api, child)).toJSON() as {
                ongoing: { submitted: number };
            };
            return { child, childSubmitted: status.ongoing.submitted };
        };

        /** Advance blocks until `index` reaches a terminal status; returns the block of transition. */
        const advanceUntilEnacted = async (index: number, maxBlocks: number): Promise<number> => {
            for (let i = 0; i < maxBlocks; i++) {
                const kind = await getStatusKind(api, index);
                if (kind === "enacted") {
                    return (await api.query.system.number()).toJSON() as number;
                }
                await nudge(context);
            }
            throw new Error(`referendum ${index} did not enact within ${maxBlocks} blocks`);
        };

        it({
            id: "T01",
            title: "1 aye → enactment shifts earlier (submitted + 8 with ease-out curve)",
            test: async () => {
                const { child, childSubmitted } = await delegateToChild(gov.proposers[0]);
                await castVote(api, context, gov.economic[0], child, true);
                // Let the alarm fire to apply the adjustment.
                await nudge(context);

                const enactedAt = await advanceUntilEnacted(child, REVIEW_INITIAL_DELAY + 5);
                const expected = childSubmitted + 8;
                // Allow ±2 blocks of slack: the alarm fires one block after
                // the vote, and the scheduler may include the task one block
                // after its scheduled `when`.
                expect(enactedAt).to.be.at.least(expected);
                expect(enactedAt).to.be.at.most(expected + 2);
                expect(enactedAt, "earlier than initial_delay default").to.be.lessThan(
                    childSubmitted + REVIEW_INITIAL_DELAY
                );
            },
        });

        it({
            id: "T02",
            title: "1 nay → enactment shifts later (submitted + 56 with ease-out curve)",
            test: async () => {
                const { child, childSubmitted } = await delegateToChild(gov.proposers[1]);
                await castVote(api, context, gov.economic[0], child, false);
                await nudge(context);

                const enactedAt = await advanceUntilEnacted(child, 60);
                const expected = childSubmitted + 56;
                expect(enactedAt).to.be.at.least(expected);
                expect(enactedAt).to.be.at.most(expected + 2);
                expect(enactedAt, "later than initial_delay default").to.be.greaterThan(
                    childSubmitted + REVIEW_INITIAL_DELAY
                );
            },
        });

        it({
            id: "T03",
            title: "1 aye + 1 nay (net zero) returns the schedule to submitted + initial_delay",
            test: async () => {
                const { child, childSubmitted } = await delegateToChild(gov.proposers[2]);
                await castVote(api, context, gov.economic[0], child, true);
                await nudge(context);
                await castVote(api, context, gov.economic[1], child, false);
                await nudge(context);

                const enactedAt = await advanceUntilEnacted(child, 45);
                const expected = childSubmitted + REVIEW_INITIAL_DELAY;
                expect(enactedAt).to.be.at.least(expected);
                expect(enactedAt).to.be.at.most(expected + 2);
            },
        });
    },
});
