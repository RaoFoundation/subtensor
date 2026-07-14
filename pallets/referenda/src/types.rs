//! Type definitions for the referenda pallet.

use frame_support::{
    pallet_prelude::*,
    sp_runtime::{Perbill, traits::Zero},
    traits::{Bounded, LockIdentifier, schedule::v3::TaskName},
};
use frame_system::pallet_prelude::*;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{SetLike, VoteTally};

use crate::Config;

/// Maximum length of a track's display name.
pub const MAX_TRACK_NAME_LEN: usize = 32;

/// Fixed-width track name. Padded with zeros if shorter than the maximum.
pub type TrackName = [u8; MAX_TRACK_NAME_LEN];

/// Monotonic referendum identifier. Issued by `submit`.
pub type ReferendumIndex = u32;

/// Hash-keyed name used to identify a scheduler entry.
pub type ProposalTaskName = [u8; 32];

/// Lock identifier reserved by this pallet for any locks placed by the
/// voting layer on behalf of a referendum.
pub const REFERENDA_ID: LockIdentifier = *b"referend";

/// `PalletsOrigin` re-exported from the runtime for use in scheduler calls.
pub type PalletsOriginOf<T> =
    <<T as frame_system::Config>::RuntimeOrigin as OriginTrait>::PalletsOrigin;

pub(crate) type AccountIdOf<T> = <T as frame_system::Config>::AccountId;

/// The runtime call type used for proposed calls and the pallet's own
/// scheduled `advance_referendum` invocations.
pub type CallOf<T> = <T as Config>::RuntimeCall;

/// Bounded reference to a runtime call. Stored on-chain as the preimage
/// hash plus length; the actual call bytes live in the preimage pallet.
pub type BoundedCallOf<T> = Bounded<CallOf<T>, <T as frame_system::Config>::Hashing>;

/// The runtime's track table type.
pub type TracksOf<T> = <T as Config>::Tracks;

/// Stable identifier used to reference a track from referenda and from
/// `ApprovalAction::Review`.
pub type TrackIdOf<T> =
    <TracksOf<T> as TracksInfo<TrackName, AccountIdOf<T>, CallOf<T>, BlockNumberFor<T>>>::Id;

/// The voting scheme tag carried on each track. The voting pallet uses it
/// to dispatch tally updates to the correct backend.
pub type VotingSchemeOf<T> = <TracksOf<T> as TracksInfo<
    TrackName,
    AccountIdOf<T>,
    CallOf<T>,
    BlockNumberFor<T>,
>>::VotingScheme;

/// Set of accounts entitled to vote on referenda on a track.
pub type VoterSetOf<T> =
    <TracksOf<T> as TracksInfo<TrackName, AccountIdOf<T>, CallOf<T>, BlockNumberFor<T>>>::VoterSet;

/// [`ReferendumStatus`] specialized to the runtime configuration.
pub type ReferendumStatusOf<T> =
    ReferendumStatus<AccountIdOf<T>, TrackIdOf<T>, BoundedCallOf<T>, BlockNumberFor<T>>;

/// [`ReferendumInfo`] specialized to the runtime configuration.
pub type ReferendumInfoOf<T> =
    ReferendumInfo<AccountIdOf<T>, TrackIdOf<T>, BoundedCallOf<T>, BlockNumberFor<T>>;

/// What a referendum proposes. Determined by the track's strategy at
/// submit time.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Debug,
)]
pub enum Proposal<Call> {
    /// A call to dispatch on approval. Used by `PassOrFail` tracks.
    Action(Call),
    /// A scheduled call whose timing is governed by votes. Used by
    /// `Adjustable` tracks. The actual call lives on the scheduler under
    /// the referendum's `task_name`; the proposal carries no payload.
    Review,
}

/// How a track decides outcomes for the referenda filed against it.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Debug,
)]
pub enum DecisionStrategy<TrackId, BlockNumber> {
    /// Binary decision before a deadline. The referendum is approved if
    /// `tally.approval` reaches `approve_threshold`, rejected if
    /// `tally.rejection` reaches `reject_threshold`, and expired if neither
    /// happens by `submitted + decision_period`. On approval, the action
    /// in `on_approval` runs.
    PassOrFail {
        /// Number of blocks after submission within which a decision must
        /// be reached. Past this point the referendum expires.
        decision_period: BlockNumber,
        /// Approval ratio required to pass.
        approve_threshold: Perbill,
        /// Rejection ratio required to fail.
        reject_threshold: Perbill,
        /// Action taken once the referendum is approved.
        on_approval: ApprovalAction<TrackId>,
    },
    /// Timing decision over a call already scheduled at submit time. The
    /// call runs after `initial_delay` by default. Voters can fast-track,
    /// cancel, or shift the dispatch time via interpolation on net votes:
    /// net approval pulls the target earlier toward `submitted`, net
    /// rejection pushes it later toward `submitted + max_delay`.
    Adjustable {
        /// Default delay between submission and dispatch when net votes
        /// are zero.
        initial_delay: BlockNumber,
        /// Upper bound on the dispatch delay. Reached as net rejection
        /// approaches `cancel_threshold`. Must be `>= initial_delay`;
        /// equal disables the rejection-side extension.
        max_delay: BlockNumber,
        /// Approval ratio at which the task is rescheduled to next block
        /// and the referendum concludes as `FastTracked`.
        fast_track_threshold: Perbill,
        /// Rejection ratio at which the scheduled task is cancelled and the
        /// referendum concludes as `Cancelled`.
        cancel_threshold: Perbill,
    },
}

/// What happens when a `PassOrFail` referendum is approved.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Debug,
)]
pub enum ApprovalAction<TrackId> {
    /// Schedule the call for next-block dispatch on this referendum's index.
    Execute,
    /// Hand the call off to a fresh `Adjustable` referendum on `track`.
    /// The parent concludes as `Delegated` and the new referendum drives
    /// the rest of the lifecycle.
    Review {
        /// Target track for the review referendum. Must be `Adjustable`;
        /// validated by [`Pallet::integrity_test`].
        track: TrackId,
    },
}

/// Per-track configuration carried in the runtime track table.
#[derive(Clone, Debug)]
pub struct TrackInfo<TrackId, Name, BlockNumber, ProposerSet, VoterSet, VotingScheme> {
    /// Display name. Padded to fixed width.
    pub name: Name,
    /// Accounts allowed to submit referenda on this track. `None` means
    /// the track is currently closed to new submissions; existing
    /// referenda continue their lifecycle normally.
    pub proposer_set: Option<ProposerSet>,
    /// Voting scheme tag. Routes tally updates to the correct backend.
    pub voting_scheme: VotingScheme,
    /// Accounts entitled to vote on referenda on this track.
    pub voter_set: VoterSet,
    /// How outcomes are decided on this track.
    pub decision_strategy: DecisionStrategy<TrackId, BlockNumber>,
}

/// A track entry in the runtime track table: an id paired with its
/// configuration.
#[derive(Clone, Debug)]
pub struct Track<Id, Name, BlockNumber, ProposerSet, VoterSet, VotingScheme> {
    /// Stable id used to reference this track from referenda and from
    /// `ApprovalAction::Review { track }`.
    pub id: Id,
    /// Track configuration.
    pub info: TrackInfo<Id, Name, BlockNumber, ProposerSet, VoterSet, VotingScheme>,
}

/// Runtime configuration of available tracks. Implementors define the
/// available tracks at compile time; the pallet queries this trait at
/// submit time and during state-machine evaluation.
pub trait TracksInfo<Name, AccountId, Call, BlockNumber> {
    /// Stable identifier for a track.
    type Id: Parameter + MaxEncodedLen + Copy + Ord + PartialOrd + Send + Sync + 'static;
    /// Accounts allowed to submit referenda.
    type ProposerSet: SetLike<AccountId>;
    /// Voting scheme tag carried on each track.
    type VotingScheme: PartialEq;
    /// Accounts entitled to vote.
    type VoterSet: SetLike<AccountId>;

    /// Iterate over every track defined in the runtime.
    fn tracks() -> impl Iterator<
        Item = Track<
            Self::Id,
            Name,
            BlockNumber,
            Self::ProposerSet,
            Self::VoterSet,
            Self::VotingScheme,
        >,
    >;

    /// Look up the configuration for a single track id.
    fn info(
        id: Self::Id,
    ) -> Option<
        TrackInfo<
            Self::Id,
            Name,
            BlockNumber,
            Self::ProposerSet,
            Self::VoterSet,
            Self::VotingScheme,
        >,
    > {
        Self::tracks().find(|t| t.id == id).map(|t| t.info)
    }

    /// Optional per-track authorization of a proposed call. Defaults to
    /// allow-all. Runtimes can override to filter calls based on track.
    fn authorize_proposal(
        _track_info: &TrackInfo<
            Self::Id,
            Name,
            BlockNumber,
            Self::ProposerSet,
            Self::VoterSet,
            Self::VotingScheme,
        >,
        _call: &Call,
    ) -> bool {
        true
    }

    /// Validate the runtime track table once at startup. Returns `Err`
    /// with a static message describing the first broken invariant.
    ///
    /// Structural invariants:
    ///
    /// 1. Track ids are unique. Lookups by id silently pick the first
    ///    match, so duplicates would mask later entries.
    /// 2. Every `ApprovalAction::Review { track }` references a track
    ///    that exists and uses the `Adjustable` strategy. Otherwise an
    ///    approval that delegates would either find no track or hand off
    ///    to a track that cannot model a review.
    ///
    /// Per-strategy parameter invariants (the threshold comparisons in
    /// `advance_ongoing` are `>=`, so a zero threshold against the
    /// default-zero tally auto-concludes on first alarm fire):
    ///
    /// * `PassOrFail`: `decision_period`, `approve_threshold`, and
    ///   `reject_threshold` must all be non-zero;
    ///   `approve_threshold + reject_threshold > 100%` so the reject
    ///   branch cannot be masked by an approval that fires first on the
    ///   same tally split.
    /// * `Adjustable`: `initial_delay`, `fast_track_threshold`, and
    ///   `cancel_threshold` must all be non-zero;
    ///   `max_delay >= initial_delay` (else net rejection cannot extend
    ///   the delay); and `fast_track_threshold + cancel_threshold > 100%`
    ///   so the cancel branch cannot be masked by a fast-track that
    ///   fires first on the same tally split.
    fn check_integrity() -> Result<(), &'static str>
    where
        BlockNumber: Zero + PartialOrd,
    {
        let tracks: alloc::vec::Vec<_> = Self::tracks().collect();

        let mut ids: alloc::vec::Vec<_> = tracks.iter().map(|t| t.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() != total {
            return Err("track ids must be unique");
        }

        for track in &tracks {
            match &track.info.decision_strategy {
                DecisionStrategy::PassOrFail {
                    decision_period,
                    approve_threshold,
                    reject_threshold,
                    on_approval,
                } => {
                    if decision_period.is_zero() {
                        return Err("PassOrFail: decision_period must be non-zero");
                    }
                    if *approve_threshold == Perbill::zero() {
                        return Err("PassOrFail: approve_threshold must be non-zero");
                    }
                    if *reject_threshold == Perbill::zero() {
                        return Err("PassOrFail: reject_threshold must be non-zero");
                    }
                    let sum = approve_threshold
                        .deconstruct()
                        .saturating_add(reject_threshold.deconstruct());
                    if sum <= Perbill::one().deconstruct() {
                        return Err(
                            "PassOrFail: approve_threshold + reject_threshold must exceed 100%",
                        );
                    }
                    if let ApprovalAction::Review {
                        track: review_track,
                    } = on_approval
                    {
                        let referenced = Self::info(*review_track)
                            .ok_or("ApprovalAction::Review references unknown track")?;
                        if !matches!(
                            referenced.decision_strategy,
                            DecisionStrategy::Adjustable { .. }
                        ) {
                            return Err("ApprovalAction::Review target track must be Adjustable");
                        }
                    }
                }
                DecisionStrategy::Adjustable {
                    initial_delay,
                    max_delay,
                    fast_track_threshold,
                    cancel_threshold,
                } => {
                    if initial_delay.is_zero() {
                        return Err("Adjustable: initial_delay must be non-zero");
                    }
                    if max_delay < initial_delay {
                        return Err("Adjustable: max_delay must be >= initial_delay");
                    }
                    if *fast_track_threshold == Perbill::zero() {
                        return Err("Adjustable: fast_track_threshold must be non-zero");
                    }
                    if *cancel_threshold == Perbill::zero() {
                        return Err("Adjustable: cancel_threshold must be non-zero");
                    }
                    let sum = fast_track_threshold
                        .deconstruct()
                        .saturating_add(cancel_threshold.deconstruct());
                    if sum <= Perbill::one().deconstruct() {
                        return Err(
                            "Adjustable: fast_track_threshold + cancel_threshold must exceed 100%",
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

/// Curve applied to net-vote progress on `Adjustable` tracks. Maps
/// `progress` (the position of the net vote between zero and the
/// side-specific threshold) to the fraction of the delay range to
/// apply.
pub trait AdjustmentCurve {
    fn apply(progress: Perbill) -> Perbill;
}

/// Per-referendum data captured at submit time and updated as votes arrive.
#[freeze_struct("b7609aee357fa7ab")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Debug,
)]
pub struct ReferendumInfo<AccountId, TrackId, Call, BlockNumber> {
    /// Track this referendum was filed against.
    pub track: TrackId,
    /// What this referendum proposes.
    pub proposal: Proposal<Call>,
    /// Account that submitted the referendum.
    pub proposer: AccountId,
    /// Submission block. Anchors timing computations in `Adjustable`
    /// strategies.
    pub submitted: BlockNumber,
    /// Latest tally observed from the voting layer.
    pub tally: VoteTally,
    /// Snapshot of the track's decision strategy taken at submit time.
    /// State-machine evaluation reads from this snapshot, so a runtime
    /// upgrade that changes track config does not change the rules under
    /// which a live referendum resolves.
    pub decision_strategy: DecisionStrategy<TrackId, BlockNumber>,
}

/// Lifecycle status of a referendum. Each terminal variant carries the
/// block number at which it was reached.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, Clone, PartialEq, Eq, TypeInfo, Debug,
)]
pub enum ReferendumStatus<AccountId, Id, Call, BlockNumber> {
    /// Voting is in progress.
    Ongoing(ReferendumInfo<AccountId, Id, Call, BlockNumber>),
    /// Approval threshold reached on a `PassOrFail` track. The call has
    /// been scheduled for dispatch on this referendum's index. Transitions
    /// to [`Enacted`](Self::Enacted) once the scheduled task has run.
    Approved(BlockNumber),
    /// Approval reached with `ApprovalAction::Review`. The call now lives
    /// on a fresh referendum on the configured review track; this index
    /// is a terminal audit trail.
    Delegated(BlockNumber),
    /// Rejection threshold reached on a `PassOrFail` track.
    Rejected(BlockNumber),
    /// Decision period elapsed without crossing approve or reject
    /// thresholds.
    Expired(BlockNumber),
    /// Fast-track threshold reached on an `Adjustable` track. The
    /// scheduled task was rescheduled to next block. Transitions to
    /// [`Enacted`](Self::Enacted).
    FastTracked(BlockNumber),
    /// Cancel threshold reached on an `Adjustable` track. The scheduled
    /// task was cancelled.
    Cancelled(BlockNumber),
    /// The dispatch attempt completed. Terminal regardless of whether
    /// the inner call returned `Ok` or `Err`.
    Enacted(BlockNumber),
    /// Terminated by [`Config::KillOrigin`](crate::Config::KillOrigin)
    /// before reaching a vote-driven outcome.
    Killed(BlockNumber),
}

/// Stable scheduler name for a referendum's enactment task.
pub fn task_name(index: ReferendumIndex) -> TaskName {
    (REFERENDA_ID, "enactment", index).using_encoded(sp_io::hashing::blake2_256)
}

/// Stable scheduler name for a referendum's alarm.
pub fn alarm_name(index: ReferendumIndex) -> TaskName {
    (REFERENDA_ID, "alarm", index).using_encoded(sp_io::hashing::blake2_256)
}
