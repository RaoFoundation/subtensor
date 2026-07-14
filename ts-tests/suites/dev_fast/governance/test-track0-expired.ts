import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import { generateKeyringPair } from "../../../utils/account";
import {
    bootstrapMembership,
    castVote,
    DEV_TRACK,
    type GovernanceMembership,
    getActivePerProposer,
    getStatusKind,
    nudge,
    submitOnTrack,
    systemEvents,
} from "../../../utils/governance";

/**
 * Reachable only with `--features fast-runtime`:
 *   TRIUMVIRATE_DECISION_PERIOD = prod_or_fast!(50_400, 50)
 *
 * A Track 0 referendum that never crosses `approve_threshold` (2/3) or
 * `reject_threshold` (2/3) before the decision period elapses must time
 * out as `Expired`. The deadline alarm is set on submission and re-armed
 * on every `expire_or_rearm_deadline` call until it actually fires at
 * `submitted + decision_period`.
 */
describeSuite({
    id: "DEV_FAST_GOV_TRACK0_EXPIRED_01",
    title: "Governance (fast-runtime) — Track 0 Expired",
    foundationMethods: "dev",
    testCases: ({ it, context }) => {
        let api: ApiPromise;
        let sudoer: KeyringPair;
        let gov: GovernanceMembership;
        const beneficiary = generateKeyringPair("sr25519");

        // Mirrors `runtime/src/governance/tracks.rs` under fast-runtime.
        const TRIUMVIRATE_DECISION_PERIOD = 50;

        beforeAll(async () => {
            api = context.polkadotJs();
            sudoer = context.keyring.alice;
            gov = await bootstrapMembership(api, context, sudoer, {
                triumvirate: 3,
                economic: 1,
                building: 1,
            });

            // Sanity: confirm we're running on a fast-runtime binary. The
            // upgrade test uses the opposite check; mismatched binaries would
            // silently make this test pass for the wrong reason.
            const minimumPeriod = (api.consts.timestamp.minimumPeriod as unknown as { toNumber(): number }).toNumber();
            if (minimumPeriod === 6000) {
                throw new Error(
                    `dev_fast suite requires a binary built with --features fast-runtime (got minimumPeriod=${minimumPeriod})`
                );
            }
        });

        it({
            id: "T01",
            title: "no threshold crossed before decision_period elapses → Expired",
            test: async () => {
                const beforeActive = await getActivePerProposer(api, gov.proposer.address);
                const index = await submitOnTrack(
                    api,
                    context,
                    gov.proposer,
                    DEV_TRACK.TRIUMVIRATE,
                    api.tx.balances.forceSetBalance(beneficiary.address, 7n)
                );

                // 1 aye sits below the 2/3 approve_threshold (≈ 33% vs 66.6%)
                // and rejection stays at 0, so neither threshold can ever
                // fire. The only way out is the deadline.
                await castVote(api, context, gov.triumvirate[0], index, true);
                expect(await getStatusKind(api, index)).to.equal("ongoing");

                // Drive blocks until the status flips to expired, capturing
                // the per-block event log so the Expired event from the
                // transitioning block isn't lost when the system events
                // storage rolls over.
                let expiredEvent: unknown = null;
                for (let i = 0; i < TRIUMVIRATE_DECISION_PERIOD + 10; i++) {
                    const ev = (await systemEvents(api)).find(
                        (e) => e.event.section === "referenda" && e.event.method === "Expired"
                    );
                    if (ev) {
                        expiredEvent = ev;
                        break;
                    }
                    if ((await getStatusKind(api, index)) === "expired") {
                        // Status flipped before we observed the event; still
                        // acceptable — status is the authoritative record.
                        break;
                    }
                    await nudge(context);
                }

                expect(await getStatusKind(api, index)).to.equal("expired");
                expect(expiredEvent, "Expired event observed during polling").to.exist;

                // Expiration is terminal → proposer's slot is released.
                expect(await getActivePerProposer(api, gov.proposer.address)).to.equal(beforeActive);
            },
        });
    },
});
