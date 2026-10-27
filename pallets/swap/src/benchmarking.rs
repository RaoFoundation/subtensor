//! Benchmarking setup for pallet-subtensor-swap
#![allow(clippy::unwrap_used)]
#![allow(clippy::multiple_bound_locations)]
#![allow(deprecated)]

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use subtensor_runtime_common::NetUid;

use crate::pallet::{Call, Config, Pallet};

#[benchmarks(where T: Config)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn set_fee_rate() {
        let netuid = NetUid::from(1);
        let rate: u16 = 100;

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, rate);
    }

    #[benchmark]
    fn toggle_user_liquidity() {
        // Deprecated dispatchable: the worst and only path returns immediately.
        #[block]
        {
            assert!(
                Pallet::<T>::toggle_user_liquidity(
                    RawOrigin::Root.into(),
                    NetUid::from(u16::MAX),
                    true,
                )
                .is_err()
            );
        }
    }

    #[benchmark]
    fn add_liquidity() {
        // Deprecated dispatchable: arguments are ignored by the call, but use
        // maximal scalar witnesses to avoid benchmarking a trivial call shape.
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("swap_hotkey", 0, u32::MAX);

        #[block]
        {
            assert!(
                Pallet::<T>::add_liquidity(
                    RawOrigin::Signed(caller).into(),
                    hotkey,
                    NetUid::from(u16::MAX),
                    crate::TickIndex::default(),
                    crate::TickIndex::default(),
                    u64::MAX,
                )
                .is_err()
            );
        }
    }

    #[benchmark]
    fn remove_liquidity() {
        // Deprecated dispatchable: arguments are ignored by the call, but use
        // maximal scalar witnesses to avoid benchmarking a trivial call shape.
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("swap_hotkey", 0, u32::MAX);

        #[block]
        {
            assert!(
                Pallet::<T>::remove_liquidity(
                    RawOrigin::Signed(caller).into(),
                    hotkey,
                    NetUid::from(u16::MAX),
                    crate::PositionId::default(),
                )
                .is_err()
            );
        }
    }

    #[allow(deprecated)]
    #[benchmark]
    fn modify_position() {
        // Deprecated dispatchable: arguments are ignored by the call, but use
        // maximal scalar witnesses to avoid benchmarking a trivial call shape.
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("swap_hotkey", 0, u32::MAX);

        #[block]
        {
            assert!(
                Pallet::<T>::modify_position(
                    RawOrigin::Signed(caller).into(),
                    hotkey,
                    NetUid::from(u16::MAX),
                    crate::PositionId::default(),
                    i64::MAX,
                )
                .is_err()
            );
        }
    }

    #[allow(deprecated)]
    #[benchmark]
    fn disable_lp() {
        // Deprecated dispatchable: the worst and only path returns immediately.
        #[block]
        {
            assert!(Pallet::<T>::disable_lp(RawOrigin::Root.into()).is_err());
        }
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
