//! Benchmarks for lift-based lending.
//!
//! Extrinsic bodies are invoked so the frame-benchmarking CLI can measure them.
//! Committed `SubstrateWeight` values in `weights.rs` come from that CLI run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_support::{assert_ok, traits::Hooks};
use frame_system::RawOrigin;
use sp_runtime::traits::{SaturatedConversion, Zero};

#[benchmarks]
mod benchmarks {
    use super::*;

    /// Fund accounts, deep-pool the subnet, init custody, and seed a reference
    /// that is already past `valid_after` so measured opens succeed.
    fn seed_lending_ready<T: Config>(netuid: NetUid, caller: &T::AccountId, hotkey: &T::AccountId) {
        Enabled::<T>::put(true);
        // Full Cap immediately: `capacity_ramp` is 0 while `age <= min_age`, so
        // with `min_age = 0` we still need `age >= 1` (and a finished ramp).
        LiftParamsStore::<T>::put(LiftParams {
            new_subnet_min_age: 0,
            new_subnet_ramp: 1,
            ..LiftParams::default()
        });
        T::Pool::benchmark_seed_pool(netuid, 1_000_000_000_000_000, 1_000_000_000_000_000);
        // Past registration (0) + finished 1-block ramp → 100% Cap.
        frame_system::Pallet::<T>::set_block_number(10_000u32.into());
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
            // Gate also requires G_ema aged one settle window past registration.
            book.last = LiftParams::default().settle_ema_blocks.max(1);
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
            None,
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
            None,
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
            Some(hotkey),
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
        // Escrow buyback path (not wallet repay): inverse-price search,
        // stepwise buyback, optional dust sale (Auditor fid:59fefdd6).
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            hotkey,
            false,
            u64::MAX,
            0u64,
            now.saturating_add(100u32.into()),
        );
    }

    #[benchmark]
    fn repay() {
        // Escrow partial buyback (not wallet repay) so reference measurements
        // cover the expensive settlement branch (Auditor fid:59fefdd6).
        // Full-principal repay delegates to close; the extrinsic weight takes
        // `close().max(repay())` so that path is not undercharged (fid:424daaa1).
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
        let half = Positions::<T>::get(&caller, netuid)
            .expect("fixture opened a short")
            .principal
            / 2;
        let now = frame_system::Pallet::<T>::block_number();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            netuid,
            hotkey,
            half.max(1),
            false,
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
        // Measure the forfeit-inclusive idle path (Skeptic fid:f2eb3d14): crank
        // the coupon above remaining collateral so charge_interest defaults the
        // loan (re-add, optional k-burn, remove_position + queue unlink).
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
        let year: u64 = T::BlocksPerYear::get();
        let period: u64 = T::InterestPeriod::get().saturated_into::<u64>().max(1);
        let due = Positions::<T>::mutate(&caller, netuid, |maybe| {
            let position = maybe.as_mut().expect("fixture opened a short");
            // Short interest burns TAO from collateral + escrowed proceeds. Size
            // the coupon so one InterestPeriod exhausts the whole pot and
            // forfeits (otherwise collect just reschedules).
            let pot = position.collateral.saturating_add(position.proceeds);
            position.annual_interest = pot
                .saturating_mul(year)
                .saturating_div(period)
                .saturating_add(1)
                .max(1);
            position.interest_remainder = 0;
            position.due
        });
        frame_system::Pallet::<T>::set_block_number(due);
        #[block]
        {
            let _ = <Pallet<T> as Hooks<BlockNumberFor<T>>>::on_idle(
                due,
                Weight::from_parts(u64::MAX, u64::MAX),
            );
        }
        assert!(
            !Positions::<T>::contains_key(&caller, netuid),
            "collect benchmark must exercise forfeiture, not a plain reschedule"
        );
    }

    #[benchmark]
    fn update_reference() {
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        seed_lending_ready::<T>(netuid, &caller, &hotkey);
        // seed_lending_ready ages G_ema to settle_ema_blocks; advance past that
        // and clear last_updated so on_finalize runs the full path (TAO EMA +
        // G EMA fixed-point update + price reference), not the early returns.
        let g_seeded = Lifts::<T>::get(netuid).last;
        let now: BlockNumberFor<T> = g_seeded.saturating_add(1).saturated_into();
        frame_system::Pallet::<T>::set_block_number(now);
        References::<T>::mutate(netuid, |r| {
            if let Some(r) = r {
                r.last_updated = Zero::zero();
            }
        });
        #[block]
        {
            <Pallet<T> as Hooks<BlockNumberFor<T>>>::on_finalize(now);
        }
        let reference = References::<T>::get(netuid).expect("reference must remain seeded");
        assert!(
            reference.last_updated >= now,
            "on_finalize must refresh the price reference"
        );
        let g_after = Lifts::<T>::get(netuid).last;
        assert!(
            g_after > g_seeded,
            "on_finalize must age G_ema past the seeded timestamp"
        );
        assert_eq!(
            g_after,
            now.saturated_into::<u64>(),
            "G_ema last must advance to the measured block"
        );
        assert!(
            TaoEma::<T>::contains_key(netuid),
            "on_finalize must maintain TaoEma"
        );
    }

    #[benchmark]
    fn settle() {
        // Measure fixed-term expiry-as-close (spot buyback / readd / k-burn),
        // the path `collect_expiry` charges via `WeightInfo::settle`
        // (Auditor fid:59fefdd6). Dissolution settle remains a separate flow.
        let caller: T::AccountId = whitelisted_caller();
        let hotkey: T::AccountId = account("hk", 0, 0);
        let netuid = NetUid::from(1u16);
        open_short_fixture::<T>(&caller, &hotkey, netuid);
        let expires_at = Positions::<T>::get(&caller, netuid)
            .expect("fixture opened a short")
            .expires_at;
        frame_system::Pallet::<T>::set_block_number(expires_at);
        #[block]
        {
            let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            Pallet::<T>::collect_expiry(expires_at, &mut meter);
        }
        assert!(
            !Positions::<T>::contains_key(&caller, netuid),
            "settle benchmark must expire the fixture via the AMM close path"
        );
    }
}
