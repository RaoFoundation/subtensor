#![cfg_attr(not(feature = "std"), no_std)]

//! # Signed Voting
//!
//! Per-account voting backend for a poll producer (typically
//! `pallet-referenda`). Voters cast a single aye or nay; the tally is
//! pushed back to the producer through the [`Polls`] trait so it can
//! re-evaluate thresholds in real time.
//!
//! The pallet is generic over the producer: it does not know what is
//! being voted on, only that polls have an index, a voting scheme, and
//! a voter set. The producer provides those via [`Polls`]; the pallet
//! provides [`OnPollCreated`] / [`OnPollCompleted`] in return for
//! lifecycle notifications.
//!
//! ## Lifecycle
//!
//! - [`OnPollCreated::on_poll_created`] snapshots the producer's voter
//!   set into [`VoterSetOf`] and initialises [`TallyOf`]. Eligibility
//!   and the tally denominator are frozen for the poll's lifetime.
//! - [`Pallet::vote`] / [`Pallet::remove_vote`] check eligibility
//!   against the snapshot (binary-searched; the snapshot is sorted at
//!   creation), update [`VotingFor`] and [`TallyOf`], and notify the
//!   producer of the new tally.
//! - [`OnPollCompleted::on_poll_completed`] removes [`TallyOf`] and
//!   [`VoterSetOf`] synchronously and enqueues the poll on
//!   [`PendingCleanup`] for lazy [`VotingFor`] cleanup.
//! - [`Hooks::on_idle`] drains the cleanup queue in
//!   [`Config::CleanupChunkSize`]-sized chunks. A single poll's cleanup
//!   may span multiple idle blocks; progress is tracked by the resume
//!   cursor returned by `clear_prefix`.
//!
//! ## Frozen voter-set snapshot
//!
//! The eligibility roster is whatever [`Polls::voter_set_of`] returns
//! at `on_poll_created`. After that the underlying collective can
//! rotate freely without affecting active polls: removed members keep
//! the voting rights they had when the poll opened, new members cannot
//! sneak votes onto polls created before they joined, and the
//! denominator stays fixed so thresholds cannot drift mid-poll.
//!
//! ## Lazy `VotingFor` cleanup
//!
//! The vote map grows linearly with `voters × active polls`. Clearing
//! it inside `on_poll_completed` would put unbounded work on the
//! producer's call. Instead, completion records the poll on
//! [`PendingCleanup`] and `on_idle` reclaims the storage in chunks
//! over subsequent blocks. The bound on chunk size and queue capacity
//! is set by the runtime via [`Config::CleanupChunkSize`] and
//! [`Config::MaxPendingCleanup`].

extern crate alloc;

use frame_support::{
    pallet_prelude::*,
    sp_runtime::{Perbill, Saturating},
    weights::WeightMeter,
};
use frame_system::pallet_prelude::*;
use subtensor_runtime_common::{OnPollCompleted, OnPollCreated, Polls, SetLike, VoteTally};

pub use pallet::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
type PollIndexOf<T> = <<T as Config>::Polls as Polls<AccountIdOf<T>>>::Index;
type VotingSchemeOf<T> = <<T as Config>::Polls as Polls<AccountIdOf<T>>>::VotingScheme;

/// Raw counts of votes cast on a poll. Converted to the producer's
/// `VoteTally` (Perbill ratios) on every tally update; storing counts
/// on-chain keeps the math exact and makes the `Voted` event payload
/// directly auditable.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, PartialEq, Eq, Clone, TypeInfo, Debug,
)]
#[subtensor_macros::freeze_struct("8f9ee43d39e00767")]
pub struct SignedVoteTally {
    /// Number of approve votes cast.
    pub ayes: u32,
    /// Number of reject votes cast.
    pub nays: u32,
    /// Number of eligible voters at poll creation.
    pub total: u32,
}

impl From<SignedVoteTally> for VoteTally {
    fn from(value: SignedVoteTally) -> Self {
        if value.total == 0 {
            // Substrate's `Perbill::from_rational(_, 0)` saturates to
            // 100%, so without this short-circuit `approval`,
            // `rejection`, and `abstention` would each be 100% and sum
            // to 300%. Return the all-abstention default instead.
            return VoteTally::default();
        }
        let approval = Perbill::from_rational(value.ayes, value.total);
        let rejection = Perbill::from_rational(value.nays, value.total);
        let abstention = Perbill::one()
            .saturating_sub(approval)
            .saturating_sub(rejection);
        VoteTally {
            approval,
            rejection,
            abstention,
        }
    }
}

/// Resume cursor returned by `clear_prefix` and persisted across idle
/// blocks so a poll's cleanup can span multiple drain passes without
/// re-iterating already-removed entries.
pub type CleanupCursorOf<T> = BoundedVec<u8, <T as Config>::CleanupCursorMaxLen>;

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
        /// Voting scheme this backend handles. Polls reporting any
        /// other scheme via the `Polls` provider are ignored.
        type Scheme: Get<VotingSchemeOf<Self>>;

        /// Poll producer that owns poll lifecycles, voter sets, and
        /// scheme assignment. This pallet only stores tallies and
        /// per-voter records for polls the producer announces.
        type Polls: Polls<Self::AccountId>;

        /// Upper bound on the size of any track's voter set, used as the
        /// storage bound for [`VoterSetOf`]. Must be ≥ the largest set
        /// the runtime can produce via [`Polls::voter_set_of`]; runtimes
        /// should derive it from their collective `max_members`.
        #[pallet::constant]
        type MaxVoterSetSize: Get<u32>;

        /// Maximum number of polls that can sit in [`PendingCleanup`] at
        /// once. Should be ≥ the [`Polls`] provider's cap on
        /// simultaneously active polls; a smaller bound risks rejecting
        /// cleanup work and leaking storage.
        #[pallet::constant]
        type MaxPendingCleanup: Get<u32>;

        /// Number of `VotingFor` entries cleared per [`Hooks::on_idle`]
        /// drain step. Tunes the trade-off between idle-block weight cost
        /// and the latency of fully draining a completed poll.
        #[pallet::constant]
        type CleanupChunkSize: Get<u32>;

        /// Storage bound on the resume cursor. The cursor is a partial
        /// trie key whose length depends on the storage layout; expose
        /// the bound as a constant so it shows up in metadata. 128 is
        /// comfortable for any `(poll, account)` shape.
        #[pallet::constant]
        type CleanupCursorMaxLen: Get<u32>;

        type WeightInfo: WeightInfo;

        /// Benchmark setup hook. The runtime supplies an ongoing poll
        /// index whose voting scheme matches `Self::Scheme::get()`.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: crate::benchmarking::BenchmarkHelper<Self>;
    }

    /// Per-`(poll, voter)` vote direction. `true` is an aye, `false` a
    /// nay; absence means the voter has not cast a vote on this poll.
    /// Drained lazily by `on_idle` after `on_poll_completed` enqueues
    /// the poll for cleanup.
    #[pallet::storage]
    pub type VotingFor<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        PollIndexOf<T>,
        Twox64Concat,
        T::AccountId,
        bool,
        OptionQuery,
    >;

    /// Per-poll tally. Doubles as the index of polls this backend
    /// owns: every poll whose scheme matches `T::Scheme` has an entry
    /// between `on_poll_created` and `on_poll_completed`, and nowhere
    /// else. Polls of other schemes never get one. The cap on
    /// simultaneously-live polls comes from the [`Polls`] provider,
    /// which is the only producer of `on_poll_created` events.
    #[pallet::storage]
    pub type TallyOf<T: Config> =
        StorageMap<_, Twox64Concat, PollIndexOf<T>, SignedVoteTally, OptionQuery>;

    /// Voter-set snapshot taken at `on_poll_created` and used as the
    /// authoritative eligibility roster for the poll's lifetime. Frozen
    /// at creation: members rotated in or out of the underlying collective
    /// during the poll do not change who can vote here. Cleared by
    /// `on_poll_completed` alongside `TallyOf`.
    #[pallet::storage]
    pub type VoterSetOf<T: Config> = StorageMap<
        _,
        Twox64Concat,
        PollIndexOf<T>,
        BoundedVec<T::AccountId, T::MaxVoterSetSize>,
        OptionQuery,
    >;

    /// FIFO queue of polls awaiting `VotingFor` cleanup. `on_poll_completed`
    /// pushes to the back; `on_idle` drains from the front in chunks of
    /// `T::CleanupChunkSize`. The optional cursor lets a poll's cleanup
    /// span multiple idle blocks without re-iterating already-removed
    /// entries.
    #[pallet::storage]
    pub type PendingCleanup<T: Config> = StorageValue<
        _,
        BoundedVec<(PollIndexOf<T>, Option<CleanupCursorOf<T>>), T::MaxPendingCleanup>,
        ValueQuery,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A member cast or changed a vote on a poll.
        Voted {
            /// Account that voted.
            who: T::AccountId,
            /// Poll voted on.
            poll_index: PollIndexOf<T>,
            /// True for approve, false for reject.
            approve: bool,
            /// Tally after the vote was applied.
            tally: SignedVoteTally,
        },

        /// A member withdrew a previously cast vote.
        VoteRemoved {
            /// Account that withdrew the vote.
            who: T::AccountId,
            /// Poll the vote was withdrawn from.
            poll_index: PollIndexOf<T>,
            /// Tally after the vote was withdrawn.
            tally: SignedVoteTally,
        },

        /// A poll concluded but the cleanup queue was full. Per-voter
        /// records were left in storage and require operator
        /// intervention to reclaim.
        CleanupQueueFull {
            /// Poll whose records were not queued for cleanup.
            poll_index: PollIndexOf<T>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The poll has not started or has already concluded.
        PollNotOngoing,
        /// No poll with this identifier is registered.
        PollNotFound,
        /// This poll is governed by a different voting scheme.
        InvalidVotingScheme,
        /// The caller is not eligible to vote on this poll.
        NotInVoterSet,
        /// The caller has already cast a vote in this direction.
        DuplicateVote,
        /// The caller has no vote on this poll to withdraw.
        VoteNotFound,
        /// The poll's eligibility roster is missing. Internal inconsistency.
        VoterSetMissing,
        /// The poll's tally is missing. Internal inconsistency.
        TallyMissing,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        // `on_poll_completed` only enqueues per-voter cleanup; this
        // hook is what actually frees the storage. Draining lazily
        // here keeps the producer-facing completion path O(1)
        // regardless of voter-set size.
        fn on_idle(_n: BlockNumberFor<T>, remaining: Weight) -> Weight {
            Pallet::<T>::drain_pending_cleanup(remaining)
        }

        fn integrity_test() {
            // Zero would silently halt cleanup and leak `VotingFor`
            // entries forever; reject at boot.
            assert!(
                T::CleanupChunkSize::get() > 0,
                "pallet-signed-voting: CleanupChunkSize must be non-zero",
            );
            // A zero pending-cleanup cap would route every completion
            // through the overflow branch and leak unconditionally.
            assert!(
                T::MaxPendingCleanup::get() > 0,
                "pallet-signed-voting: MaxPendingCleanup must be non-zero",
            );
            // The voter-set snapshot must fit at least one account, or
            // every poll degrades to the empty-snapshot defense path.
            assert!(
                T::MaxVoterSetSize::get() > 0,
                "pallet-signed-voting: MaxVoterSetSize must be non-zero",
            );
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Cast or change a vote on an ongoing poll. Calling again with
        /// the opposite direction flips the vote and updates the tally;
        /// calling with the same direction is rejected as a duplicate.
        ///
        /// The caller must be in the poll's voter-set snapshot taken at
        /// creation; eligibility is not affected by membership changes
        /// after the poll started.
        #[pallet::call_index(0)]
        #[pallet::weight(
            T::WeightInfo::vote(T::MaxVoterSetSize::get())
                .saturating_add(T::Polls::on_tally_updated_weight())
        )]
        pub fn vote(
            origin: OriginFor<T>,
            poll_index: PollIndexOf<T>,
            approve: bool,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(T::Polls::is_ongoing(poll_index), Error::<T>::PollNotOngoing);
            Self::ensure_valid_voting_scheme(poll_index)?;
            Self::ensure_in_voter_set(poll_index, &who)?;

            let tally = Self::try_vote(poll_index, &who, approve)?;

            Self::deposit_event(Event::<T>::Voted {
                who,
                poll_index,
                approve,
                tally,
            });
            Ok(())
        }

        /// Withdraw a previously-cast vote on an ongoing poll. The
        /// tally is rolled back as if the caller had never voted, and
        /// the caller may cast a new vote afterwards.
        #[pallet::call_index(1)]
        #[pallet::weight(
            T::WeightInfo::remove_vote(T::MaxVoterSetSize::get())
                .saturating_add(T::Polls::on_tally_updated_weight())
        )]
        pub fn remove_vote(origin: OriginFor<T>, poll_index: PollIndexOf<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(T::Polls::is_ongoing(poll_index), Error::<T>::PollNotOngoing);
            Self::ensure_valid_voting_scheme(poll_index)?;
            Self::ensure_in_voter_set(poll_index, &who)?;

            let tally = Self::try_remove_vote(poll_index, &who)?;

            Self::deposit_event(Event::<T>::VoteRemoved {
                who,
                poll_index,
                tally,
            });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    fn try_vote(
        poll_index: PollIndexOf<T>,
        who: &T::AccountId,
        approve: bool,
    ) -> Result<SignedVoteTally, DispatchError> {
        let mut tally = TallyOf::<T>::get(poll_index).ok_or(Error::<T>::TallyMissing)?;

        VotingFor::<T>::try_mutate(poll_index, who, |vote| -> DispatchResult {
            match vote {
                Some(vote) => match (vote, approve) {
                    (true, false) => {
                        tally.ayes.saturating_dec();
                        tally.nays.saturating_inc();
                    }
                    (false, true) => {
                        tally.nays.saturating_dec();
                        tally.ayes.saturating_inc();
                    }
                    _ => return Err(Error::<T>::DuplicateVote.into()),
                },
                None => {
                    if approve {
                        tally.ayes.saturating_inc();
                    } else {
                        tally.nays.saturating_inc();
                    }
                }
            }
            *vote = Some(approve);
            Ok(())
        })?;

        TallyOf::<T>::insert(poll_index, tally.clone());
        T::Polls::on_tally_updated(poll_index, &tally.clone().into());

        Ok(tally)
    }

    // Decrement the counter matching the *stored* direction, not
    // anything the caller passes in.
    fn try_remove_vote(
        poll_index: PollIndexOf<T>,
        who: &T::AccountId,
    ) -> Result<SignedVoteTally, DispatchError> {
        let mut tally = TallyOf::<T>::get(poll_index).ok_or(Error::<T>::TallyMissing)?;

        VotingFor::<T>::try_mutate_exists(poll_index, who, |vote| -> DispatchResult {
            match vote {
                Some(vote) => {
                    if *vote {
                        tally.ayes.saturating_dec();
                    } else {
                        tally.nays.saturating_dec();
                    }
                }
                None => return Err(Error::<T>::VoteNotFound.into()),
            }
            *vote = None;
            Ok(())
        })?;

        TallyOf::<T>::insert(poll_index, tally.clone());
        T::Polls::on_tally_updated(poll_index, &tally.clone().into());

        Ok(tally)
    }

    // The producer can host multiple voting backends keyed by scheme;
    // refuse polls owned by another backend so their tallies can't be
    // mutated through this pallet.
    fn ensure_valid_voting_scheme(poll_index: PollIndexOf<T>) -> DispatchResult {
        let scheme = T::Polls::voting_scheme_of(poll_index).ok_or(Error::<T>::PollNotFound)?;
        ensure!(T::Scheme::get() == scheme, Error::<T>::InvalidVotingScheme);
        Ok(())
    }

    // O(log n) thanks to the snapshot being sorted at `on_poll_created`.
    // The sort cost is paid once; eligibility is read on every vote.
    fn ensure_in_voter_set(poll_index: PollIndexOf<T>, who: &T::AccountId) -> DispatchResult {
        let voter_set = VoterSetOf::<T>::get(poll_index).ok_or(Error::<T>::VoterSetMissing)?;
        voter_set
            .binary_search(who)
            .map_err(|_| Error::<T>::NotInVoterSet)?;
        Ok(())
    }

    // The queue read and write are billed atomically via `entry_cost`:
    // we don't read the queue if we can't also afford to write progress
    // back. Mutation between iterations happens in memory.
    fn drain_pending_cleanup(remaining: Weight) -> Weight {
        let chunk = T::CleanupChunkSize::get();
        if chunk == 0 {
            return Weight::zero();
        }
        let per_step = T::WeightInfo::idle_cleanup_chunk(chunk);
        let entry_cost = T::DbWeight::get().reads_writes(1, 1);
        let body_cost = per_step.saturating_sub(entry_cost);
        let mut meter = WeightMeter::with_limit(remaining);

        if meter.try_consume(entry_cost).is_err() {
            return meter.consumed();
        }
        let mut queue = PendingCleanup::<T>::get();
        if queue.is_empty() {
            return meter.consumed();
        }

        let mut dirty = false;
        loop {
            if meter.try_consume(body_cost).is_err() {
                break;
            }
            let Some((poll, prev_cursor)) = queue.first().cloned() else {
                break;
            };
            let result = VotingFor::<T>::clear_prefix(
                poll,
                chunk,
                prev_cursor.as_ref().map(|c| c.as_slice()),
            );
            match result.maybe_cursor {
                None => {
                    if !queue.is_empty() {
                        let _ = queue.remove(0);
                    }
                }
                Some(c) => {
                    // If the cursor exceeds `CleanupCursorMaxLen` it
                    // gets dropped here; the next pass then restarts
                    // the prefix and re-iterates already-removed
                    // entries (slower but still correct).
                    let bounded = BoundedVec::<u8, T::CleanupCursorMaxLen>::try_from(c).ok();
                    if let Some(head) = queue.iter_mut().next() {
                        *head = (poll, bounded);
                    }
                }
            }
            dirty = true;
            if queue.is_empty() {
                break;
            }
        }

        if dirty {
            PendingCleanup::<T>::put(queue);
        }
        meter.consumed()
    }
}

impl<T: Config> OnPollCreated<PollIndexOf<T>> for Pallet<T> {
    fn on_poll_created(poll_index: PollIndexOf<T>) {
        if T::Polls::voting_scheme_of(poll_index) != Some(T::Scheme::get()) {
            return;
        }

        // A second call would clobber `VoterSetOf` and reset the tally,
        // silently erasing votes already cast.
        if TallyOf::<T>::contains_key(poll_index) {
            log::warn!(
                target: "runtime::signed-voting",
                "on_poll_created called twice for poll {:?}; ignoring",
                poll_index,
            );
            return;
        }

        // Sort + dedup so `ensure_in_voter_set` can `binary_search` and
        // a producer returning a multiset cannot inflate `total`.
        let snapshot: BoundedVec<T::AccountId, T::MaxVoterSetSize> =
            T::Polls::voter_set_of(poll_index)
                .map(|s| {
                    let mut v = s.to_vec();
                    v.sort();
                    v.dedup();
                    v
                })
                .and_then(|v| BoundedVec::try_from(v).ok())
                .unwrap_or_default();

        if snapshot.is_empty() {
            log::error!(
                target: "runtime::signed-voting",
                "on_poll_created received empty or oversized voter set for poll {:?}; \
                 producer or runtime configuration is broken",
                poll_index,
            );
        }

        let total = snapshot.len() as u32;
        VoterSetOf::<T>::insert(poll_index, snapshot);
        TallyOf::<T>::insert(
            poll_index,
            SignedVoteTally {
                ayes: 0,
                nays: 0,
                total,
            },
        );
    }

    fn weight() -> Weight {
        T::WeightInfo::on_poll_created()
    }
}

impl<T: Config> OnPollCompleted<PollIndexOf<T>> for Pallet<T> {
    fn on_poll_completed(poll_index: PollIndexOf<T>) {
        // Tally absent means either another backend owns this poll or
        // the hook fired twice; either way there is nothing to clean up.
        // `voting_scheme_of` is not usable as the scheme gate here: the
        // producer transitions status to terminal before firing this hook.
        if !TallyOf::<T>::contains_key(poll_index) {
            return;
        }

        TallyOf::<T>::remove(poll_index);
        VoterSetOf::<T>::remove(poll_index);

        let pushed = PendingCleanup::<T>::mutate(|q| q.try_push((poll_index, None)).is_ok());
        if !pushed {
            // Failing the hook would tear down the producer's call.
            // The orphaned `VotingFor` entries leak storage but are
            // unread once `TallyOf` is gone.
            log::error!(
                target: "runtime::signed-voting",
                "PendingCleanup queue full; VotingFor entries for poll {:?} \
                 leaked. Raise MaxPendingCleanup or run a cleanup migration.",
                poll_index,
            );
            Self::deposit_event(Event::<T>::CleanupQueueFull { poll_index });
        }
    }

    fn weight() -> Weight {
        T::WeightInfo::on_poll_completed()
    }
}
