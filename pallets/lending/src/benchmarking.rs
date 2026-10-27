//! Benchmarks for lift-based lending.
//!
//! Extrinsic bodies are invoked so the frame-benchmarking CLI can measure them
//! on reference hardware. Until that run lands, committed `SubstrateWeight`
//! values are conservative R/W-derived placeholders (see `weights.rs`).
#![allow(clippy::unwrap_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_support::traits::Hooks;
use frame_system::RawOrigin;

#[benchmarks]
mod benchmarks {
    use super::*;

    fn seed_enabled<T: Config>() {
        Enabled::<T>::put(true);
        LiftParamsStore::<T>::put(LiftParams::default());
    }

    #[benchmark]
    fn open() {
        seed_enabled::<T>();
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            Side::Short,
            1_000_000_000u64,
            hotkey,
            1u64,
            0u64,
            0u64,
            now.saturating_add(100u32.into()),
        );
    }

    #[benchmark]
    fn open_exact() {
        seed_enabled::<T>();
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            Side::Long,
            1_000_000_000u64,
            hotkey,
            1_000_000u64,
            0u64,
            0u64,
            now.saturating_add(100u32.into()),
        );
    }

    #[benchmark]
    fn open_long_and_sell() {
        seed_enabled::<T>();
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            hotkey,
            Some(1_000_000_000u64),
            None,
            1_000_000_000u64,
            1u64,
            Some(1_000_000u64),
            0u64,
            0u64,
            now.saturating_add(100u32.into()),
        );
    }

    #[benchmark]
    fn close() {
        seed_enabled::<T>();
        let caller: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1u16);
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            true,
            u64::MAX,
            0u64,
            now.saturating_add(100u32.into()),
        );
    }

    #[benchmark]
    fn repay() {
        seed_enabled::<T>();
        let caller: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1u16);
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            1u64,
            true,
            u64::MAX,
            0u64,
            now.saturating_add(100u32.into()),
        );
    }

    #[benchmark]
    fn set_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);
    }

    #[benchmark]
    fn set_lift_params() {
        let params = LiftParams::default();
        #[extrinsic_call]
        _(RawOrigin::Root, params);
    }

    #[benchmark]
    fn set_min_price_impact() {
        // No dedicated extrinsic; keep the weight slot for template compatibility.
        #[block]
        {}
    }

    #[benchmark]
    fn warm_reference() {
        seed_enabled::<T>();
        let caller: T::AccountId = whitelisted_caller();
        let netuid = NetUid::from(1u16);
        #[extrinsic_call]
        _(RawOrigin::Signed(caller), netuid);
    }

    #[benchmark]
    fn collect() {
        seed_enabled::<T>();
        let now = frame_system::Pallet::<T>::block_number();
        #[block]
        {
            let _ = <Pallet<T> as Hooks<BlockNumberFor<T>>>::on_idle(
                now,
                Weight::from_parts(u64::MAX, u64::MAX),
            );
        }
    }

    #[benchmark]
    fn update_reference() {
        seed_enabled::<T>();
        let netuid = NetUid::from(1u16);
        ActiveNets::<T>::insert(netuid, ());
        ActiveNetCount::<T>::put(1);
        #[block]
        {
            let _ = Pallet::<T>::ensure_tao_ema(netuid);
        }
    }

    #[benchmark]
    fn settle() {
        seed_enabled::<T>();
        let netuid = NetUid::from(1u16);
        let _ = Pallet::<T>::start_dissolution(netuid);
        #[block]
        {
            let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            let _ = Pallet::<T>::settle_lending(netuid, &mut meter);
        }
    }
}
