//! Benchmarks for `pallet_derivatives`.
//!
//! Shorts are the heavier side to settle: the buyback is an exact-output swap that may take
//! several passes, so the position benchmarks settle shorts after the price moved against them.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_runtime::Percent;
use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::{DerivativesPoolInterface, OrderSwapInterface};

use crate::*;

/// The owner's TAO cushion.
const CUSHION_TAO: u64 = 10_000_000_000;
/// TAO a whale trades to move the pool price against the position: 900 TAO into a 1000 TAO
/// pool takes the price ×3.6, so a 1x short's buyback costs well over its pot.
const WHALE_TAO: u64 = 900_000_000_000;

fn setup<T: Config>() -> (T::AccountId, NetUid) {
    let netuid = NetUid::from(1u16);
    T::Pool::set_up_pool_for_benchmark(netuid);
    Pallet::<T>::claim_hotkey();

    let owner: T::AccountId = frame_benchmarking::account("owner", 0, 0);
    T::Pool::set_up_acc_for_benchmark(&owner, &owner);
    (owner, netuid)
}

/// A short for `owner`, then a whale pump big enough to leave it underwater.
fn underwater_short<T: Config>(owner: &T::AccountId, netuid: NetUid) {
    let whale: T::AccountId = frame_benchmarking::account("whale", 0, 0);
    T::Pool::set_up_acc_for_benchmark(&whale, &whale);
    Pallet::<T>::do_add(
        owner.clone(),
        netuid,
        Side::Short,
        Deposit::Tao(TaoBalance::from(CUSHION_TAO)),
        100,
    )
    .unwrap();
    T::Pool::buy_alpha_internal(&whale, &whale, netuid, TaoBalance::from(WHALE_TAO)).unwrap();
}

#[benchmarks]
mod benchmarks {
    use super::*;

    /// Worst case: a flip. The caller's short was pumped underwater, so the settlement runs
    /// every exact-output pass, spends the whole pot and forfeits the rest; then the surplus
    /// opens a long.
    #[benchmark]
    fn add() {
        let (owner, netuid) = setup::<T>();
        underwater_short::<T>(&owner, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Long,
            Deposit::Tao(TaoBalance::from(2 * CUSHION_TAO)),
            100,
        );

        let after = Positions::<T>::get(&owner, netuid).unwrap();
        assert_eq!(after.side(), Side::Long);
        assert_eq!(after.cushion.tao, TaoBalance::from(CUSHION_TAO));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// Worst case: a liquidation of a short pumped underwater. The health check quotes the
    /// buyback, the buyback runs every exact-output pass and then spends the whole pot, the
    /// remainder is forfeited to the pool, and the pool tops the liquidator up to one day of
    /// fee.
    #[benchmark]
    fn close() {
        let (owner, netuid) = setup::<T>();
        underwater_short::<T>(&owner, netuid);
        let liquidator: T::AccountId = frame_benchmarking::account("liquidator", 0, 0);
        T::Pool::set_up_acc_for_benchmark(&liquidator, &liquidator);

        #[extrinsic_call]
        _(RawOrigin::Signed(liquidator), owner.clone(), netuid);

        assert!(!Positions::<T>::contains_key(&owner, netuid));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
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
            rate_per_day: None,
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
