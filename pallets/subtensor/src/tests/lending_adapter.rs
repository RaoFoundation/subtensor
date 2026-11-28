//! `pallet_lending` on the real pool adapter (`staking::lending`): shorts end to end on
//! weighted pools, the interest credit, issuance (SPEC I7), the supply cap (P23), the no-flow,
//! no-volume rule of every leg (I18, I19), the short default (SPEC section 1 item 4, I2) and
//! the emission-set gate of a short's growth.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok, traits::Hooks};
use pallet_lending::{
    InterestTotals, LendingInterface, LendingPoolInterface, Positions, ResidualAlpha,
    SyntheticOutstanding,
    math::curve::Curve,
    types::{Leg, Position, Side},
};
use pallet_subtensor_swap::{FeeRate, PalSwapInitialized, SwapBalancer, balancer::Balancer};
use sp_core::U256;
use sp_runtime::{Perquintill, traits::AccountIdConversion};
use substrate_fixed::types::{I96F32, U64F64, U96F32};
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

fn burn_account() -> U256 {
    <Test as Config>::BurnAccountId::get().into_account_truncating()
}

/// Opens Alice's short of 2,000 alpha and leaves `collateral` rao in it and in its custody,
/// as after a long bleed; the rest goes back to Alice.
fn short_with_collateral_left(netuid: NetUid, collateral: u64) -> Position<u64> {
    assert_ok!(Lending::open_short(
        RuntimeOrigin::signed(alice()),
        netuid,
        2_000 * RAO,
        5_000 * RAO,
        0
    ));
    let pos = short(alice(), netuid).expect("opened");
    assert_ok!(
        <SubtensorModule as LendingPoolInterface<U256>>::transfer_tao(
            &custody(alice(), netuid),
            &alice(),
            pos.collateral - collateral
        )
    );
    let left = Position { collateral, ..pos };
    Positions::<Test>::insert((alice(), netuid, Side::Short), left);
    assert_ok!(Lending::do_try_state());
    left
}

/// Runs the lending hook alone for every block up to the short's charge block, so nothing
/// else can move the pool. The events left are those of the charge block.
fn run_charge(pos: &Position<u64>) {
    for block in System::block_number() + 1..=pos.charge_at {
        System::reset_events();
        System::set_block_number(block);
        Lending::on_initialize(block);
    }
}

fn defaults() -> Vec<(U256, NetUid, u64, u64)> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::Lending(pallet_lending::Event::ShortDefaulted {
                who,
                netuid,
                residual_alpha,
                proceeds_to_pool,
            }) => Some((who, netuid, residual_alpha, proceeds_to_pool)),
            _ => None,
        })
        .collect()
}

/// What the first scheduled charge takes from Alice's short when `collateral` is left in it,
/// and whether it then defaults, on a fresh chain with a pool of quote weight `weight`%.
fn first_charge(weight: u64, collateral: u64) -> (u64, bool) {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(weight);
        let pos = short_with_collateral_left(netuid, collateral);
        run_charge(&pos);
        let after = short(alice(), netuid);
        let left = after.map_or(0, |pos| pos.collateral);
        (pos.collateral - left, after.is_none())
    })
}

/// The collateral that the first charge takes exactly, leaving `C == 0` with nothing due. The
/// rate depends on what the short holds (its debt over `C + P`), so the amount is searched
/// for: it is the least collateral whose short does not default at its first charge.
fn exact_charge(weight: u64) -> u64 {
    let (mut defaults_at, mut pays_at) = (0u64, 1_000 * RAO);
    assert!(first_charge(weight, defaults_at).1 && !first_charge(weight, pays_at).1);
    while pays_at - defaults_at > 1 {
        let middle = defaults_at + (pays_at - defaults_at) / 2;
        if first_charge(weight, middle).1 {
            defaults_at = middle;
        } else {
            pays_at = middle;
        }
    }
    assert_eq!(first_charge(weight, pays_at), (pays_at, false));
    assert_eq!(first_charge(weight, pays_at - 1), (pays_at - 1, true));
    pays_at
}

/// SPEC section 1 item 4 through the real adapter. A short whose collateral is one rao short
/// of the charge pays what it has and defaults in that block: its proceeds are credited to
/// `SubnetTAO` (backed by the subnet account, with no flow and no volume), its deposit goes
/// to the burn account, its alpha moves from `SyntheticOutstanding` to `ResidualAlpha` and
/// stays issued, no TAO is created or destroyed, and custody is left empty.
#[test]
fn a_default_credits_the_pool_burns_the_deposit_and_keeps_the_alpha_issued() {
    let due = exact_charge(30);
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(30);
        let q = 2_000 * RAO;
        let pos = short_with_collateral_left(netuid, due - 1);
        let Leg::Short { minted, proceeds } = pos.leg else {
            panic!("short leg expected");
        };
        assert_eq!(minted, q);
        let custody = custody(alice(), netuid);
        let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        assert_eq!(balance(&custody), due - 1 + proceeds + pos.deposit);
        let pool_tao = SubnetTAO::<Test>::get(netuid).to_u64();
        let backing = balance(&subnet_account);
        let burned = balance(&burn_account());
        let holder = balance(&alice());
        let total_stake = TotalStake::<Test>::get();
        let tao_issued = (
            TotalIssuance::<Test>::get(),
            pallet_balances::TotalIssuance::<Test>::get(),
        );
        let alpha_issued = issuance(netuid);
        let reserve = SubnetAlphaIn::<Test>::get(netuid);
        let untouched = flow_and_volume(netuid);
        let cut = u16::MAX / 5;
        let owner_cut = SubtensorModule::legacy_owner_cut_accrued(netuid, cut);

        run_charge(&pos);

        // The charge took the last collateral, and the default sent the proceeds after it.
        assert_eq!(defaults(), vec![(alice(), netuid, q, proceeds)]);
        let to_pool = due - 1 + proceeds;
        assert_eq!(SubnetTAO::<Test>::get(netuid).to_u64(), pool_tao + to_pool);
        assert_eq!(balance(&subnet_account), backing + to_pool);
        assert_eq!(TotalStake::<Test>::get(), total_stake + to_pool.into());
        assert_eq!(flow_and_volume(netuid), untouched);
        // Only the interest is interest.
        assert_eq!(
            InterestTotals::<Test>::get(netuid).charged,
            u128::from(due - 1)
        );
        // The deposit is burned as every burn of this pallet: moved to the burn account, with
        // TAO issuance as it was. The holder gets nothing back.
        assert_eq!(balance(&burn_account()), burned + pos.deposit);
        assert_eq!(balance(&custody), 0);
        assert_eq!(balance(&alice()), holder);
        assert_eq!(
            (
                TotalIssuance::<Test>::get(),
                pallet_balances::TotalIssuance::<Test>::get()
            ),
            tao_issued
        );
        // I2: the alpha is neither minted nor burned, only rebooked.
        assert!(short(alice(), netuid).is_none());
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid), 0);
        assert_eq!(ResidualAlpha::<Test>::get(netuid), q);
        assert_eq!(Lending::lending_alpha(netuid), q);
        assert_eq!(issuance(netuid), alpha_issued);
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), reserve);
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::alpha_issuance(netuid),
            alpha_issued
        );
        // The residual stays out of the legacy owner cut's base, as the minted alpha was.
        assert_eq!(
            SubtensorModule::legacy_owner_cut_accrued(netuid, cut),
            owner_cut
        );
        assert_ok!(Lending::do_try_state());
    });
}

/// A short with exactly the charge in collateral pays it and stays open at `C == 0`; the next
/// charge defaults it. Neither moves a flow or the volume.
#[test]
fn collateral_that_exactly_covers_the_charge_defaults_one_charge_later() {
    let due = exact_charge(50);
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(50);
        let pos = short_with_collateral_left(netuid, due);
        let untouched = flow_and_volume(netuid);

        run_charge(&pos);
        let paid = short(alice(), netuid).expect("the charge was covered");
        assert_eq!((paid.collateral, paid.accrual.due), (0, 0));
        assert!(defaults().is_empty());

        run_charge(&paid);
        assert!(short(alice(), netuid).is_none());
        assert_eq!(defaults().len(), 1);
        assert_eq!(ResidualAlpha::<Test>::get(netuid), 2_000 * RAO);
        assert_eq!(flow_and_volume(netuid), untouched);
        assert_ok!(Lending::do_try_state());
    });
}

/// SPEC P23 and I2: the alpha of a defaulted short is still issued, so it still counts in
/// `get_alpha_issuance` and against the 21,000,000 cap. A short that takes issuance to exactly
/// the cap opens, and one rao more is refused.
#[test]
fn residual_alpha_counts_in_issuance_and_against_the_supply_cap() {
    let q = 2_000 * RAO;
    for (over, ok) in [(0u64, true), (1, false)] {
        new_test_ext(1).execute_with(|| {
            let netuid = lending_subnet(50);
            let before = issuance(netuid);
            let pos = short_with_collateral_left(netuid, 0);
            run_charge(&pos);
            assert_eq!(defaults().len(), 1);
            assert_eq!(ResidualAlpha::<Test>::get(netuid), q);
            assert_eq!(issuance(netuid), before + q);

            // Fill issuance with outstanding alpha up to the cap less the next short.
            let outstanding = SubnetAlphaOut::<Test>::get(netuid).to_u64();
            let room = ALPHA_SUPPLY_CAP - q + over - issuance(netuid);
            SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(outstanding + room));
            assert_eq!(issuance(netuid), ALPHA_SUPPLY_CAP - q + over);
            let open = Lending::open_short(RuntimeOrigin::signed(bob()), netuid, q, 5_000 * RAO, 0);
            if ok {
                assert_ok!(open);
                assert_eq!(issuance(netuid), ALPHA_SUPPLY_CAP);
                assert_eq!(Lending::lending_alpha(netuid), 2 * q);
            } else {
                assert_noop!(open, pallet_lending::Error::<Test>::AlphaSupplyCap);
            }
        });
    }
}

/// Another dynamic subnet with its moving price at `price`.
fn priced_subnet(seed: u64, price: f64) -> NetUid {
    let netuid = add_dynamic_network(&U256::from(seed), &U256::from(seed + 1));
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::saturating_from_num(price));
    netuid
}

/// The subnets the coinbase gives TAO emission in this state, by its own entry point.
fn coinbase_emission_set() -> Vec<NetUid> {
    let all = SubtensorModule::get_all_subnet_netuids();
    let eligible = SubtensorModule::get_subnets_to_emit_to(&all);
    let block_emission = U96F32::saturating_from_num(1_000_000_000_000_000_000u64);
    SubtensorModule::get_subnet_block_emissions(&eligible, block_emission)
        .into_iter()
        .filter(|(_, emission)| *emission > U96F32::saturating_from_num(0))
        .map(|(netuid, _)| netuid)
        .collect()
}

/// The adapter's predicate is the coinbase's emission set: on a fixture with a subnet failing
/// each condition (no first emission block, subtoken disabled, registration closed, emission
/// disabled, no price share), `is_in_emission_set` is true exactly for the subnets
/// `get_subnet_block_emissions` gives a nonzero emission, and it writes nothing.
#[test]
fn the_emission_set_predicate_agrees_with_the_coinbase() {
    new_test_ext(1).execute_with(|| {
        let lending = lending_subnet(50);
        let healthy = priced_subnet(10, 0.02);
        let no_first_block = priced_subnet(20, 0.03);
        FirstEmissionBlockNumber::<Test>::remove(no_first_block);
        let no_subtoken = priced_subnet(30, 0.03);
        SubtokenEnabled::<Test>::insert(no_subtoken, false);
        let closed = priced_subnet(40, 0.03);
        NetworkRegistrationAllowed::<Test>::insert(closed, false);
        let disabled = priced_subnet(50, 0.03);
        SubnetEmissionEnabled::<Test>::insert(disabled, false);
        let no_share = priced_subnet(60, 0.0);
        // A stored bar, as on a running chain: the coinbase applies it without updating it in
        // this block, so both sides read the same state.
        let bar = U64F64::saturating_from_num(0.05);
        EmissionGateBar::<Test>::put(bar);

        let all = SubtensorModule::get_all_subnet_netuids();
        let predicate: Vec<NetUid> = all
            .iter()
            .copied()
            .filter(|netuid| {
                <SubtensorModule as LendingPoolInterface<U256>>::is_in_emission_set(*netuid)
            })
            .collect();
        assert_eq!(EmissionGateBar::<Test>::get(), bar);
        assert_eq!(predicate, coinbase_emission_set());
        assert_eq!(EmissionGateBar::<Test>::get(), bar);
        assert_eq!(predicate, SubtensorModule::emission_set());
        assert_eq!(predicate, vec![lending, healthy]);
        for out in [
            NetUid::ROOT,
            no_first_block,
            no_subtoken,
            closed,
            disabled,
            no_share,
        ] {
            assert!(!SubtensorModule::is_in_emission_set(out), "{out:?}");
        }

        // A subnet that comes back is in the set again, on both sides.
        SubnetEmissionEnabled::<Test>::insert(disabled, true);
        assert!(SubtensorModule::is_in_emission_set(disabled));
        assert!(coinbase_emission_set().contains(&disabled));
    });
}

/// The emission-set gate through the real adapter: a short opens and grows only on a subnet
/// in the emission set (`SubnetNotInEmission` otherwise), while a top-up and a close always
/// work, so a holder on a subnet that dropped out can still reduce or leave.
#[test]
fn shorts_open_and_grow_only_on_a_subnet_in_the_emission_set() {
    new_test_ext(1).execute_with(|| {
        let netuid = lending_subnet(50);
        let q = 2_000 * RAO;
        let open =
            |who: U256| Lending::open_short(RuntimeOrigin::signed(who), netuid, q, 5_000 * RAO, 0);
        let grow =
            || Lending::increase_short(RuntimeOrigin::signed(alice()), netuid, q, 5_000 * RAO, 0);

        // Out of the set before any short: nothing opens.
        SubnetEmissionEnabled::<Test>::insert(netuid, false);
        assert_noop!(
            open(alice()),
            pallet_lending::Error::<Test>::SubnetNotInEmission
        );
        SubnetEmissionEnabled::<Test>::insert(netuid, true);
        assert_ok!(open(alice()));
        assert_ok!(grow());

        // The subnet drops out by each of the coinbase's conditions in turn.
        let drops: [(fn(NetUid), fn(NetUid)); 3] = [
            (
                |n| SubnetEmissionEnabled::<Test>::insert(n, false),
                |n| SubnetEmissionEnabled::<Test>::insert(n, true),
            ),
            (
                |n| SubtokenEnabled::<Test>::insert(n, false),
                |n| SubtokenEnabled::<Test>::insert(n, true),
            ),
            (
                |n| NetworkRegistrationAllowed::<Test>::insert(n, false),
                |n| NetworkRegistrationAllowed::<Test>::insert(n, true),
            ),
        ];
        for (drop, restore) in drops {
            drop(netuid);
            assert_noop!(grow(), pallet_lending::Error::<Test>::SubnetNotInEmission);
            assert_noop!(
                open(bob()),
                pallet_lending::Error::<Test>::SubnetNotInEmission
            );
            restore(netuid);
        }

        // Dropped out for good: the holder can still top up, reduce and leave.
        SubnetEmissionEnabled::<Test>::insert(netuid, false);
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(alice()),
            netuid,
            100 * RAO
        ));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            u64::MAX
        ));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            u64::MAX
        ));
        assert!(short(alice(), netuid).is_none());
        assert_ok!(Lending::do_try_state());
    });
}
