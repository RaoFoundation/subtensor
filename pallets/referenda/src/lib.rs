#![cfg_attr(not(feature = "std"), no_std)]

//! # Referenda
//!
//! Track-based on-chain referenda with two decision strategies.
//!
//! ## Tracks
//!
//! Each referendum is filed against a `Track` defined by the runtime via the
//! [`TracksInfo`] trait. A track carries the proposer set, the voter set, the
//! voting scheme, and the decision strategy. Two strategies are supported:
//!
//! * `PassOrFail`: a binary decision before a deadline. Submitters provide a
//!   call. On approval the call is dispatched (either directly, or handed off
//!   to an `Adjustable` review track via `ApprovalAction::Review`).
//! * `Adjustable`: a timing decision over an already-scheduled call. The call
//!   runs after `initial_delay` by default. Voters can fast-track it sooner,
//!   cancel it entirely, or shift the dispatch time via a curve-shaped
//!   interpolation on net votes.
//!
//! ## Lifecycle
//!
//! `submit` records a referendum, schedules the relevant scheduler entries
//! (an alarm for `PassOrFail`; an enactment task for `Adjustable`), and
//! notifies subscribers via [`OnPollCreated::on_poll_created`].
//!
//! Tally updates arrive through [`Polls::on_tally_updated`]. The hook is
//! intentionally side-effect-light: it stores the new tally and arms an
//! alarm at `now + 1`. All decision logic runs from the alarm via
//! `advance_referendum`, which keeps the tally hook free of re-entrancy.
//!
//! `advance_referendum` is the single state-machine entry point. For an
//! `Ongoing` referendum it dispatches into the appropriate threshold or
//! timing logic; on terminal statuses it is a no-op.
//!
//! ## Dispatch wrapping
//!
//! Approval (Execute) and Adjustable submission both schedule a wrapper
//! call `Pallet::enact(index, call)` rather than the governed call
//! directly. The scheduler invokes the wrapper with `RawOrigin::Root` at
//! the configured time; `enact` dispatches the inner call and marks the
//! referendum `Enacted` in the same call. Dispatch and `Enacted` are
//! atomic; the pallet never has to infer dispatch from scheduler-internal
//! state. `enact` no-ops on terminal-no-dispatch statuses, so a stale
//! wrapper task that fires after a failed scheduler cancel (e.g. inside
//! `kill` or `do_cancel`) cannot dispatch. The submit-time preimage is
//! dropped at scheduling time since the wrapper is the sole reference to
//! the inner call from then on.
//!
//! ## State machine
//!
//! `PassOrFail` track:
//!
//! ```text
//!                            submit
//!                              │
//!                              ▼
//!     vote re-arms alarm   ┌───────┐   kill
//!     (now + 1)         ┌─►│Ongoing│───────────────────────────► Killed    (terminal)
//!                       │  └───┬───┘
//!                       │      │
//!                       │      │ alarm fires:
//!                       │      ├─ approve_threshold + Execute  ─► Approved ─► enact ─► Enacted
//!                       │      ├─ approve_threshold + Review   ─► Delegated (terminal)
//!                       │      ├─ reject_threshold             ─► Rejected  (terminal)
//!                       │      ├─ deadline reached             ─► Expired   (terminal)
//!                       │      └─ no decision, before deadline ─► re-arm at deadline,
//!                       └──────┘                                  stay Ongoing
//! ```
//!
//! `Adjustable` track:
//!
//! ```text
//!                            submit
//!                              │
//!                              │ schedule enact(index) at submitted + initial_delay
//!                              ▼
//!     vote re-arms alarm   ┌───────┐   kill
//!     (now + 1)         ┌─►│Ongoing│───────────────────────────► Killed    (terminal)
//!                       │  └───┬───┘
//!                       │      │
//!                       │      ├─ enact fires (natural)        ─► Enacted     (terminal)
//!                       │      │ alarm fires:
//!                       │      ├─ fast_track_threshold        ─► FastTracked ─► enact ─► Enacted
//!                       │      ├─ cancel_threshold            ─► Cancelled   (terminal)
//!                       │      └─ otherwise: do_adjust_delay  ─► move enact task (earlier
//!                       └──────┘                                 on net approval, later on
//!                                                                net rejection), stay Ongoing
//! ```
//!
//! `kill` is also accepted from `Approved` (PassOrFail) and
//! `FastTracked` (Adjustable) until `enact` dispatches: the wrapper task
//! is cancelled and the inner call never runs.
//!
//! ## Status taxonomy
//!
//! * `Ongoing`: voting in progress.
//! * `Approved`: vote crossed `approve_threshold` on a `PassOrFail` track
//!   with `ApprovalAction::Execute`. The `enact(index)` wrapper is
//!   scheduled on this index and will mark `Enacted` when it dispatches.
//! * `Delegated`: vote crossed `approve_threshold` on a `PassOrFail` track
//!   with `ApprovalAction::Review`. The call now lives on a fresh
//!   referendum on the configured review track; this index is a terminal
//!   audit trail.
//! * `Rejected`: vote crossed `reject_threshold` on a `PassOrFail` track.
//! * `Expired`: `PassOrFail` decision period elapsed without crossing
//!   either threshold.
//! * `FastTracked`: vote crossed `fast_track_threshold` on an `Adjustable`
//!   track. Wrapper rescheduled to next block; marks `Enacted` on dispatch.
//! * `Cancelled`: vote crossed `cancel_threshold` on an `Adjustable`
//!   track. Wrapper cancelled and [`EnactmentTask`] cleared.
//! * `Enacted`: the dispatch attempt completed. The `Enacted` event
//!   carries the inner call's result via an `Option<DispatchError>`.
//! * `Killed`: privileged termination via `KillOrigin`.
//!
//! ## Alarm and task discipline
//!
//! Each referendum has at most one alarm (`alarm_name(index)`) and at
//! most one enactment task (`task_name(index)`). [`set_alarm`] is
//! idempotent: it reschedules an existing alarm when possible and only
//! schedules a fresh alarm when none is pending.
//!
//! `Adjustable` enactment tasks can move earlier or later than the
//! initial schedule via interpolation on net votes (see
//! `do_adjust_delay`): net approval shrinks the delay toward zero,
//! net rejection extends it toward the track's `max_delay` before
//! the cancel threshold fires. The mapping from net-vote progress to
//! delay fraction is shaped by [`Config::AdjustmentCurve`], which the
//! runtime supplies; the pallet itself stays curve-agnostic.
//!
//! ## Runtime configuration check
//!
//! [`Pallet::integrity_test`] runs at startup and asserts that the track
//! table is well-formed:
//!
//! * Track ids are unique.
//! * Every `ApprovalAction::Review { track }` references a track that
//!   exists and uses the `Adjustable` strategy.
//! * `PassOrFail` tracks have non-zero `decision_period`,
//!   `approve_threshold`, and `reject_threshold`;
//!   `approve_threshold + reject_threshold > 100%` so the reject branch
//!   cannot be masked by an approval that fires first on the same tally
//!   split.
//! * `Adjustable` tracks have non-zero `initial_delay`,
//!   `fast_track_threshold`, and `cancel_threshold`;
//!   `max_delay >= initial_delay`; and
//!   `fast_track_threshold + cancel_threshold > 100%` so the cancel
//!   branch cannot be masked by a fast-track that fires first on the
//!   same tally split.
//!
//! A misconfigured runtime panics at boot with a precise cause.
//!
//! ## Track-config snapshotting
//!
//! `submit` snapshots the track's [`DecisionStrategy`] into
//! [`ReferendumInfo`]. State-machine evaluation reads the snapshot, not
//! the live track table. Runtime upgrades that change thresholds, swap
//! strategy, or remove a track therefore only affect *new* submissions;
//! live referenda continue to resolve under the rules they started with.
//!
//! Voter-set membership stays dynamic by design (collective members
//! naturally come and go), so percentages reflect current membership.
//!
//! Removing a track from the runtime is safe for the state machine but
//! freezes the tally on any in-flight referendum (signed-voting refuses
//! new votes when [`Polls::voter_set_of`] returns `None`). All paths are
//! still terminal: PassOrFail resolves on the frozen tally or expires at
//! `decision_period`; Adjustable runs at `initial_delay`. To drop a
//! track cleanly, ship a migration that resolves (kills, concludes, or
//! reassigns) live referenda on that track before the upgrade.

extern crate alloc;

use alloc::boxed::Box;
use frame_support::{
    dispatch::{DispatchResult, GetDispatchInfo},
    pallet_prelude::*,
    sp_runtime::{
        Perbill, Saturating,
        traits::{BlockNumberProvider, Dispatchable, One, Zero},
    },
    traits::{
        QueryPreimage, StorePreimage,
        schedule::{DispatchTime, v3::Named as ScheduleNamed},
    },
};
use frame_system::pallet_prelude::*;
use subtensor_runtime_common::{OnPollCompleted, OnPollCreated, Polls, SetLike, VoteTally};

pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod types;
pub mod weights;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    // Pinned to 0 to satisfy try-runtime CLI's pre/post-upgrade checks.
    // The project tracks migrations via a per-pallet `HasMigrationRun` map
    // so this value is not bumped on schema changes.
    const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The aggregate runtime call type. Submitted calls and the
        /// pallet's own `advance_referendum` are dispatched through this.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin>
            + GetDispatchInfo
            + From<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>
            + From<frame_system::Call<Self>>;

        /// Named scheduler used to queue enactment tasks and alarms. Each
        /// referendum has at most one task and one alarm, identified by
        /// the names produced by [`task_name`] and [`alarm_name`].
        type Scheduler: ScheduleNamed<
                BlockNumberFor<Self>,
                CallOf<Self>,
                PalletsOriginOf<Self>,
                Hasher = Self::Hashing,
            >;

        /// Preimage provider used to bound submitted calls into a
        /// content-addressed reference and to bound the pallet's own
        /// `advance_referendum` call when scheduling alarms.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// Maximum number of simultaneously-active referenda. Submission is
        /// rejected with [`Error::QueueFull`] when this is reached.
        type MaxQueued: Get<u32>;

        /// Maximum number of simultaneously-active referenda that a single
        /// proposer may hold. Bounds the queue surface a single account can
        /// occupy when many proposers compete for [`MaxQueued`] slots.
        type MaxActivePerProposer: Get<u32>;

        /// Origin authorized to terminate an ongoing referendum via `kill`.
        type KillOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Track configuration. Defines the proposer set, voter set, voting
        /// scheme, and decision strategy for each track id.
        type Tracks: TracksInfo<TrackName, Self::AccountId, CallOf<Self>, BlockNumberFor<Self>>;

        /// Curve applied to net-vote progress on `Adjustable` tracks. Not
        /// snapshotted: a runtime upgrade that swaps the impl affects all
        /// in-flight referenda.
        type AdjustmentCurve: AdjustmentCurve;

        /// Source of "now" used for scheduling decisions. Typically
        /// `frame_system::Pallet<T>`; configurable for runtimes that
        /// expose a different block-number authority.
        type BlockNumberProvider: BlockNumberProvider<BlockNumber = BlockNumberFor<Self>>;

        /// Subscriber notified when a new referendum is created. The hook
        /// returns its actual weight; the pallet pre-charges
        /// `OnPollCreated::weight()` and refunds the unused portion.
        type OnPollCreated: OnPollCreated<ReferendumIndex>;

        /// Subscriber notified when a referendum reaches a terminal status.
        /// Same weight contract as [`OnPollCreated`].
        type OnPollCompleted: OnPollCompleted<ReferendumIndex>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// Helper for setting up cross-pallet state needed by benchmarks.
        /// The runtime provides track ids of each strategy variant plus a
        /// proposer guaranteed to be in those tracks' proposer sets.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: BenchmarkHelper<TrackIdOf<Self>, Self::AccountId, CallOf<Self>>;
    }

    /// Benchmark setup helper. The runtime wires this with track ids and a
    /// proposer that match its track table; the mock provides defaults
    /// matching `pallet-referenda::mock::TestTracks`.
    ///
    /// Note: only a `PassOrFail` track is needed for the approve benchmark
    /// because the `Review` outcome is the worst case and bounds `Execute`
    /// from above (see [`weights::WeightInfo`]).
    #[cfg(feature = "runtime-benchmarks")]
    pub trait BenchmarkHelper<TrackId, AccountId, Call> {
        /// Track id of a `PassOrFail` track. The benchmark drives both the
        /// approve and reject paths through it.
        fn track_passorfail() -> TrackId;
        /// Track id of an `Adjustable` track.
        fn track_adjustable() -> TrackId;
        /// Account in the proposer set of both tracks returned above.
        fn proposer() -> AccountId;
        /// Seed collective members that we need for benchmarks.
        fn seed_collective_members();
        /// A call that `T::Tracks::authorize_proposal` accepts. Should be
        /// cheap to bound (e.g. `frame_system::remark`).
        fn call() -> Call;
    }

    /// Monotonic referendum id generator. Incremented by `submit`; never
    /// decremented. Existing referenda continue to be identified by their
    /// assigned id even after the count moves on.
    #[pallet::storage]
    pub type ReferendumCount<T: Config> = StorageValue<_, ReferendumIndex, ValueQuery>;

    /// Number of currently-ongoing referenda. Bounded by [`Config::MaxQueued`]
    /// and used as the capacity check at submit time. Distinct from
    /// [`ReferendumCount`], which only ever grows.
    #[pallet::storage]
    pub type ActiveCount<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Per-proposer count of currently-ongoing referenda. Bounded by
    /// [`Config::MaxActivePerProposer`].
    #[pallet::storage]
    pub type ActivePerProposer<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    /// Status of every referendum that has been submitted, keyed by index.
    /// Entries persist after the referendum reaches a terminal state so the
    /// outcome remains queryable for audit.
    #[pallet::storage]
    pub type ReferendumStatusFor<T: Config> =
        StorageMap<_, Blake2_128Concat, ReferendumIndex, ReferendumStatusOf<T>, OptionQuery>;

    /// Wrapper preimage handle for any referendum with a scheduled enactment
    /// task. Present iff `task_name(index)` is currently in the scheduler's
    /// agenda. Used to release the scheduler's preimage ref on cancel paths,
    /// since `Scheduler::cancel_named` via the trait API does not drop the
    /// preimage it requested at schedule time.
    #[pallet::storage]
    pub type EnactmentTask<T: Config> =
        StorageMap<_, Blake2_128Concat, ReferendumIndex, BoundedCallOf<T>, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new referendum was submitted.
        Submitted {
            /// Index assigned to the new referendum.
            index: ReferendumIndex,
            /// Track the referendum was filed against.
            track: TrackIdOf<T>,
            /// Account that submitted the referendum.
            proposer: T::AccountId,
        },
        /// The approval threshold was crossed and the call has been
        /// scheduled for direct dispatch.
        Approved {
            /// Referendum that was approved.
            index: ReferendumIndex,
        },
        /// The approval threshold was crossed and the call was handed
        /// off to a child review referendum.
        Delegated {
            /// Parent referendum that approved the handoff.
            index: ReferendumIndex,
            /// New referendum that now carries the call.
            review: ReferendumIndex,
            /// Track the new referendum was filed against.
            track: TrackIdOf<T>,
        },
        /// Approval was reached on a review handoff but the child
        /// referendum could not be created. The parent stays ongoing
        /// and will retry on the next vote or expire at its deadline.
        ReviewSchedulingFailed {
            /// Parent referendum whose handoff failed.
            index: ReferendumIndex,
            /// Track the handoff was attempting to file against.
            track: TrackIdOf<T>,
        },
        /// The rejection threshold was crossed.
        Rejected {
            /// Referendum that was rejected.
            index: ReferendumIndex,
        },
        /// The cancel threshold was crossed and the scheduled call has
        /// been cancelled.
        Cancelled {
            /// Referendum that was cancelled.
            index: ReferendumIndex,
        },
        /// The referendum was terminated by a privileged origin before
        /// dispatch.
        Killed {
            /// Referendum that was killed.
            index: ReferendumIndex,
        },
        /// The decision period elapsed without crossing the approve or
        /// reject threshold.
        Expired {
            /// Referendum that expired.
            index: ReferendumIndex,
        },
        /// The fast-track threshold was crossed and the call now runs
        /// in the next block.
        FastTracked {
            /// Referendum that was fast-tracked.
            index: ReferendumIndex,
        },
        /// The dispatch attempt completed.
        Enacted {
            /// Referendum that was enacted.
            index: ReferendumIndex,
            /// Block at which dispatch ran.
            when: BlockNumberFor<T>,
            /// `None` if the inner call returned `Ok`, otherwise the
            /// failure returned by the dispatch.
            error: Option<DispatchError>,
        },
        /// A scheduler operation failed. Surfaced for observability;
        /// the pallet does not roll back the surrounding state change.
        SchedulerOperationFailed {
            /// Referendum the failed operation was acting on.
            index: ReferendumIndex,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The specified track does not exist.
        BadTrack,
        /// The track has no proposer set configured.
        TrackNotSubmittable,
        /// The caller is not in the track's proposer set.
        NotProposer,
        /// The referendum has already concluded.
        ReferendumFinalized,
        /// The proposal is not authorized for this track.
        ProposalNotAuthorized,
        /// The active-referenda cap has been reached.
        QueueFull,
        /// The per-proposer active-referenda cap has been reached.
        ProposerQuotaExceeded,
        /// A scheduler operation failed at submit time.
        SchedulerError,
        /// The specified referendum does not exist.
        ReferendumNotFound,
        /// Reached a state combination that should be prevented by
        /// submit-time invariants. Indicates a configuration mismatch.
        Unreachable,
        /// The track's voter set is empty. With no eligible voters the
        /// tally would freeze at zero and the referendum would resolve
        /// to a pre-determined outcome.
        EmptyVoterSet,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            T::Tracks::check_integrity().expect("pallet-referenda: invalid track configuration");
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(
            _n: BlockNumberFor<T>,
        ) -> Result<(), frame_support::sp_runtime::TryRuntimeError> {
            Pallet::<T>::do_try_state()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Submit a new referendum on `track` carrying `call`. On a
        /// pass-or-fail track the call is held until the approval
        /// threshold is reached; on an adjustable track the call is
        /// scheduled for dispatch immediately and voting only adjusts
        /// when it runs.
        #[pallet::call_index(0)]
        #[pallet::weight(
            T::WeightInfo::submit().saturating_add(T::OnPollCreated::weight())
        )]
        pub fn submit(
            origin: OriginFor<T>,
            track: TrackIdOf<T>,
            call: Box<CallOf<T>>,
        ) -> DispatchResult {
            let proposer = ensure_signed(origin)?;
            let track_info = T::Tracks::info(track).ok_or(Error::<T>::BadTrack)?;

            let Some(ref proposer_set) = track_info.proposer_set else {
                return Err(Error::<T>::TrackNotSubmittable.into());
            };
            ensure!(proposer_set.contains(&proposer), Error::<T>::NotProposer);
            ensure!(
                T::Tracks::authorize_proposal(&track_info, &call),
                Error::<T>::ProposalNotAuthorized
            );
            ensure!(!track_info.voter_set.is_empty(), Error::<T>::EmptyVoterSet);
            let active = ActiveCount::<T>::get();
            ensure!(active < T::MaxQueued::get(), Error::<T>::QueueFull);
            let active_per_proposer = ActivePerProposer::<T>::get(&proposer);
            ensure!(
                active_per_proposer < T::MaxActivePerProposer::get(),
                Error::<T>::ProposerQuotaExceeded
            );

            let now = T::BlockNumberProvider::current_block_number();
            let index = ReferendumCount::<T>::get();
            ReferendumCount::<T>::put(index.saturating_add(1));
            ActiveCount::<T>::put(active.saturating_add(1));
            ActivePerProposer::<T>::insert(&proposer, active_per_proposer.saturating_add(1));

            let proposal = match &track_info.decision_strategy {
                DecisionStrategy::PassOrFail {
                    decision_period, ..
                } => {
                    let when = now.saturating_add(*decision_period);
                    Self::set_alarm(index, when)?;
                    let bounded_call = T::Preimages::bound(*call)?;
                    Proposal::Action(bounded_call)
                }
                DecisionStrategy::Adjustable { initial_delay, .. } => {
                    let when = now.saturating_add(*initial_delay);
                    Self::schedule_enactment(index, DispatchTime::At(when), call)?;
                    Proposal::Review
                }
            };

            let info = ReferendumInfo {
                track,
                proposal,
                proposer: proposer.clone(),
                submitted: now,
                tally: VoteTally::default(),
                decision_strategy: track_info.decision_strategy,
            };
            ReferendumStatusFor::<T>::insert(index, ReferendumStatus::Ongoing(info));

            T::OnPollCreated::on_poll_created(index);

            Self::deposit_event(Event::<T>::Submitted {
                index,
                track,
                proposer,
            });

            Ok(())
        }

        /// Privileged termination of a referendum that has not yet
        /// dispatched. Cancels any pending scheduler entries, releases
        /// the wrapper preimage, and records the referendum as killed.
        /// Already-terminal referenda are rejected.
        #[pallet::call_index(1)]
        #[pallet::weight(
            T::WeightInfo::kill().saturating_add(T::OnPollCompleted::weight())
        )]
        pub fn kill(origin: OriginFor<T>, index: ReferendumIndex) -> DispatchResult {
            T::KillOrigin::ensure_origin(origin)?;

            let status =
                ReferendumStatusFor::<T>::get(index).ok_or(Error::<T>::ReferendumNotFound)?;
            ensure!(
                matches!(
                    status,
                    ReferendumStatus::Ongoing(_)
                        | ReferendumStatus::Approved(_)
                        | ReferendumStatus::FastTracked(_)
                ),
                Error::<T>::ReferendumFinalized
            );

            // Best-effort cleanup. Either entry may legitimately be absent:
            // PassOrFail has no enactment task before approval, and the alarm
            // for Approved/FastTracked has already fired (it is what drove
            // the transition). If a cancel fails and the wrapper task still
            // dispatches, `enact` no-ops on the terminal status.
            let _ = T::Scheduler::cancel_named(task_name(index));
            let _ = T::Scheduler::cancel_named(alarm_name(index));
            // `Scheduler::cancel_named` via the trait API does not drop the
            // preimage it requested at schedule time; balance manually so the
            // wrapper preimage is fully released.
            if let Some(wrapper) = EnactmentTask::<T>::take(index) {
                T::Preimages::drop(&wrapper);
            }

            let now = T::BlockNumberProvider::current_block_number();
            Self::conclude(
                index,
                ReferendumStatus::Killed(now),
                Event::<T>::Killed { index },
            );
            Ok(())
        }

        /// Drive the state machine for `index`. Invoked by the alarm
        /// and available as a privileged extrinsic for manual recovery
        /// if the alarm has been dropped.
        #[pallet::call_index(2)]
        #[pallet::weight(
            // Worst-case bound: the approve-with-`Review` branch fires both hooks.
            T::WeightInfo::advance_referendum()
                .saturating_add(T::OnPollCreated::weight())
                .saturating_add(T::OnPollCompleted::weight())
        )]
        pub fn advance_referendum(origin: OriginFor<T>, index: ReferendumIndex) -> DispatchResult {
            ensure_root(origin)?;

            let status =
                ReferendumStatusFor::<T>::get(index).ok_or(Error::<T>::ReferendumNotFound)?;

            if let ReferendumStatus::Ongoing(info) = status {
                Self::advance_ongoing(index, info)?;
            }

            Ok(())
        }

        /// Dispatch `call` and mark the referendum as enacted.
        /// Invoked by the scheduler at the configured dispatch time;
        /// root may also call it directly to retry a referendum whose
        /// scheduled task was lost.
        ///
        /// No-op on terminal-no-dispatch statuses, so a stale task
        /// that fires after a cancel cannot run the call twice.
        #[pallet::call_index(3)]
        #[pallet::weight(
            T::WeightInfo::advance_referendum()
                .saturating_add(call.get_dispatch_info().call_weight)
        )]
        pub fn enact(
            origin: OriginFor<T>,
            index: ReferendumIndex,
            call: Box<CallOf<T>>,
        ) -> DispatchResult {
            ensure_root(origin)?;

            let Some(status) = ReferendumStatusFor::<T>::get(index) else {
                return Ok(());
            };
            match status {
                ReferendumStatus::Ongoing(_)
                | ReferendumStatus::Approved(_)
                | ReferendumStatus::FastTracked(_) => {}
                _ => return Ok(()),
            }

            let error = call
                .dispatch(frame_system::RawOrigin::Root.into())
                .err()
                .map(|post| post.error);

            // Tracking entry only; the scheduler drops the wrapper preimage
            // ref itself once the dispatch returns to it.
            EnactmentTask::<T>::remove(index);

            let now = T::BlockNumberProvider::current_block_number();
            Self::conclude(
                index,
                ReferendumStatus::Enacted(now),
                Event::<T>::Enacted {
                    index,
                    when: now,
                    error,
                },
            );

            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    /// Runtime-state invariants. Live against populated state, so this
    /// runs from `try_state` rather than `integrity_test`.
    ///
    /// * Initialized voter sets are non-empty: an empty voter set silently
    ///   breaks delegation. `schedule_for_review` would create a review
    ///   child no one can vote on, and the Adjustable state machine would
    ///   lapse it to `Enacted` after `initial_delay`.
    /// * Initialized `proposer_set: Some(_)` sets are non-empty:
    ///   `Some(empty)` silently closes the track to all submissions; if
    ///   that is intended, the track must declare `proposer_set: None` to
    ///   make it explicit.
    ///
    /// Genesis can legitimately observe empty sets before the
    /// stake-ranking warmup populates collectives; that is a separate
    /// concern and not enforced here.
    #[cfg(any(feature = "try-runtime", test))]
    pub fn do_try_state() -> Result<(), frame_support::sp_runtime::TryRuntimeError> {
        for track in T::Tracks::tracks() {
            ensure!(
                !track.info.voter_set.is_initialized() || !track.info.voter_set.is_empty(),
                "pallet-referenda: track has empty voter set"
            );
            if let Some(set) = &track.info.proposer_set {
                ensure!(
                    !set.is_initialized() || !set.is_empty(),
                    "pallet-referenda: track has Some(empty) proposer_set; use None"
                );
            }
        }
        Ok(())
    }

    /// PassOrFail no-decision branch: expire if the deadline has elapsed,
    /// otherwise re-arm the deadline alarm.
    fn expire_or_rearm_deadline(
        index: ReferendumIndex,
        submitted: BlockNumberFor<T>,
        decision_period: BlockNumberFor<T>,
    ) {
        let deadline = submitted.saturating_add(decision_period);
        let now = T::BlockNumberProvider::current_block_number();
        if now >= deadline {
            Self::do_expire(index);
        } else if let Err(err) = Self::set_alarm(index, deadline) {
            Self::report_scheduler_error(index, "set_alarm", err);
        }
    }

    /// Used in scheduled-call contexts where `Err` cannot be propagated
    /// to a caller; surfaces the failure off-chain instead.
    fn report_scheduler_error(index: ReferendumIndex, operation: &str, err: DispatchError) {
        log::error!(
            target: "runtime::referenda",
            "Scheduler {} failed for referendum {}: {:?}",
            operation,
            index,
            err,
        );
        Self::deposit_event(Event::<T>::SchedulerOperationFailed { index });
    }

    /// Run threshold checks on an `Ongoing` referendum and dispatch to
    /// the appropriate action helper based on the proposal kind.
    fn advance_ongoing(index: ReferendumIndex, info: ReferendumInfoOf<T>) -> DispatchResult {
        let tally = info.tally;

        match &info.proposal {
            Proposal::Action(_) => {
                let DecisionStrategy::PassOrFail {
                    decision_period,
                    approve_threshold,
                    reject_threshold,
                    on_approval,
                } = &info.decision_strategy
                else {
                    return Err(Error::<T>::Unreachable.into());
                };

                if tally.approval >= *approve_threshold {
                    Self::do_approve(index, &info, on_approval, *decision_period);
                } else if tally.rejection >= *reject_threshold {
                    Self::do_reject(index);
                } else {
                    Self::expire_or_rearm_deadline(index, info.submitted, *decision_period);
                }
            }
            Proposal::Review => {
                let DecisionStrategy::Adjustable {
                    initial_delay,
                    max_delay,
                    fast_track_threshold,
                    cancel_threshold,
                } = &info.decision_strategy
                else {
                    return Err(Error::<T>::Unreachable.into());
                };

                if tally.approval >= *fast_track_threshold {
                    Self::do_fast_track(index);
                } else if tally.rejection >= *cancel_threshold {
                    Self::do_cancel(index);
                } else {
                    Self::do_adjust_delay(
                        index,
                        &tally,
                        info.submitted,
                        *initial_delay,
                        *max_delay,
                        *fast_track_threshold,
                        *cancel_threshold,
                    );
                }
            }
        }

        Ok(())
    }

    fn conclude(index: ReferendumIndex, status: ReferendumStatusOf<T>, event: Event<T>) {
        let releases_preimage = matches!(
            status,
            ReferendumStatus::Rejected(_)
                | ReferendumStatus::Expired(_)
                | ReferendumStatus::Killed(_)
        );

        let prior = ReferendumStatusFor::<T>::get(index);
        ReferendumStatusFor::<T>::insert(index, status);

        if let Some(ReferendumStatus::Ongoing(info)) = prior {
            ActiveCount::<T>::mutate(|c| *c = c.saturating_sub(1));
            ActivePerProposer::<T>::mutate(&info.proposer, |c| *c = c.saturating_sub(1));
            T::OnPollCompleted::on_poll_completed(index);

            if releases_preimage && let Proposal::Action(bounded) = info.proposal {
                T::Preimages::drop(&bounded);
            }
        }

        Self::deposit_event(event);
    }

    /// Both `Execute` and `Review` fail closed on scheduler error: the
    /// parent stays `Ongoing` with the deadline alarm re-armed so the
    /// approved call cannot dispatch without going through the configured
    /// path.
    fn do_approve(
        index: ReferendumIndex,
        info: &ReferendumInfoOf<T>,
        on_approval: &ApprovalAction<TrackIdOf<T>>,
        decision_period: BlockNumberFor<T>,
    ) {
        let Proposal::Action(bounded_call) = &info.proposal else {
            // Reachable only on a configuration mismatch (track strategy
            // changed under live referenda). Bail without action.
            return;
        };

        let Ok((inner, _)) = T::Preimages::peek(bounded_call) else {
            Self::expire_or_rearm_deadline(index, info.submitted, decision_period);
            return;
        };

        if let ApprovalAction::Review { track } = on_approval {
            let Some(review) =
                Self::schedule_for_review(Box::new(inner), info.proposer.clone(), *track)
            else {
                Self::deposit_event(Event::<T>::ReviewSchedulingFailed {
                    index,
                    track: *track,
                });
                Self::expire_or_rearm_deadline(index, info.submitted, decision_period);
                return;
            };
            T::Preimages::drop(bounded_call);

            let now = T::BlockNumberProvider::current_block_number();
            Self::conclude(
                index,
                ReferendumStatus::Delegated(now),
                Event::<T>::Delegated {
                    index,
                    review,
                    track: *track,
                },
            );
            return;
        }

        if let Err(err) =
            Self::schedule_enactment(index, DispatchTime::After(Zero::zero()), Box::new(inner))
        {
            Self::report_scheduler_error(index, "schedule_enactment", err);
            Self::expire_or_rearm_deadline(index, info.submitted, decision_period);
            return;
        }
        T::Preimages::drop(bounded_call);

        let now = T::BlockNumberProvider::current_block_number();
        Self::conclude(
            index,
            ReferendumStatus::Approved(now),
            Event::<T>::Approved { index },
        );
    }

    /// The child claims a slot against `ActiveCount`; the caller's
    /// `conclude` on the parent releases its slot, so the net change is
    /// zero. No `Submitted` event is emitted: the child is created by
    /// approval, not by user submission.
    fn schedule_for_review(
        call: Box<CallOf<T>>,
        proposer: T::AccountId,
        track: TrackIdOf<T>,
    ) -> Option<ReferendumIndex> {
        let track_info = T::Tracks::info(track)?;
        let DecisionStrategy::Adjustable { initial_delay, .. } = &track_info.decision_strategy
        else {
            return None;
        };
        if track_info.voter_set.is_empty() {
            return None;
        }

        let now = T::BlockNumberProvider::current_block_number();
        let when = now.saturating_add(*initial_delay);
        let new_index = ReferendumCount::<T>::get();

        if let Err(err) = Self::schedule_enactment(new_index, DispatchTime::At(when), call) {
            Self::report_scheduler_error(new_index, "schedule_enactment", err);
            return None;
        }

        ReferendumCount::<T>::put(new_index.saturating_add(1));
        ActiveCount::<T>::mutate(|c| *c = c.saturating_add(1));
        ActivePerProposer::<T>::mutate(&proposer, |c| *c = c.saturating_add(1));

        let new_info = ReferendumInfo {
            track,
            proposal: Proposal::Review,
            proposer,
            submitted: now,
            tally: VoteTally::default(),
            decision_strategy: track_info.decision_strategy,
        };
        ReferendumStatusFor::<T>::insert(new_index, ReferendumStatus::Ongoing(new_info));

        T::OnPollCreated::on_poll_created(new_index);

        Some(new_index)
    }

    fn do_reject(index: ReferendumIndex) {
        let now = T::BlockNumberProvider::current_block_number();
        Self::conclude(
            index,
            ReferendumStatus::Rejected(now),
            Event::<T>::Rejected { index },
        );
    }

    fn do_expire(index: ReferendumIndex) {
        let now = T::BlockNumberProvider::current_block_number();
        Self::conclude(
            index,
            ReferendumStatus::Expired(now),
            Event::<T>::Expired { index },
        );
    }

    fn do_fast_track(index: ReferendumIndex) {
        if let Err(err) =
            T::Scheduler::reschedule_named(task_name(index), DispatchTime::After(Zero::zero()))
        {
            Self::report_scheduler_error(index, "reschedule_task", err);
            return;
        }

        let now = T::BlockNumberProvider::current_block_number();
        Self::conclude(
            index,
            ReferendumStatus::FastTracked(now),
            Event::<T>::FastTracked { index },
        );
    }

    /// The scheduler emits its own `Canceled` event for the underlying task.
    /// If `cancel_named` fails and the wrapper still fires, `enact` no-ops
    /// on the `Cancelled` status.
    fn do_cancel(index: ReferendumIndex) {
        if let Err(err) = T::Scheduler::cancel_named(task_name(index)) {
            Self::report_scheduler_error(index, "cancel_task", err);
        }
        // See `kill` for the rationale on the manual preimage drop.
        if let Some(wrapper) = EnactmentTask::<T>::take(index) {
            T::Preimages::drop(&wrapper);
        }

        let now = T::BlockNumberProvider::current_block_number();
        Self::conclude(
            index,
            ReferendumStatus::Cancelled(now),
            Event::<T>::Cancelled { index },
        );
    }

    /// Interpolation on net votes (approval - rejection), shaped by
    /// [`Config::AdjustmentCurve`]. At net = 0 the delay equals
    /// `initial_delay`. Net approval shrinks the delay toward zero as the
    /// net approaches `fast_track_threshold`; net rejection extends it
    /// toward `max_delay` as the net approaches `-cancel_threshold`. The
    /// target is anchored at `submitted` so repeated reschedules cannot
    /// drift the call.
    fn do_adjust_delay(
        index: ReferendumIndex,
        tally: &VoteTally,
        submitted: BlockNumberFor<T>,
        initial_delay: BlockNumberFor<T>,
        max_delay: BlockNumberFor<T>,
        fast_track_threshold: Perbill,
        cancel_threshold: Perbill,
    ) {
        let computed_delay: BlockNumberFor<T> = if tally.approval >= tally.rejection {
            let net = tally.approval.saturating_sub(tally.rejection);
            let progress =
                Perbill::from_rational(net.deconstruct(), fast_track_threshold.deconstruct());
            let curved = T::AdjustmentCurve::apply(progress);
            let remaining = Perbill::one().saturating_sub(curved);
            remaining.mul_floor(initial_delay)
        } else {
            let net = tally.rejection.saturating_sub(tally.approval);
            let progress =
                Perbill::from_rational(net.deconstruct(), cancel_threshold.deconstruct());
            let curved = T::AdjustmentCurve::apply(progress);
            let max_extension = max_delay.saturating_sub(initial_delay);
            initial_delay.saturating_add(curved.mul_floor(max_extension))
        };
        let target = submitted.saturating_add(computed_delay);

        let now = T::BlockNumberProvider::current_block_number();
        if target <= now {
            Self::do_fast_track(index);
            return;
        }

        // Avoid `RescheduleNoChange` when the target is unchanged.
        if Self::next_task_dispatch_time(index) == Some(target) {
            return;
        }

        if let Err(err) = T::Scheduler::reschedule_named(task_name(index), DispatchTime::At(target))
        {
            Self::report_scheduler_error(index, "reschedule_task", err);
        }
    }

    /// Idempotent: reschedules any prior alarm with the same name, so callers
    /// do not need to track whether one is currently pending.
    fn set_alarm(index: ReferendumIndex, when: BlockNumberFor<T>) -> Result<(), DispatchError> {
        if let Ok(existing) = T::Scheduler::next_dispatch_time(alarm_name(index)) {
            if existing == when {
                return Ok(());
            }
            return T::Scheduler::reschedule_named(alarm_name(index), DispatchTime::At(when))
                .map(|_| ());
        }
        let call = T::Preimages::bound(CallOf::<T>::from(Call::advance_referendum { index }))?;
        let res = T::Scheduler::schedule_named(
            alarm_name(index),
            DispatchTime::At(when),
            None,
            0, // highest priority
            frame_system::RawOrigin::Root.into(),
            call.clone(),
        );
        T::Preimages::drop(&call);
        res.map(|_| ())
    }

    /// Wraps the inner call in `Pallet::enact { index, call }`, making
    /// the `Ongoing/Approved/FastTracked -> Enacted` transition atomic
    /// with dispatch. Parks the handle in [`EnactmentTask`] so cancel
    /// paths can release the scheduler's preimage ref.
    fn schedule_enactment(
        index: ReferendumIndex,
        desired: DispatchTime<BlockNumberFor<T>>,
        call: Box<CallOf<T>>,
    ) -> DispatchResult {
        let wrapper = T::Preimages::bound(CallOf::<T>::from(Call::enact { index, call }))?;
        let res = T::Scheduler::schedule_named(
            task_name(index),
            desired,
            None,
            0, // highest priority
            frame_system::RawOrigin::Root.into(),
            wrapper.clone(),
        );
        T::Preimages::drop(&wrapper);
        res?;
        EnactmentTask::<T>::insert(index, wrapper);
        Ok(())
    }

    fn ongoing_info(index: ReferendumIndex) -> Option<ReferendumInfoOf<T>> {
        match ReferendumStatusFor::<T>::get(index)? {
            ReferendumStatus::Ongoing(info) => Some(info),
            _ => None,
        }
    }

    /// `None` when no task with that name is currently queued.
    fn next_task_dispatch_time(index: ReferendumIndex) -> Option<BlockNumberFor<T>> {
        <T::Scheduler as ScheduleNamed<
            BlockNumberFor<T>,
            CallOf<T>,
            PalletsOriginOf<T>,
        >>::next_dispatch_time(task_name(index))
        .ok()
    }
}

impl<T: Config> Polls<T::AccountId> for Pallet<T> {
    type Index = ReferendumIndex;
    type VotingScheme = VotingSchemeOf<T>;
    type VoterSet = VoterSetOf<T>;

    fn is_ongoing(index: Self::Index) -> bool {
        Self::ongoing_info(index).is_some()
    }

    fn voting_scheme_of(index: Self::Index) -> Option<Self::VotingScheme> {
        let info = Self::ongoing_info(index)?;
        T::Tracks::info(info.track).map(|t| t.voting_scheme)
    }

    fn voter_set_of(index: Self::Index) -> Option<Self::VoterSet> {
        let info = Self::ongoing_info(index)?;
        T::Tracks::info(info.track).map(|t| t.voter_set)
    }

    fn on_tally_updated(index: Self::Index, tally: &VoteTally) {
        let Some(mut info) = Self::ongoing_info(index) else {
            return;
        };
        let now = T::BlockNumberProvider::current_block_number();

        info.tally = *tally;
        ReferendumStatusFor::<T>::insert(index, ReferendumStatus::Ongoing(info));

        // Defer evaluation by one block. The hook stores the new tally; the
        // alarm fires next block and runs `advance_referendum` from a clean
        // dispatch context, avoiding re-entrancy with caller.
        if let Err(err) = Self::set_alarm(index, now.saturating_add(One::one())) {
            Self::report_scheduler_error(index, "set_alarm", err);
        }
    }

    fn on_tally_updated_weight() -> Weight {
        T::WeightInfo::on_tally_updated()
    }
}
