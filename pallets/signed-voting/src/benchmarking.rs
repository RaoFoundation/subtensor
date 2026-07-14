//! Benchmarks for `pallet-signed-voting`.
//!
//! Setup is parameterised through [`Config::BenchmarkHelper`]: the runtime
//! supplies an ongoing poll index whose [`Polls::voting_scheme_of`] matches
//! [`Config::Scheme`]. Voter-set storage is populated directly, bypassing
//! [`OnPollCreated`], so each extrinsic benchmark can exercise the worst
//! case at a chosen `voters` count without rebuilding the producer's state.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use alloc::vec::Vec;
use frame_benchmarking::v2::*;
#[allow(unused_imports)]
use frame_system::RawOrigin;

const SEED: u32 = 0;

/// Runtime-supplied bootstrap for benchmarks.
#[cfg(feature = "runtime-benchmarks")]
pub trait BenchmarkHelper<T: Config> {
    /// Return a poll index for which `T::Polls::is_ongoing` is true and
    /// `T::Polls::voting_scheme_of` matches `T::Scheme::get()`. The
    /// runtime should bootstrap this via its real [`Polls`] producer.
    fn ongoing_poll() -> PollIndexOf<T>;
}

/// Pre-populate `VoterSetOf` and `TallyOf` for `index` with `voters`
/// distinct synthetic accounts, sorted to match the storage invariant
/// (`on_poll_created` sorts before insert). Returns the accounts in
/// sorted order.
fn populate_snapshot<T: Config>(index: PollIndexOf<T>, voters: u32) -> Vec<T::AccountId> {
    let mut accounts: Vec<T::AccountId> = (0..voters)
        .map(|i| account::<T::AccountId>("voter", i, SEED))
        .collect();
    accounts.sort();
    let snapshot: BoundedVec<T::AccountId, T::MaxVoterSetSize> =
        BoundedVec::try_from(accounts.clone())
            .expect("benchmark voter count must respect MaxVoterSetSize");
    VoterSetOf::<T>::insert(index, snapshot);
    TallyOf::<T>::insert(
        index,
        SignedVoteTally {
            ayes: 0,
            nays: 0,
            total: voters,
        },
    );
    accounts
}

#[benchmarks]
mod benches {
    use super::*;

    /// `vote` worst case: no prior vote (so the `None` branch of
    /// `try_vote` runs). Snapshot is sorted, so `binary_search` is
    /// `O(log v)` regardless of which voter is chosen; we pick the last
    /// for determinism. `v` parameterises snapshot size.
    #[benchmark]
    fn vote(v: Linear<1, { T::MaxVoterSetSize::get() }>) {
        let index = T::BenchmarkHelper::ongoing_poll();
        let accounts = populate_snapshot::<T>(index, v);
        let who = accounts.last().expect("voters >= 1").clone();

        #[extrinsic_call]
        vote(RawOrigin::Signed(who.clone()), index, true);

        let tally = TallyOf::<T>::get(index).unwrap();
        assert_eq!(tally.ayes, 1);
        assert_eq!(VotingFor::<T>::get(index, who), Some(true));
    }

    /// `remove_vote` worst case: existing aye vote so the tally
    /// decrement runs.
    #[benchmark]
    fn remove_vote(v: Linear<1, { T::MaxVoterSetSize::get() }>) {
        let index = T::BenchmarkHelper::ongoing_poll();
        let accounts = populate_snapshot::<T>(index, v);
        let who = accounts.last().expect("voters >= 1").clone();
        Pallet::<T>::vote(RawOrigin::Signed(who.clone()).into(), index, true)
            .expect("vote setup must succeed");

        #[extrinsic_call]
        remove_vote(RawOrigin::Signed(who.clone()), index);

        assert_eq!(VotingFor::<T>::get(index, who), None);
    }

    /// `OnPollCreated` hook: invokes `T::Polls::voter_set_of`,
    /// materialises and sorts the result, and writes the snapshot.
    /// The runtime helper provisions a poll on its widest track (the
    /// Adjustable one) so this measures the worst-case voter-set size
    /// available on-chain. No parameter: the size is fixed by the
    /// runtime's track configuration, not by the benchmark.
    #[benchmark]
    fn on_poll_created() {
        let index = T::BenchmarkHelper::ongoing_poll();
        // Strip the snapshot the producer may have already inserted so
        // the hook re-runs the materialisation path under the bench's
        // weight measurement.
        VoterSetOf::<T>::remove(index);
        TallyOf::<T>::remove(index);

        #[block]
        {
            <Pallet<T> as OnPollCreated<PollIndexOf<T>>>::on_poll_created(index);
        }

        assert!(VoterSetOf::<T>::get(index).is_some());
    }

    /// `OnPollCompleted` hook: removes the snapshot and tally, queues
    /// the poll for lazy `VotingFor` cleanup. Fixed cost, independent of
    /// the number of voters.
    #[benchmark]
    fn on_poll_completed() {
        let index = T::BenchmarkHelper::ongoing_poll();
        let _ = populate_snapshot::<T>(index, T::MaxVoterSetSize::get());

        #[block]
        {
            <Pallet<T> as OnPollCompleted<PollIndexOf<T>>>::on_poll_completed(index);
        }

        assert!(TallyOf::<T>::get(index).is_none());
    }

    /// One drain step of `on_idle`: clears `c` `VotingFor` entries via
    /// `clear_prefix`, updates the queue head's cursor or pops it.
    /// Parameterised over `c` up to `CleanupChunkSize` (the maximum
    /// chunk size the runtime actually uses); values above that are
    /// unreachable in production.
    #[benchmark]
    fn idle_cleanup_chunk(c: Linear<1, { T::CleanupChunkSize::get() }>) {
        let index = T::BenchmarkHelper::ongoing_poll();
        let accounts = populate_snapshot::<T>(index, c);
        for who in &accounts {
            Pallet::<T>::vote(RawOrigin::Signed(who.clone()).into(), index, true)
                .expect("vote setup must succeed");
        }
        <Pallet<T> as OnPollCompleted<PollIndexOf<T>>>::on_poll_completed(index);

        let weight = <T as Config>::WeightInfo::idle_cleanup_chunk(c);
        // Idle weight large enough for exactly one drain iteration.
        let budget = weight.saturating_mul(2);

        #[block]
        {
            let _ = Pallet::<T>::drain_pending_cleanup(budget);
        }
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
