//! Benchmarks for `pallet-multi-collective`.
//!
//! Setup is parameterised through [`Config::BenchmarkHelper`]: the runtime
//! supplies a non-rotatable collective whose bounds allow the pallet to
//! fill and drain it freely, plus a separate rotatable collective for
//! `force_rotate`.
#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;

const SEED: u32 = 0;

fn fill_members<T: Config>(collective_id: T::CollectiveId, count: u32) -> Vec<T::AccountId> {
    let mut members: Vec<T::AccountId> = (0..count)
        .map(|i| account::<T::AccountId>("member", i, SEED))
        .collect();
    members.sort();

    // Bypass `add_member` to avoid paying the per-call binary_search cost
    // during setup: we know the list is sorted and unique, so we can
    // write the storage directly.
    let bounded =
        BoundedVec::try_from(members.clone()).expect("benchmark fill must respect MaxMembers");
    Members::<T>::insert(collective_id, bounded);
    members
}

#[benchmarks]
mod benches {
    use super::*;

    /// Worst case: pre-fill to `MaxMembers - 1` so the binary_search runs at full depth.
    #[benchmark]
    fn add_member() {
        let collective = T::BenchmarkHelper::collective();
        let max = T::MaxMembers::get();
        let _existing = fill_members::<T>(collective, max.saturating_sub(1));
        let new_member = account::<T::AccountId>("new", 0, SEED);

        #[extrinsic_call]
        add_member(RawOrigin::Root, collective, new_member);

        assert_eq!(Members::<T>::get(collective).len(), max as usize);
    }

    /// Worst case: full collective; binary_search at max depth, remove
    /// shifts the maximum number of trailing elements.
    #[benchmark]
    fn remove_member() {
        let collective = T::BenchmarkHelper::collective();
        let max = T::MaxMembers::get();
        let members = fill_members::<T>(collective, max);
        // Remove the head: `remove(0)` shifts every other element.
        let to_remove = members[0].clone();

        #[extrinsic_call]
        remove_member(RawOrigin::Root, collective, to_remove);

        assert_eq!(
            Members::<T>::get(collective).len(),
            (max as usize).saturating_sub(1),
        );
    }

    /// Worst case: full collective; two binary_searches at max depth,
    /// then a remove + insert each shifting the maximum trailing slice.
    #[benchmark]
    fn swap_member() {
        let collective = T::BenchmarkHelper::collective();
        let max = T::MaxMembers::get();
        let members = fill_members::<T>(collective, max);
        let to_remove = members[0].clone();
        let to_add = account::<T::AccountId>("new", 0, SEED);

        #[extrinsic_call]
        swap_member(RawOrigin::Root, collective, to_remove, to_add);

        assert_eq!(Members::<T>::get(collective).len(), max as usize);
    }

    /// Worst case: replace a fully-populated collective with a completely disjoint set
    /// of `MaxMembers` new accounts.
    #[benchmark]
    fn set_members() {
        let collective = T::BenchmarkHelper::collective();
        let max = T::MaxMembers::get();
        let _existing = fill_members::<T>(collective, max);

        let new_members: Vec<T::AccountId> = (0..max)
            .map(|i| account::<T::AccountId>("new", i, SEED))
            .collect();

        #[extrinsic_call]
        set_members(RawOrigin::Root, collective, new_members.clone());

        assert_eq!(Members::<T>::get(collective).len(), max as usize);
    }

    /// `force_rotate` itself does only validation + a hook dispatch;
    /// this benchmark measures just the extrinsic-side overhead. The
    /// hook's worst-case cost is added separately via
    /// `T::OnNewTerm::weight()` in the `#[pallet::weight(...)]`
    /// annotation.
    #[benchmark]
    fn force_rotate() {
        let collective = T::BenchmarkHelper::rotatable_collective();

        #[extrinsic_call]
        force_rotate(RawOrigin::Root, collective);
    }

    #[benchmark]
    fn do_add_member() {
        let collective = T::BenchmarkHelper::collective();
        let max = T::MaxMembers::get();
        let _existing = fill_members::<T>(collective, max.saturating_sub(1));
        let new_member = account::<T::AccountId>("new", 0, SEED);

        #[block]
        {
            Pallet::<T>::do_add_member(collective, new_member)
                .expect("benchmark setup must allow add");
        }

        assert_eq!(Members::<T>::get(collective).len(), max as usize);
    }

    #[benchmark]
    fn do_remove_member() {
        let collective = T::BenchmarkHelper::collective();
        let max = T::MaxMembers::get();
        let members = fill_members::<T>(collective, max);
        let to_remove = members[0].clone();

        #[block]
        {
            Pallet::<T>::do_remove_member(collective, to_remove)
                .expect("benchmark setup must allow remove");
        }

        assert_eq!(
            Members::<T>::get(collective).len(),
            (max as usize).saturating_sub(1),
        );
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
