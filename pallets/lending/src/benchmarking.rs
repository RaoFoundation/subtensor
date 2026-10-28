//! Benchmarks for lift-based lending.
//!
//! Extrinsic bodies are invoked so the frame-benchmarking CLI can measure them
//! on reference hardware. Until that run lands, committed `SubstrateWeight`
//! values are conservative R/W-derived placeholders (see `weights.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_support::{assert_ok, traits::Hooks};
use frame_system::RawOrigin;
use sp_runtime::traits::Zero;

#[benchmarks]
mod benchmarks {
    use super::*;

    /// Fund accounts, deep-pool the subnet, init custody, and seed a reference
    /// that is already past `valid_after` so measured opens succeed.
    fn seed_lending_ready<T: Config>(netuid: NetUid, caller: &T::AccountId, hotkey: &T::AccountId) {
        Enabled::<T>::put(true);
        LiftParamsStore::<T>::put(LiftParams {
            new_subnet_min_age: 0,
            new_subnet_ramp: 1,
            ..LiftParams::default()
        });
        T::Pool::benchmark_seed_pool(netuid, 1_000_000_000_000_000, 1_000_000_000_000_000);
        T::Pool::benchmark_mint_tao(caller, 50_000_000_000_000);
        T::Pool::benchmark_mint_alpha(caller, hotkey, netuid, 50_000_000_000_000);
        assert_ok!(Pallet::<T>::initialize_custody());
        assert!(
            Pallet::<T>::try_seed_reference(netuid),
            "benchmark fixture must seed a lending reference"
        );
        // try_seed_reference sets valid_after into the future; open requires now.
        References::<T>::mutate(netuid, |r| {
            if let Some(r) = r {
                r.valid_after = Zero::zero();
                r.last_updated = Zero::zero();
            }
        });
        assert!(
            References::<T>::contains_key(netuid),
            "benchmark fixture must seed a lending reference"
        );
        ActiveNets::<T>::insert(netuid, ());
        if ActiveNetCount::<T>::get() == 0 {
            ActiveNetCount::<T>::put(1);
        }
        Lifts::<T>::mutate(netuid, |book| {
            book.g_ema = T::Pool::pool_tao(netuid).to_u64();
            book.last = LiftParams::default().settle_ema_blocks;
        });
    }

    fn open_short_fixture<T: Config>(caller: &T::AccountId, hotkey: &T::AccountId, netuid: NetUid) {
        seed_lending_ready::<T>(netuid, caller, hotkey);
        let now = frame_system::Pallet::<T>::block_number();
        assert_ok!(Pallet::<T>::open(
            RawOrigin::Signed(caller.clone()).into(),
            netuid,
            Side::Short,
            1_000_000_000u64,
            hotkey.clone(),
            1u64,
            0u64,
            0u64,
            now.saturating_add(100u32.into()),
        ));
        assert!(
            Positions::<T>::contains_key(caller, netuid),
            "benchmark fixture must open a short position"
        );
    }

    #[benchmark]
    fn open() {
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        seed_lending_ready::<T>(netuid, &caller, &hotkey);
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
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        seed_lending_ready::<T>(netuid, &caller, &hotkey);
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
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        seed_lending_ready::<T>(netuid, &caller, &hotkey);
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
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
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
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
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
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        seed_lending_ready::<T>(netuid, &caller, &hotkey);
        References::<T>::remove(netuid);
        ActiveNets::<T>::remove(netuid);
        #[extrinsic_call]
        _(RawOrigin::Signed(caller), netuid);
    }

    #[benchmark]
    fn collect() {
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
        // Interest is due at open_block + InterestPeriod — advance so collect_due
        // actually charges and reschedules (not a no-op skip).
        let due = Positions::<T>::get(&caller, netuid)
            .expect("fixture opened a short")
            .due;
        frame_system::Pallet::<T>::set_block_number(due);
        #[block]
        {
            let _ = <Pallet<T> as Hooks<BlockNumberFor<T>>>::on_idle(
                due,
                Weight::from_parts(u64::MAX, u64::MAX),
            );
        }
        // Outside measured block: interest path must have run (reschedule or forfeit).
        if let Some(position) = Positions::<T>::get(&caller, netuid) {
            assert!(position.due > due, "collect must reschedule the due bucket");
        }
    }

    #[benchmark]
    fn update_reference() {
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        seed_lending_ready::<T>(netuid, &caller, &hotkey);
        // Advance one block and clear last_updated so on_finalize performs the
        // full per-subnet path (TAO EMA + G EMA + price reference), not a
        // read-only ensure_tao_ema hit.
        let now = frame_system::Pallet::<T>::block_number().saturating_add(1u32.into());
        frame_system::Pallet::<T>::set_block_number(now);
        References::<T>::mutate(netuid, |r| {
            if let Some(r) = r {
                r.last_updated = Zero::zero();
            }
        });
        let g_before = Lifts::<T>::get(netuid).last;
        #[block]
        {
            <Pallet<T> as Hooks<BlockNumberFor<T>>>::on_finalize(now);
        }
        let reference = References::<T>::get(netuid).expect("reference must remain seeded");
        assert!(
            reference.last_updated >= now,
            "on_finalize must refresh the price reference"
        );
        assert!(
            Lifts::<T>::get(netuid).last >= g_before,
            "on_finalize must age G_ema"
        );
        assert!(
            TaoEma::<T>::contains_key(netuid),
            "on_finalize must maintain TaoEma"
        );
    }

    #[benchmark]
    fn settle() {
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
        assert_ok!(Pallet::<T>::start_dissolution(netuid));
        #[block]
        {
            let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            let _ = Pallet::<T>::settle_lending(netuid, &mut meter);
        }
    }
}
