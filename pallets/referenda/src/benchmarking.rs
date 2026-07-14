//! Benchmarks for `pallet_referenda`.
//!
//! Setup is parameterised through [`Config::BenchmarkHelper`]: the runtime
//! supplies track ids of each strategy variant plus a proposer that's
//! already in the directly submittable track's proposer set.
//!
//! `advance_referendum` is benchmarked on its worst-case branch
//! (approve-with-`Review`): the parent fires `OnPollCompleted`, the child
//! fires `OnPollCreated`, and two scheduler operations run. Every other
//! branch is strictly cheaper, so a single figure soundly bounds them all.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use alloc::boxed::Box;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_runtime::Perbill;

#[benchmarks]
mod benches {
    use super::*;

    /// Worst-case `submit` for directly submittable tracks: this runtime's
    /// `Adjustable` review track is not directly submittable, so the worst
    /// reachable path is `PassOrFail`, which schedules the deadline alarm.
    #[benchmark]
    fn submit() {
        let proposer = T::BenchmarkHelper::proposer();
        T::BenchmarkHelper::seed_collective_members();
        let track = T::BenchmarkHelper::track_passorfail();
        let call = Box::new(T::BenchmarkHelper::call());

        #[extrinsic_call]
        submit(RawOrigin::Signed(proposer), track, call);

        assert_eq!(ActiveCount::<T>::get(), 1);
    }

    /// Worst-case `kill` for directly submittable tracks: an `Adjustable`
    /// review would cancel both enactment and alarm tasks, but it is not
    /// directly submittable in this runtime, so the worst reachable path is
    /// `PassOrFail` before approval.
    #[benchmark]
    fn kill() {
        let proposer = T::BenchmarkHelper::proposer();
        T::BenchmarkHelper::seed_collective_members();
        let track = T::BenchmarkHelper::track_passorfail();
        let call = Box::new(T::BenchmarkHelper::call());
        let index = ReferendumCount::<T>::get();
        Pallet::<T>::submit(RawOrigin::Signed(proposer).into(), track, call)
            .expect("submit must succeed in benchmark setup");

        #[extrinsic_call]
        kill(RawOrigin::Root, index);

        assert!(matches!(
            ReferendumStatusFor::<T>::get(index),
            Some(ReferendumStatus::Killed(_))
        ));
    }

    /// Worst-case `advance_referendum`: PassOrFail with `Review` outcome.
    /// Fires both `OnPollCreated` (for the child) and `OnPollCompleted`
    /// (parent), runs two scheduler operations.
    #[benchmark]
    fn advance_referendum() {
        let proposer = T::BenchmarkHelper::proposer();
        T::BenchmarkHelper::seed_collective_members();
        let track = T::BenchmarkHelper::track_passorfail();
        let call = Box::new(T::BenchmarkHelper::call());
        let index = ReferendumCount::<T>::get();
        Pallet::<T>::submit(RawOrigin::Signed(proposer).into(), track, call)
            .expect("submit must succeed in benchmark setup");

        // Force the approve-with-Review branch by overwriting the tally.
        let mut info = match ReferendumStatusFor::<T>::get(index) {
            Some(ReferendumStatus::Ongoing(info)) => info,
            _ => panic!("expected ongoing referendum"),
        };
        info.tally = VoteTally {
            approval: Perbill::one(),
            rejection: Perbill::zero(),
            abstention: Perbill::zero(),
        };
        ReferendumStatusFor::<T>::insert(index, ReferendumStatus::Ongoing(info));

        #[extrinsic_call]
        advance_referendum(RawOrigin::Root, index);

        assert!(matches!(
            ReferendumStatusFor::<T>::get(index),
            Some(ReferendumStatus::Delegated(_))
        ));
    }

    /// `OnTallyUpdated` hook: stores the new tally and arms an alarm at
    /// `now + 1`. Benchmarked as a function call rather than an extrinsic.
    #[benchmark]
    fn on_tally_updated() {
        let proposer = T::BenchmarkHelper::proposer();
        T::BenchmarkHelper::seed_collective_members();
        let track = T::BenchmarkHelper::track_passorfail();
        let call = Box::new(T::BenchmarkHelper::call());
        let index = ReferendumCount::<T>::get();
        Pallet::<T>::submit(RawOrigin::Signed(proposer).into(), track, call)
            .expect("submit must succeed in benchmark setup");

        let tally = VoteTally {
            approval: Perbill::from_percent(50),
            rejection: Perbill::from_percent(10),
            abstention: Perbill::from_percent(40),
        };

        #[block]
        {
            <Pallet<T> as Polls<T::AccountId>>::on_tally_updated(index, &tally);
        }
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
