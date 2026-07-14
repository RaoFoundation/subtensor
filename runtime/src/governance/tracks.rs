//! Static governance tracks: Triumvirate approval, then collective review.

use pallet_referenda::{
    AdjustmentCurve, ApprovalAction, DecisionStrategy, MAX_TRACK_NAME_LEN, Track as RefTrack,
    TrackInfo as RefTrackInfo, TracksInfo as RefTracksInfo,
};
use runtime_common::prod_or_fast;
use safe_math::SafeDiv;
use sp_runtime::{Perbill, traits::UniqueSaturatedInto};
use subtensor_runtime_common::{
    pad_name,
    time::{DAYS, HOURS},
};

use super::collectives::CollectiveId;
use super::{MemberSet, VotingScheme};
use crate::{AccountId, BlockNumber, RuntimeCall};

const TRIUMVIRATE_DECISION_PERIOD: BlockNumber = prod_or_fast!(7 * DAYS, 50);

const REVIEW_INITIAL_DELAY: BlockNumber = prod_or_fast!(24 * HOURS, 30);

const TRIUMVIRATE_TRACK_ID: u8 = 0;
const REVIEW_TRACK_ID: u8 = 1;

/// Upper bound on the Review dispatch delay, reached as net rejection
/// approaches `cancel_threshold`.
const REVIEW_MAX_DELAY: BlockNumber = prod_or_fast!(2 * DAYS, 60);

/// Ease-out curve for review delay adjustment: `1 - (1 - p)^3`.
///
/// Early collective signal has a visible effect on the dispatch time, while
/// additional votes near the threshold taper off before the hard fast-track
/// or cancel threshold concludes the referendum.
pub struct EaseOutAdjustmentCurve;
impl AdjustmentCurve for EaseOutAdjustmentCurve {
    fn apply(progress: Perbill) -> Perbill {
        let scale = u128::from(Perbill::from_percent(100).deconstruct());
        let remaining = scale.saturating_sub(u128::from(progress.deconstruct()));
        let remaining_cubed = remaining
            .saturating_mul(remaining)
            .saturating_mul(remaining)
            .safe_div(scale)
            .safe_div(scale);
        let curved = scale.saturating_sub(remaining_cubed);

        Perbill::from_parts(curved.min(scale).unique_saturated_into())
    }
}

pub struct Tracks;
impl RefTracksInfo<[u8; MAX_TRACK_NAME_LEN], AccountId, RuntimeCall, BlockNumber> for Tracks {
    type Id = u8;
    type ProposerSet = MemberSet;
    type VotingScheme = VotingScheme;
    type VoterSet = MemberSet;

    fn tracks() -> impl Iterator<
        Item = RefTrack<
            Self::Id,
            [u8; MAX_TRACK_NAME_LEN],
            BlockNumber,
            Self::ProposerSet,
            Self::VoterSet,
            Self::VotingScheme,
        >,
    > {
        [
            RefTrack {
                id: TRIUMVIRATE_TRACK_ID,
                info: RefTrackInfo {
                    name: pad_name(b"triumvirate"),
                    proposer_set: Some(MemberSet::Single(CollectiveId::Proposers)),
                    voter_set: MemberSet::Single(CollectiveId::Triumvirate),
                    voting_scheme: VotingScheme::Signed,
                    decision_strategy: DecisionStrategy::PassOrFail {
                        decision_period: TRIUMVIRATE_DECISION_PERIOD,
                        approve_threshold: Perbill::from_rational(2u32, 3u32),
                        reject_threshold: Perbill::from_rational(2u32, 3u32),
                        // Triumvirate approval still gets a wider review
                        // window before enactment.
                        on_approval: ApprovalAction::Review {
                            track: REVIEW_TRACK_ID,
                        },
                    },
                },
            },
            // `proposer_set: None` is load-bearing: it makes track 1 reachable
            // only via Track 0's `ApprovalAction::Review` handoff. Setting it
            // to `Some(_)` would let a proposer schedule a root call for
            // auto-dispatch at `now + initial_delay`, bypassing Triumvirate
            // approval.
            RefTrack {
                id: REVIEW_TRACK_ID,
                info: RefTrackInfo {
                    name: pad_name(b"review"),
                    proposer_set: None,
                    voter_set: MemberSet::Union(alloc::vec![
                        CollectiveId::Economic,
                        CollectiveId::Building,
                    ]),
                    voting_scheme: VotingScheme::Signed,
                    decision_strategy: DecisionStrategy::Adjustable {
                        initial_delay: REVIEW_INITIAL_DELAY,
                        max_delay: REVIEW_MAX_DELAY,
                        fast_track_threshold: Perbill::from_percent(75),
                        cancel_threshold: Perbill::from_percent(51),
                    },
                },
            },
        ]
        .into_iter()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pallet_referenda::TracksInfo;

    fn track(
        id: u8,
    ) -> RefTrack<u8, [u8; MAX_TRACK_NAME_LEN], BlockNumber, MemberSet, MemberSet, VotingScheme>
    {
        Tracks::tracks()
            .find(|track| track.id == id)
            .expect("track must exist")
    }

    #[test]
    fn track_0_triumvirate_is_directly_submittable() {
        let track_0 = track(TRIUMVIRATE_TRACK_ID);

        assert!(
            track_0.info.proposer_set.is_some(),
            "track 0 must have a proposer_set; without it there is no \
             on-chain entry point into governance."
        );

        match track_0.info.decision_strategy {
            DecisionStrategy::PassOrFail {
                on_approval: ApprovalAction::Review { track },
                ..
            } => assert_eq!(
                track, REVIEW_TRACK_ID,
                "track 0 approval must hand off to the review track"
            ),
            other => panic!("track 0 must stay PassOrFail with review handoff, got {other:?}"),
        }
    }

    #[test]
    fn track_1_review_is_not_directly_submittable() {
        let track_1 = track(REVIEW_TRACK_ID);

        assert!(
            track_1.info.proposer_set.is_none(),
            "track 1 must have proposer_set: None; Some(_) would let a \
             proposer schedule a root call without Triumvirate approval."
        );
    }

    #[test]
    fn ease_out_curve_uses_cubic_complement() {
        assert_eq!(
            EaseOutAdjustmentCurve::apply(Perbill::from_percent(0)),
            Perbill::from_percent(0),
        );
        assert_eq!(
            EaseOutAdjustmentCurve::apply(Perbill::from_percent(50)),
            Perbill::from_rational(7u32, 8u32),
        );
        assert_eq!(
            EaseOutAdjustmentCurve::apply(Perbill::from_percent(100)),
            Perbill::from_percent(100),
        );
    }
}
