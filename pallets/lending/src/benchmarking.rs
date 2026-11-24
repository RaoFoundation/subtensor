//! Benchmarks for synthetic-mint shorts.
//!
//! Fixtures need [`LendingPoolInterface::benchmark_seed_pool`] to leave a deep,
//! trading pool with a non-zero moving price and matching alpha issuance.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_support::assert_ok;
use frame_system::RawOrigin;

const POOL: u64 = 1_000_000_000_000_000;
const MINT: u64 = 100_000_000_000;
const COLLATERAL: u64 = 1_000_000_000_000;

#[benchmarks]
mod benchmarks {
    use super::*;

    fn netuid() -> NetUid {
        NetUid::from(1u16)
    }

    fn seed<T: Config>(caller: &T::AccountId) {
        Enabled::<T>::put(true);
        SynthParamsStore::<T>::put(SynthParams::default());
        T::Pool::benchmark_seed_pool(netuid(), POOL, POOL);
        frame_system::Pallet::<T>::set_block_number(10_000u32.into());
        T::Pool::benchmark_mint_tao(caller, COLLATERAL.saturating_mul(10));
        assert_ok!(Pallet::<T>::initialize_custody());
    }

    fn open_fixture<T: Config>(caller: &T::AccountId) {
        seed::<T>(caller);
        assert_ok!(Pallet::<T>::open_short(
            RawOrigin::Signed(caller.clone()).into(),
            netuid(),
            MINT,
            COLLATERAL,
            0,
        ));
        assert!(Positions::<T>::contains_key(caller, netuid()));
    }

    fn advance<T: Config>(blocks: u32) {
        let now = frame_system::Pallet::<T>::block_number();
        frame_system::Pallet::<T>::set_block_number(now.saturating_add(blocks.into()));
    }

    #[benchmark]
    fn open_short() {
        let caller: T::AccountId = whitelisted_caller();
        seed::<T>(&caller);
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid(),
            MINT,
            COLLATERAL,
            0,
        );
        assert!(Positions::<T>::contains_key(&caller, netuid()));
    }

    #[benchmark]
    fn increase_short() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        advance::<T>(1);
        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            netuid(),
            MINT,
            COLLATERAL,
            0,
        );
        assert_eq!(
            Positions::<T>::get(&caller, netuid()).map(|p| p.minted),
            Some(MINT * 2)
        );
    }

    #[benchmark]
    fn add_collateral() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        #[extrinsic_call]
        _(RawOrigin::Signed(caller), netuid(), 1_000_000_000);
    }

    #[benchmark]
    fn withdraw_collateral() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        advance::<T>(1);
        #[extrinsic_call]
        _(RawOrigin::Signed(caller), netuid(), 1_000_000_000);
    }

    #[benchmark]
    fn close_short() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        advance::<T>(1);
        #[extrinsic_call]
        _(RawOrigin::Signed(caller.clone()), netuid(), MINT, u64::MAX);
        assert!(!Positions::<T>::contains_key(&caller, netuid()));
    }

    #[benchmark]
    fn poke() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        advance::<T>(1);
        #[extrinsic_call]
        _(RawOrigin::Signed(caller.clone()), caller.clone(), netuid());
    }

    #[benchmark]
    fn set_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);
        assert!(Enabled::<T>::get());
    }

    #[benchmark]
    fn set_synth_params() {
        let params = SynthParams::default();
        #[extrinsic_call]
        _(RawOrigin::Root, params);
    }

    #[benchmark]
    fn charge_interest() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        let due = Positions::<T>::get(&caller, netuid())
            .map(|p| p.due)
            .unwrap();
        frame_system::Pallet::<T>::set_block_number(due);
        #[block]
        {
            let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            Pallet::<T>::collect_due(due, &mut meter);
        }
        assert!(Positions::<T>::get(&caller, netuid()).is_some_and(|p| p.due > due));
    }

    #[benchmark]
    fn deleverage_step() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        Positions::<T>::mutate(&caller, netuid(), |p| {
            if let Some(p) = p {
                p.state = ShortState::Deleveraging {
                    since: frame_system::Pallet::<T>::block_number(),
                };
            }
        });
        DeleverageQueue::<T>::insert((caller.clone(), netuid()), ());
        advance::<T>(1);
        let params = SynthParamsStore::<T>::get();
        #[block]
        {
            Pallet::<T>::deleverage_step(&caller, netuid(), &params);
        }
        assert!(Positions::<T>::get(&caller, netuid()).is_none_or(|p| p.minted < MINT));
    }

    #[benchmark]
    fn retire_residual() {
        let caller: T::AccountId = whitelisted_caller();
        seed::<T>(&caller);
        let reserve = Pallet::<T>::reserve_account(netuid());
        T::Pool::benchmark_mint_tao(&reserve, COLLATERAL);
        ResidualReserve::<T>::insert(netuid(), COLLATERAL);
        ResidualAlpha::<T>::insert(netuid(), MINT);
        let params = SynthParamsStore::<T>::get();
        #[block]
        {
            Pallet::<T>::retire_residual(netuid(), MINT, &params);
        }
        assert!(ResidualAlpha::<T>::get(netuid()) < MINT);
    }

    #[benchmark]
    fn twap_sample() {
        let caller: T::AccountId = whitelisted_caller();
        seed::<T>(&caller);
        let params = SynthParamsStore::<T>::get();
        for _ in 0..params.twap_samples {
            Pallet::<T>::sample_twap(netuid(), Zero::zero(), &params);
        }
        let now = frame_system::Pallet::<T>::block_number();
        #[block]
        {
            Pallet::<T>::sample_twap(netuid(), now, &params);
        }
        assert_eq!(TwapRing::<T>::get(netuid()).last_sample, now);
    }

    #[benchmark]
    fn settle_dissolution() {
        let caller: T::AccountId = whitelisted_caller();
        open_fixture::<T>(&caller);
        assert_ok!(Pallet::<T>::start_dissolution(netuid()));
        #[block]
        {
            let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
            assert!(Pallet::<T>::settle_positions_on_dissolution(
                netuid(),
                &mut meter
            ));
        }
        assert!(!Positions::<T>::contains_key(&caller, netuid()));
    }
}
