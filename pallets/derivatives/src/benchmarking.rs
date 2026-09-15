//! Benchmarks for `pallet_derivatives`.
//!
//! A settlement only trades when the pool's quote says the position is covered; an underwater
//! one is handed back in kind with no swap, which is the cheap path. So the position benchmarks
//! settle a short that the price moved against but that is still covered: the exact-output
//! buyback, the interest swap and recycle, the payout and the liquidity return all run.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_runtime::Percent;
use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::{DerivativesPoolInterface, OrderSwapInterface};

use crate::*;

/// The owner's TAO cushion.
const CUSHION_TAO: u64 = 10_000_000_000;
/// TAO a whale trades to move the pool price against the position: 300 TAO into a 1000 TAO
/// pool takes the price to about ×1.7, so a 1x short's buyback digs deep into its cushion but
/// is still covered, and the settlement trades instead of forfeiting.
const PUMP_TAO: u64 = 300_000_000_000;

fn setup<T: Config>() -> (T::AccountId, NetUid) {
    let netuid = NetUid::from(1u16);
    T::Pool::set_up_pool_for_benchmark(netuid);
    Pallet::<T>::claim_hotkey();
    DerivativesEnabled::<T>::put(true);
    // `add` measures a flip into a long, so both switches must be on.
    LongsEnabled::<T>::put(true);

    let owner: T::AccountId = frame_benchmarking::account("owner", 0, 0);
    T::Pool::set_up_acc_for_benchmark(&owner, &owner);
    (owner, netuid)
}

/// Move the chain `blocks` ahead so interest accrues on every open position.
fn advance<T: Config>(blocks: u64) {
    frame_system::Pallet::<T>::set_block_number(
        frame_system::Pallet::<T>::block_number() + (blocks as u32).into(),
    );
}

/// A 1x short for `owner`, a whale pump that moves the price against it, and a week of
/// interest owed: the most expensive settlement that still trades.
fn pumped_short<T: Config>(owner: &T::AccountId, netuid: NetUid) {
    let whale: T::AccountId = frame_benchmarking::account("whale", 0, 0);
    T::Pool::set_up_acc_for_benchmark(&whale, &whale);
    Pallet::<T>::do_add(
        owner.clone(),
        netuid,
        Side::Short,
        TaoBalance::from(CUSHION_TAO),
        100,
        TaoBalance::ZERO,
    )
    .unwrap();
    T::Pool::buy_alpha_internal(&whale, &whale, netuid, TaoBalance::from(PUMP_TAO)).unwrap();
    advance::<T>(INTEREST_PERIOD.into());
    assert_short_is_covered::<T>(owner, netuid);
}

/// The settlement about to run takes the trading path, by the same test `do_settle` applies:
/// the pool's buyback quote plus the interest owed fits in the pot. The quote must also exceed
/// the proceeds, so the buyback really dips into the cushion rather than closing at a profit.
fn assert_short_is_covered<T: Config>(owner: &T::AccountId, netuid: NetUid) {
    let position = Positions::<T>::get(owner, netuid).unwrap();
    let Legs::Short { proceeds, debt, .. } = position.legs else {
        panic!("benchmark setup opened the wrong side");
    };
    let quote = T::Pool::quote_buy(netuid, debt);
    let interest_due = position.interest_due(frame_system::Pallet::<T>::block_number());
    assert!(
        !interest_due.is_zero(),
        "no interest owed: the interest swap would be skipped"
    );
    assert!(quote > proceeds, "price did not move against the short");
    assert!(
        quote.saturating_add(interest_due) <= position.cushion.saturating_add(proceeds),
        "short is underwater: the settlement would skip the swap"
    );
}

#[benchmarks]
mod benchmarks {
    use super::*;

    /// Worst case: a flip. The caller's short is settled in full on the trading path (buyback,
    /// interest swap and recycle, payout, liquidity return), then the surplus lifts a fresh
    /// tranche and opens a long.
    #[benchmark]
    fn add() {
        let (owner, netuid) = setup::<T>();
        pumped_short::<T>(&owner, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Long,
            TaoBalance::from(2 * CUSHION_TAO),
            100,
            TaoBalance::ZERO,
        );

        let after = Positions::<T>::get(&owner, netuid).unwrap();
        assert_eq!(after.side(), Side::Long);
        assert_eq!(after.cushion, TaoBalance::from(CUSHION_TAO));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// Worst case: closing a covered short after the price moved against it. The exact-output
    /// buyback runs, the interest owed is swapped and recycled, the owner is paid what is left,
    /// and the pool takes its slice back through the liquidity return.
    #[benchmark]
    fn close() {
        let (owner, netuid) = setup::<T>();
        pumped_short::<T>(&owner, netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner.clone()), netuid, TaoBalance::ZERO);

        assert!(!Positions::<T>::contains_key(&owner, netuid));
        assert_eq!(Footprint::<T>::get(netuid, Side::Short), 0);
    }

    /// Worst case for one collection: a position that pays. The interest comes off the cushion,
    /// is swapped for alpha and recycled, and the position is re-queued. A forfeit trades
    /// nothing, so it is the cheaper outcome.
    #[benchmark]
    fn collect_interest() {
        let (owner, netuid) = setup::<T>();
        Pallet::<T>::do_add(
            owner.clone(),
            netuid,
            Side::Short,
            TaoBalance::from(CUSHION_TAO),
            100,
            TaoBalance::ZERO,
        )
        .unwrap();
        // A year of interest on a 1x short is about half the cushion: a large swap, still paid.
        advance::<T>(BLOCKS_PER_YEAR);
        let now = frame_system::Pallet::<T>::block_number();
        let before = Positions::<T>::get(&owner, netuid).unwrap();
        let due = before.interest_due(now);
        assert!(!due.is_zero() && due < before.cushion);

        #[block]
        {
            Pallet::<T>::collect_interest(&owner, netuid, now).unwrap();
        }

        let after = Positions::<T>::get(&owner, netuid).unwrap();
        assert_eq!(after.cushion, before.cushion.saturating_sub(due));
        assert_eq!(after.due, now + INTEREST_PERIOD.into());
        assert!(Due::<T>::contains_key(after.due, (&owner, netuid)));
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
            short_interest_rate: Percent::from_percent(60),
            long_interest_rate: Percent::from_percent(30),
        };

        #[extrinsic_call]
        _(RawOrigin::Root, params);

        assert_eq!(Params::<T>::get(), params);
    }

    #[benchmark]
    fn sudo_set_derivatives_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);

        assert!(DerivativesEnabled::<T>::get());
    }

    #[benchmark]
    fn sudo_set_longs_enabled() {
        LongsEnabled::<T>::put(false);

        #[extrinsic_call]
        _(RawOrigin::Root, true);

        assert!(LongsEnabled::<T>::get());
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}
