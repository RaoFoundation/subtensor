//! Benchmarks for `pallet_canonical_pool`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]

use frame_benchmarking::v2::*;
use frame_support::traits::{EnsureOrigin, Get, fungible::Mutate};
use frame_system::RawOrigin;
use sp_runtime::Permill;
use subtensor_runtime_common::{TaoBalance, TusdBalance};

use crate::{
    Accumulator, Call, Caps, Config, Enabled, Fee, Pallet, PriceAccumulator, ReserveTao,
    ReserveTusd, Shares, SwapCaps, TotalShares, math,
};

const UNIT: u64 = 1_000_000_000;
/// The seeded pool: 10,000 TAO and 4,000,000 tUSD.
const POOL_TAO: u64 = 10_000 * UNIT;
const POOL_TUSD: u64 = 4_000_000 * UNIT;

/// A funded account.
fn funded<T: Config>(name: &'static str) -> T::AccountId {
    let who: T::AccountId = account(name, 0, 0);
    <T as Config>::Currency::set_balance(&who, TaoBalance::from(100_000 * UNIT));
    pallet_tusd::Pallet::<T>::benchmark_fund(&who, TusdBalance::from(40_000_000 * UNIT)).unwrap();
    who
}

/// An enabled pool with liquidity and an old price history, so every call takes its
/// costliest path: the accumulator accrues and rotates its checkpoint.
fn seed_pool<T: Config>() -> T::AccountId {
    let provider = funded::<T>("provider");
    let pool = Pallet::<T>::account_id();
    <T as Config>::Currency::set_balance(&pool, TaoBalance::from(POOL_TAO));
    pallet_tusd::Pallet::<T>::benchmark_fund(&pool, TusdBalance::from(POOL_TUSD)).unwrap();
    ReserveTao::<T>::put(TaoBalance::from(POOL_TAO));
    ReserveTusd::<T>::put(TusdBalance::from(POOL_TUSD));
    let shares = math::initial_shares(POOL_TAO, POOL_TUSD);
    TotalShares::<T>::put(shares);
    Shares::<T>::insert(&provider, shares - math::MINIMUM_LIQUIDITY);
    Accumulator::<T>::put(PriceAccumulator::default());
    Enabled::<T>::put(true);
    frame_system::Pallet::<T>::set_block_number((T::TwapPeriod::get() * 3).into());
    provider
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn add_liquidity() {
        seed_pool::<T>();
        let caller = funded::<T>("caller");

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            TaoBalance::from(100 * UNIT),
            TusdBalance::from(40_000 * UNIT),
            1,
        );

        assert!(Shares::<T>::get(&caller) > 0);
    }

    #[benchmark]
    fn remove_liquidity() {
        let provider = seed_pool::<T>();
        let shares = Shares::<T>::get(&provider) / 2;

        #[extrinsic_call]
        _(
            RawOrigin::Signed(provider.clone()),
            shares,
            TaoBalance::from(1),
            TusdBalance::from(1),
        );

        assert!(ReserveTao::<T>::get() < TaoBalance::from(POOL_TAO));
    }

    #[benchmark]
    fn swap_tusd_for_tao() {
        seed_pool::<T>();
        let caller = funded::<T>("caller");
        let amount = Caps::<T>::get().per_swap.mul_floor(POOL_TUSD);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            TusdBalance::from(amount),
            TaoBalance::from(1),
        );

        assert!(ReserveTao::<T>::get() < TaoBalance::from(POOL_TAO));
    }

    #[benchmark]
    fn swap_tao_for_tusd() {
        seed_pool::<T>();
        let caller = funded::<T>("caller");
        let amount = Caps::<T>::get().per_swap.mul_floor(POOL_TAO);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            TaoBalance::from(amount),
            TusdBalance::from(1),
        );

        assert!(ReserveTusd::<T>::get() < TusdBalance::from(POOL_TUSD));
    }

    #[benchmark]
    fn set_enabled() -> Result<(), BenchmarkError> {
        let origin = <T as Config>::AdminOrigin::try_successful_origin()
            .map_err(|_| BenchmarkError::Weightless)?;

        #[extrinsic_call]
        _(origin as T::RuntimeOrigin, true);

        assert!(Enabled::<T>::get());
        Ok(())
    }

    #[benchmark]
    fn set_fee() -> Result<(), BenchmarkError> {
        let origin = <T as Config>::AdminOrigin::try_successful_origin()
            .map_err(|_| BenchmarkError::Weightless)?;

        #[extrinsic_call]
        _(origin as T::RuntimeOrigin, crate::MAX_FEE);

        assert_eq!(Fee::<T>::get(), crate::MAX_FEE);
        Ok(())
    }

    #[benchmark]
    fn set_swap_caps() -> Result<(), BenchmarkError> {
        let origin = <T as Config>::AdminOrigin::try_successful_origin()
            .map_err(|_| BenchmarkError::Weightless)?;
        let caps = SwapCaps {
            per_swap: Permill::from_percent(5),
            per_window: Permill::from_percent(50),
            window: 600,
        };

        #[extrinsic_call]
        _(origin as T::RuntimeOrigin, caps);

        assert_eq!(Caps::<T>::get(), caps);
        Ok(())
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
