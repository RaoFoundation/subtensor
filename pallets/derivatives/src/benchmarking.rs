//! Benchmarks for `pallet_derivatives`.
//!
//! Shorts are the heavier side to settle: the buyback is an exact-output swap that may take
//! several passes, so the position benchmarks close shorts after the price moved against them.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_runtime::Percent;
use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::{DerivativesPoolInterface, OrderSwapInterface};

use crate::*;

/// The owner's TAO cushion.
const CUSHION_TAO: u64 = 10_000_000_000;
/// TAO a whale trades to move the pool price against the position.
const WHALE_TAO: u64 = 300_000_000_000;
/// Smaller price move for `roll`: the position must lose, but keep enough cushion to reopen.
const ROLL_WHALE_TAO: u64 = 30_000_000_000;

fn setup<T: Config>() -> (T::AccountId, NetUid) {
    let netuid = NetUid::from(1u16);
    T::Pool::set_up_pool_for_benchmark(netuid);
    Pallet::<T>::claim_hotkey();

    let owner: T::AccountId = frame_benchmarking::account("owner", 0, 0);
    T::Pool::set_up_acc_for_benchmark(&owner, &owner);
    (owner, netuid)
}

/// Fill the `MAX_EXPIRY_SHIFT - 1` queues a new position probes first, so `schedule_expiry`
/// walks every one of them before it finds room.
fn fill_expiry_queues<T: Config>(owner: &T::AccountId, netuid: NetUid) {
    let now = frame_system::Pallet::<T>::block_number();
    let mut at = now.saturating_add(Params::<T>::get().lifetime_blocks);
    for _ in 1..settle::MAX_EXPIRY_SHIFT {
        Expiring::<T>::mutate(at, |queue| {
            while queue.try_push((owner.clone(), netuid, Side::Short)).is_ok() {}
        });
        at.saturating_inc();
    }
}

#[benchmarks]
mod benchmarks {
    use super::*;

    /// Worst case: a full expiry window, so `schedule_expiry` probes every queue.
    #[benchmark]
    fn open() {
        let (owner, netuid) = setup::<T>();
        fill_expiry_queues::<T>(&owner, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Short,
            TaoBalance::from(CUSHION_TAO),
        );

        let position = Positions::<T>::get(&owner, (netuid, Side::Short)).unwrap();
        let nominal = position
            .opened_at
            .saturating_add(Params::<T>::get().lifetime_blocks);
        assert_eq!(
            position.expires_at,
            nominal.saturating_add((settle::MAX_EXPIRY_SHIFT - 1).into())
        );
    }

    /// Worst case: a short closed after a pump big enough to leave it underwater. The buyback
    /// runs every exact-output pass and then spends the whole pot, and the remainder is
    /// forfeited to the pool.
    #[benchmark]
    fn close() {
        let (owner, netuid) = setup::<T>();
        let whale: T::AccountId = frame_benchmarking::account("whale", 0, 0);
        T::Pool::set_up_acc_for_benchmark(&whale, &whale);

        Pallet::<T>::do_open(
            owner.clone(),
            netuid,
            Side::Short,
            TaoBalance::from(CUSHION_TAO),
        )
        .unwrap();
        T::Pool::buy_alpha_internal(&whale, &whale, netuid, TaoBalance::from(WHALE_TAO)).unwrap();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            owner.clone(),
            netuid,
            Side::Short,
        );

        assert!(!Positions::<T>::contains_key(&owner, (netuid, Side::Short)));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// Worst case: the `close` path of a losing short (full buyback, fee, payout), then the
    /// `open` path with a top-up and a full expiry window.
    #[benchmark]
    fn roll() {
        let (owner, netuid) = setup::<T>();
        let whale: T::AccountId = frame_benchmarking::account("whale", 0, 0);
        T::Pool::set_up_acc_for_benchmark(&whale, &whale);

        Pallet::<T>::do_open(
            owner.clone(),
            netuid,
            Side::Short,
            TaoBalance::from(CUSHION_TAO),
        )
        .unwrap();
        T::Pool::buy_alpha_internal(&whale, &whale, netuid, TaoBalance::from(ROLL_WHALE_TAO))
            .unwrap();
        // One block later, so the slot the old position frees in its own queue is not one the
        // new expiry probes.
        let now = frame_system::Pallet::<T>::block_number().saturating_add(1u32.into());
        frame_system::Pallet::<T>::set_block_number(now);
        fill_expiry_queues::<T>(&owner, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Short,
            TaoBalance::from(CUSHION_TAO),
        );

        let after = Positions::<T>::get(&owner, (netuid, Side::Short)).unwrap();
        let nominal = now.saturating_add(Params::<T>::get().lifetime_blocks);
        assert_eq!(
            after.expires_at,
            nominal.saturating_add((settle::MAX_EXPIRY_SHIFT - 1).into())
        );
        assert!(after.cushion.tao() > TaoBalance::from(CUSHION_TAO));
    }

    #[benchmark]
    fn sudo_set_params() {
        let mut params = Params::<T>::get();
        params.shorts_enabled = false;

        #[extrinsic_call]
        _(RawOrigin::Root, params.clone());

        assert_eq!(Params::<T>::get(), params);
    }

    #[benchmark]
    fn sudo_set_subnet_override() {
        let (_, netuid) = setup::<T>();
        let override_ = SubnetOverride {
            shorts_enabled: false,
            longs_enabled: true,
            max_pool_share: Some(Percent::from_percent(5)),
        };

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, Some(override_));

        assert_eq!(SubnetOverrides::<T>::get(netuid), Some(override_));
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}
