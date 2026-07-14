use super::VoteTally;
use frame_support::pallet_prelude::*;
use sp_runtime::Vec;

pub trait SetLike<T> {
    fn contains(&self, item: &T) -> bool;
    fn len(&self) -> u32;
    fn is_initialized(&self) -> bool;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Materialize the set as a `Vec`. Used by signed-voting to snapshot
    /// the voter set at poll creation. Implementations must return each
    /// distinct member exactly once; ordering is unspecified.
    fn to_vec(&self) -> Vec<T>;
}

/// Poll provider seen from the voting pallet's side. Carries the
/// read-only queries plus the tally-update notification fired when a
/// vote moves the tally.
pub trait Polls<AccountId> {
    type Index: Parameter + Copy + MaxEncodedLen;
    type VotingScheme: PartialEq;
    type VoterSet: SetLike<AccountId>;

    fn is_ongoing(index: Self::Index) -> bool;
    fn voting_scheme_of(index: Self::Index) -> Option<Self::VotingScheme>;
    fn voter_set_of(index: Self::Index) -> Option<Self::VoterSet>;

    fn on_tally_updated(index: Self::Index, tally: &VoteTally);
    /// Worst-case upper bound on `on_tally_updated`'s weight.
    fn on_tally_updated_weight() -> Weight;
}

/// Notification fired when a poll is created.
///
/// # Producer contract
///
/// Implementations are entitled to assume:
///
/// 1. `on_poll_created(p)` is called at most once per `(p, lifecycle)`,
///    where `lifecycle` is the span between this hook and the matching
///    `OnPollCompleted::on_poll_completed(p)`. A second call for the
///    same index without an intervening completion is a contract
///    violation: implementations should treat it as a no-op (so a buggy
///    producer cannot silently clobber tallies) but are not required to
///    detect every form of misuse.
/// 2. `Polls::is_ongoing(p)` and `Polls::voting_scheme_of(p)` return
///    consistent values for the duration of the lifecycle.
/// 3. `Polls::voter_set_of(p)` may be queried during this hook.
pub trait OnPollCreated<PollIndex> {
    fn on_poll_created(poll_index: PollIndex);
    /// Returns the worst-case upper bound on `on_poll_created`'s weight.
    fn weight() -> Weight;
}

/// Notification fired when a poll reaches a terminal status.
///
/// # Producer contract
///
/// Implementations are entitled to assume:
///
/// 1. `on_poll_completed(p)` is called at most once per `(p, lifecycle)`.
/// 2. The producer may have already updated `p`'s status to a terminal
///    value before firing this hook, so `Polls::voting_scheme_of(p)` is
///    not required to return `Some` here. Implementations that need to
///    distinguish polls owned by a specific scheme should rely on
///    locally-stored state rather than re-querying the producer.
/// 3. `on_poll_completed` must not synchronously call back into the
///    producer in a way that would re-enter `OnPollCreated`.
pub trait OnPollCompleted<PollIndex> {
    fn on_poll_completed(poll_index: PollIndex);
    /// Returns the worst-case upper bound on `on_poll_completed`'s weight.
    fn weight() -> Weight;
}

#[impl_trait_for_tuples::impl_for_tuples(10)]
impl<I: Copy> OnPollCreated<I> for Tuple {
    fn on_poll_created(poll_index: I) {
        for_tuples!( #( Tuple::on_poll_created(poll_index); )* );
    }

    fn weight() -> Weight {
        #[allow(clippy::let_and_return)]
        let mut weight = Weight::zero();
        for_tuples!( #( weight.saturating_accrue(Tuple::weight()); )* );
        weight
    }
}

#[impl_trait_for_tuples::impl_for_tuples(10)]
impl<I: Copy> OnPollCompleted<I> for Tuple {
    fn on_poll_completed(poll_index: I) {
        for_tuples!( #( Tuple::on_poll_completed(poll_index); )* );
    }

    fn weight() -> Weight {
        #[allow(clippy::let_and_return)]
        let mut weight = Weight::zero();
        for_tuples!( #( weight.saturating_accrue(Tuple::weight()); )* );
        weight
    }
}

/// Handler for when the members of a collective have changed.
pub trait OnMembersChanged<CollectiveId, AccountId> {
    /// A collective's members have changed, `incoming` members have joined and
    /// `outgoing` members have left.
    fn on_members_changed(
        collective_id: CollectiveId,
        incoming: &[AccountId],
        outgoing: &[AccountId],
    );
    /// Worst-case upper bound on `on_members_changed`'s weight. The
    /// implementation is responsible for bounding its own iteration over
    /// `incoming`/`outgoing` against the relevant `MaxMembers` constant.
    fn weight() -> Weight;
}

#[impl_trait_for_tuples::impl_for_tuples(10)]
impl<CollectiveId: Clone, AccountId> OnMembersChanged<CollectiveId, AccountId> for Tuple {
    fn on_members_changed(
        collective_id: CollectiveId,
        incoming: &[AccountId],
        outgoing: &[AccountId],
    ) {
        for_tuples!( #( Tuple::on_members_changed(collective_id.clone(), incoming, outgoing); )* );
    }

    fn weight() -> Weight {
        #[allow(clippy::let_and_return)]
        let mut weight = Weight::zero();
        for_tuples!( #( weight.saturating_accrue(Tuple::weight()); )* );
        weight
    }
}
