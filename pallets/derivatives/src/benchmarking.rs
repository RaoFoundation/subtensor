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
        TaoBalance::from(CUSHION_TAO),
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
            TaoBalance::from(2 * CUSHION_TAO),
            100,
        );

        let after = Positions::<T>::get(&owner, netuid).unwrap();
        assert_eq!(after.side(), Side::Long);
        assert_eq!(after.cushion, TaoBalance::from(CUSHION_TAO));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// Worst case: closing a short pumped underwater. The buyback runs every exact-output pass
    /// and then spends the whole pot, and the remainder is forfeited to the pool.
    #[benchmark]
    fn close() {
        let (owner, netuid) = setup::<T>();
        underwater_short::<T>(&owner, netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner.clone()), netuid);

        assert!(!Positions::<T>::contains_key(&owner, netuid));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// Worst case for one collection: a starved short is forfeited, which returns both tokens
    /// and clears every index.
    #[benchmark]
    fn collect_interest() {
        let (owner, netuid) = setup::<T>();
        Pallet::<T>::do_add(
            owner.clone(),
            netuid,
            Side::Short,
            TaoBalance::from(CUSHION_TAO),
            100,
        )
        .unwrap();
        // Ten years on, the interest due is far more than the cushion.
        frame_system::Pallet::<T>::set_block_number(
            frame_system::Pallet::<T>::block_number() + ((10 * BLOCKS_PER_YEAR) as u32).into(),
        );

        #[block]
        {
            Pallet::<T>::collect_interest(
                &owner,
                netuid,
                frame_system::Pallet::<T>::block_number(),
            )
            .unwrap();
        }

        assert!(!Positions::<T>::contains_key(&owner, netuid));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// One release attempt on a subnet with a parked pair whose spot is back within the band:
    /// both price reads and the liquidity return.
    #[benchmark]
    fn release_parked() {
        let (_, netuid) = setup::<T>();
        // Park a slice the pallet really holds: lifted straight out of the pool.
        let pallet_account = Pallet::<T>::pallet_account();
        let pallet_hotkey = Pallet::<T>::pallet_hotkey().unwrap();
        let (tao, alpha) = T::Pool::lift_liquidity(
            netuid,
            Perquintill::from_percent(1),
            &pallet_account,
            &pallet_hotkey,
        )
        .unwrap();
        Parked::<T>::insert(netuid, (tao, alpha));

        #[block]
        {
            Pallet::<T>::release_parked_within(Weight::MAX);
        }

        assert!(!Parked::<T>::contains_key(netuid));
    }

    #[benchmark]
    fn sudo_set_params() {
        let params = DerivativesParams {
            pool_share: Percent::from_percent(5),
            interest_rate: Percent::from_percent(50),
        };

        #[extrinsic_call]
        _(RawOrigin::Root, params);

        assert_eq!(Params::<T>::get(), params);
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}
