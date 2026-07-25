//! Runtime bridge traits for swapping Aura authorities and scheduling GRANDPA changes.
//!
//! `pallet-admin-utils` does not depend on the Aura/GRANDPA pallets directly. The runtime
//! implements these traits so [`crate::Pallet::swap_authorities`] and
//! [`crate::Pallet::schedule_grandpa_change`] can update consensus authority sets.

use frame_system::pallet_prelude::BlockNumberFor;
use sp_consensus_grandpa::AuthorityList;
use sp_runtime::{BoundedVec, DispatchResult};

/// Hook used by [`crate::Pallet::swap_authorities`] to replace the Aura authority set.
pub trait AuraInterface<AuthorityId, MaxAuthorities> {
    /// Replace the current Aura authorities with `new`.
    fn change_authorities(new: BoundedVec<AuthorityId, MaxAuthorities>);
}

impl<A, M> AuraInterface<A, M> for () {
    fn change_authorities(_: BoundedVec<A, M>) {}
}

/// Hook used by [`crate::Pallet::schedule_grandpa_change`] to queue a GRANDPA authority change.
pub trait GrandpaInterface<Runtime>
where
    Runtime: frame_system::Config,
{
    /// Schedule a GRANDPA authority set change after `in_blocks`, optionally forced.
    fn schedule_change(
        next_authorities: AuthorityList,
        in_blocks: BlockNumberFor<Runtime>,
        forced: Option<BlockNumberFor<Runtime>>,
    ) -> DispatchResult;
}

impl<R> GrandpaInterface<R> for ()
where
    R: frame_system::Config,
{
    fn schedule_change(
        _next_authorities: AuthorityList,
        _in_blocks: BlockNumberFor<R>,
        _forced: Option<BlockNumberFor<R>>,
    ) -> DispatchResult {
        Ok(())
    }
}
