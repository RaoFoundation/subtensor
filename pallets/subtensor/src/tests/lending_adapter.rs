//! `pallet_lending` on the real pool adapter (`staking::lending`): shorts end to end on
//! weighted pools, the interest credit, issuance (SPEC I7), the supply cap (P23) and the
//! no-flow, no-volume rule of every leg (I18, I19).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok, traits::Hooks};
use pallet_lending::{
    InterestTotals, LendingInterface, LendingPoolInterface, Positions, SyntheticOutstanding,
    math::curve::Curve,
    types::{Leg, Position, Side},
};
use pallet_subtensor_swap::{FeeRate, PalSwapInitialized, SwapBalancer, balancer::Balancer};
use sp_core::U256;
use sp_runtime::Perquintill;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid};
use subtensor_swap_interface::SwapHandler;

use super::mock::*;
use crate::staking::lending::BalancerCurve as Bal;
use crate::*;

pub(crate) const RAO: u64 = 1_000_000_000;
/// 21,000,000 alpha (SPEC P23).
const ALPHA_SUPPLY_CAP: u64 = 21_000_000 * RAO;
const POOL_TAO: u64 = 50_000 * RAO;
const POOL_ALPHA: u64 = 500_000 * RAO;

pub(crate) fn alice() -> U256 {
    U256::from(1_001)
}

pub(crate) fn bob() -> U256 {
    U256::from(1_002)
}

/// A dynamic subnet with a 50,000 TAO / 500,000 alpha pool at quote weight `weight`% and the
/// default fee, its TAO reserve backed by the subnet account, 1,000,000 alpha outstanding, and
/// lending migrated and enabled. Alice and Bob hold 100,000 TAO each.
pub(crate) fn lending_subnet(weight: u64) -> NetUid {
    let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
    setup_reserves(netuid, POOL_TAO.into(), POOL_ALPHA.into());
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
    add_balance_to_coldkey_account(&subnet_account, POOL_TAO.into());
    TotalStake::<Test>::mutate(|total| *total = total.saturating_add(POOL_TAO.into()));
    SwapBalancer::<Test>::insert(
        netuid,
        Balancer::new(Perquintill::from_percent(weight)).unwrap(),
    );
    PalSwapInitialized::<Test>::insert(netuid, true);
    FeeRate::<Test>::insert(netuid, 33);
    SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(1_000_000 * RAO));
    let spot = <Test as Config>::SwapInterface::current_alpha_price(netuid);
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::saturating_from_num(spot));
    Lending::on_runtime_upgrade();
    assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
    add_balance_to_coldkey_account(&alice(), (100_000 * RAO).into());
    add_balance_to_coldkey_account(&bob(), (100_000 * RAO).into());
    netuid
}

pub(crate) fn short(who: U256, netuid: NetUid) -> Option<Position<u64>> {
    Positions::<Test>::get((who, netuid, Side::Short))
}

pub(crate) fn custody(who: U256, netuid: NetUid) -> U256 {
    Lending::custody_account(&who, netuid, Side::Short).expect("short is open")
}

fn balance(who: &U256) -> u64 {
    SubtensorModule::get_coldkey_balance(who).to_u64()
}

fn issuance(netuid: NetUid) -> u64 {
    SubtensorModule::get_alpha_issuance(netuid).to_u64()
}

/// Everything a lending leg must leave alone (SPEC I18, I19): the TAO flow, the protocol flow
/// and the subnet volume.
fn flow_and_volume(netuid: NetUid) -> (i64, i64, u128) {
    (
        SubnetTaoFlow::<Test>::get(netuid),
        SubnetProtocolFlow::<Test>::get(netuid),
        SubnetVolume::<Test>::get(netuid),
    )
}

/// Opens and fully closes a short on a pool of quote weight `weight`%: every leg returns the
/// quote of `BalancerCurve` (the pallet refuses with `QuoteMismatch` otherwise), the reserves
/// move as the curve's after-states say, and issuance, custody and the books come out exact.
fn short_round_trip(weight: u64) {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(weight);
        let q = 2_000 * RAO;
        let collateral = 5_000 * RAO;
        let deposit = RAO / 10;
        let before = SubtensorModule::lending_pool_state(netuid);
        let issued = issuance(netuid);
        let total_stake = TotalStake::<Test>::get();
        let untouched = flow_and_volume(netuid);
        let builder = balance(&U256::from(MOCK_BLOCK_BUILDER));

        let (after_sell, quoted) = Bal::after_sell(&before, q).unwrap();
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            collateral,
            quoted
        ));
        let pos = short(alice(), netuid).expect("opened");
        let Leg::Short { minted, proceeds } = pos.leg else {
            panic!("short leg expected");
        };
        assert_eq!((minted, proceeds), (q, quoted));
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid), q);
        // I7: the mint is in issuance exactly once.
        assert_eq!(issuance(netuid), issued + q);
        assert_eq!(Lending::lending_alpha(netuid), q);
        // I3: custody holds collateral, deposit and proceeds.
        let custody = custody(alice(), netuid);
        assert_eq!(balance(&custody), collateral + deposit + proceeds);
        assert_eq!(balance(&alice()), 100_000 * RAO - collateral - deposit);
        // The sale moved the reserves as the curve quotes; the fee alpha was then sold
        // fee-free for the block author, as for any unstake.
        let fee = q - (after_sell.alpha - before.alpha);
        let fee_tao = Bal::sell_proceeds_nofee(&after_sell, fee);
        let opened = SubtensorModule::lending_pool_state(netuid);
        assert_eq!(opened.alpha, before.alpha + q);
        assert_eq!(opened.tao, after_sell.tao - fee_tao);
        assert_eq!(balance(&U256::from(MOCK_BLOCK_BUILDER)), builder + fee_tao);
        assert_eq!(
            TotalStake::<Test>::get(),
            total_stake - (proceeds + fee_tao).into()
        );
        assert_eq!(flow_and_volume(netuid), untouched);
        assert_ok!(Lending::do_try_state());

        // Close in full: the buy-back costs exactly the curve's minimal input.
        let (after_buy, cost) = Bal::after_buy_alpha(&opened, q).unwrap();
        let issued = issuance(netuid);
        let builder = balance(&U256::from(MOCK_BLOCK_BUILDER));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            cost
        ));
        assert!(short(alice(), netuid).is_none());
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid), 0);
        assert_eq!(issuance(netuid), issued - q);
        assert_eq!(SubtensorModule::lending_pool_state(netuid), after_buy);
        let buy_fee = cost - (after_buy.tao - opened.tao);
        assert_eq!(balance(&U256::from(MOCK_BLOCK_BUILDER)), builder + buy_fee);
        // Custody is empty and the holder has everything but the round trip's cost.
        assert_eq!(balance(&custody), 0);
        assert_eq!(balance(&alice()), 100_000 * RAO + proceeds - cost);
        assert_eq!(flow_and_volume(netuid), untouched);
        assert_ok!(Lending::do_try_state());
    });
}

#[test]
fn short_opens_and_closes_through_the_adapter_at_weight_30() {
    short_round_trip(30);
}

#[test]
fn short_opens_and_closes_through_the_adapter_at_weight_70() {
    short_round_trip(70);
}

#[test]
fn short_opens_and_closes_through_the_adapter_at_weight_50() {
    short_round_trip(50);
}

/// A partial close burns exactly the alpha closed and leaves the rest in issuance (I7).
#[test]
fn partial_close_lowers_issuance_by_the_alpha_burned() {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(30);
        let q = 2_000 * RAO;
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            5_000 * RAO,
            0
        ));
        let issued = issuance(netuid);
        let burned = 750 * RAO + 1;
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            burned,
            u64::MAX
        ));
        assert_eq!(issuance(netuid), issued - burned);
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid), q - burned);
        assert_eq!(Lending::lending_alpha(netuid), q - burned);
        assert_ok!(Lending::do_try_state());
    });
}

/// SPEC I19: an interest charge raises `SubnetTAO` (and the subnet account that backs it) by
/// exactly the charge, creates no alpha and records no flow or volume.
#[test]
fn interest_credit_raises_pool_tao_by_exactly_the_charge() {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(30);
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            2_000 * RAO,
            5_000 * RAO,
            0
        ));
        let pos = short(alice(), netuid).expect("opened");
        let custody = custody(alice(), netuid);
        let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let pool_tao = SubnetTAO::<Test>::get(netuid).to_u64();
        let backing = balance(&subnet_account);
        let held = balance(&custody);
        let total_stake = TotalStake::<Test>::get();
        let issued = issuance(netuid);
        let untouched = flow_and_volume(netuid);

        // Only lending runs, so nothing else can move the pool.
        System::set_block_number(pos.charge_at);
        Lending::on_initialize(pos.charge_at);

        let totals = InterestTotals::<Test>::get(netuid);
        assert!(totals.charged > 0);
        assert_eq!(totals.to_pool, totals.charged);
        let charged = u64::try_from(totals.charged).unwrap();
        assert_eq!(SubnetTAO::<Test>::get(netuid).to_u64(), pool_tao + charged);
        assert_eq!(balance(&subnet_account), backing + charged);
        assert_eq!(balance(&custody), held - charged);
        assert_eq!(TotalStake::<Test>::get(), total_stake + charged.into());
        assert_eq!(issuance(netuid), issued);
        assert_eq!(flow_and_volume(netuid), untouched);
        let charged_pos = short(alice(), netuid).expect("still open");
        assert_eq!(charged_pos.collateral, pos.collateral - charged);
        assert_ok!(Lending::do_try_state());
    });
}

/// SPEC P23 through the real adapter: a short that takes issuance to exactly 21,000,000 alpha
/// opens, and one rao more is refused.
#[test]
fn supply_cap_is_enforced_on_the_real_issuance() {
    let q = 2_000 * RAO;
    for (over, ok) in [(0u64, true), (1, false)] {
        new_test_ext(1).execute_with(|| {
            let netuid = lending_subnet(50);
            let outstanding = SubnetAlphaOut::<Test>::get(netuid).to_u64();
            let room = ALPHA_SUPPLY_CAP - q + over - issuance(netuid);
            SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(outstanding + room));
            assert_eq!(issuance(netuid), ALPHA_SUPPLY_CAP - q + over);
            assert_eq!(
                <SubtensorModule as LendingPoolInterface<U256>>::alpha_issuance(netuid),
                issuance(netuid)
            );
            let open =
                Lending::open_short(RuntimeOrigin::signed(alice()), netuid, q, 5_000 * RAO, 0);
            if ok {
                assert_ok!(open);
                assert_eq!(issuance(netuid), ALPHA_SUPPLY_CAP);
            } else {
                assert_noop!(open, pallet_lending::Error::<Test>::AlphaSupplyCap);
            }
        });
    }
}

/// The long legs are stubs until the longs spec is settled.
#[test]
fn long_legs_are_refused() {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(50);
        assert_noop!(
            <SubtensorModule as LendingPoolInterface<U256>>::lift_and_buy(netuid, RAO, &alice()),
            Error::<Test>::LongsPendingSpec
        );
        assert_noop!(
            <SubtensorModule as LendingPoolInterface<U256>>::sell_alpha(netuid, &alice(), RAO),
            Error::<Test>::LongsPendingSpec
        );
    });
}

/// The owner cut of the legacy lock refund is taken on emitted alpha only: minted short alpha
/// is in issuance but not in the cut's base.
#[test]
fn legacy_owner_cut_excludes_lending_alpha() {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(50);
        let cut = u16::MAX / 5;
        let before = SubtensorModule::legacy_owner_cut_accrued(netuid, cut);
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            2_000 * RAO,
            5_000 * RAO,
            0
        ));
        assert_eq!(
            SubtensorModule::legacy_owner_cut_accrued(netuid, cut),
            before
        );
    });
}
