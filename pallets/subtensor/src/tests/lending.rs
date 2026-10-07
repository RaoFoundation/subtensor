#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use super::mock::*;
use crate::*;
use frame_support::{
    StorageDoubleMap as _, assert_noop, assert_ok, traits::Hooks, weights::WeightMeter,
};
use pallet_lending::{LendingPoolInterface, Positions, Side, Vaults};
use safe_math::FixedExt;
use sp_core::U256;
use sp_runtime::traits::AccountIdConversion;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::{Order, OrderSwapInterface, SwapHandler};

const UNIT: u64 = 1_000_000_000;
const POOL: u64 = 100_000 * UNIT;
const COLLATERAL: u64 = 100 * UNIT;

/// Unlike the lending pallet's arithmetic mock, this market has real currency,
/// real alpha shares, and the production Subtensor/AMM custody adapter.
fn market(backed: bool) -> NetUid {
    let netuid = NetUid::from(1);
    add_network(netuid, 360, 0);
    SubnetMechanism::<Test>::insert(netuid, 1);
    NetworkRegisteredAt::<Test>::insert(netuid, 1);
    TaoInRefundDeploymentBlock::<Test>::put(0);
    setup_reserves(netuid, POOL.into(), POOL.into());
    TotalStake::<Test>::put(TaoBalance::from(POOL));
    if backed {
        let account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        add_balance_to_coldkey_account(&account, POOL.into());
    }
    System::set_block_number(7_201);
    SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));
    netuid
}

fn funded_market() -> NetUid {
    let netuid = market(true);
    assert_ok!(SubtensorModule::fund_unreachable_reserves(netuid, true));
    assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
    netuid
}

fn borrower(id: u64) -> (U256, U256) {
    let owner = U256::from(id);
    let hotkey = U256::from(id + 10_000);
    assert_ok!(SubtensorModule::create_account_if_non_existent(
        &owner, &hotkey
    ));
    add_balance_to_coldkey_account(&owner, (2_000 * UNIT).into());
    (owner, hotkey)
}

fn buy(owner: &U256, hotkey: &U256, netuid: NetUid, amount: u64) -> AlphaBalance {
    <SubtensorModule as OrderSwapInterface<U256>>::buy_alpha(
        owner,
        hotkey,
        netuid,
        amount.into(),
        u64::MAX.into(),
        true,
    )
    .unwrap()
}

fn stake(owner: &U256, hotkey: &U256, netuid: NetUid) -> u64 {
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, owner, netuid).to_u64()
}

fn open(owner: U256, hotkey: U256, netuid: NetUid, side: Side) {
    let quote = Lending::quote_open(netuid, side, COLLATERAL).unwrap();
    assert_ok!(Lending::open(
        RuntimeOrigin::signed(owner),
        netuid,
        side,
        COLLATERAL,
        hotkey,
        quote.principal,
        if side == Side::Short {
            quote.opening_value
        } else {
            0
        },
    ));
}

fn meter() -> WeightMeter {
    WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX))
}

fn burn_account() -> U256 {
    <Test as Config>::BurnAccountId::get().into_account_truncating()
}

fn redemption_supply(netuid: NetUid) -> u128 {
    <SubtensorModule as LendingPoolInterface<U256>>::redemption_alpha_supply(netuid).unwrap()
}

fn alpha_redemption_basis(netuid: NetUid, available: u64) -> (TaoBalance, u128) {
    <SubtensorModule as LendingPoolInterface<U256>>::alpha_loan_redemption_basis(
        netuid,
        available.into(),
    )
    .unwrap()
}

fn freeze_basis(netuid: NetUid, status: &crate::subnets::dissolution::DissolveCleanupStatus) {
    assert_ok!(Lending::freeze_redemption_basis(
        netuid,
        SubnetTAO::<Test>::get(netuid),
        status.subnet_total_alpha_value.unwrap(),
        DissolutionEligibleAlphaRows::<Test>::get(netuid),
    ));
}

fn grow(
    owner: U256,
    hotkey: U256,
    netuid: NetUid,
    side: Side,
    collateral: u64,
) -> pallet_lending::OpeningQuote {
    let quote = Lending::quote_open_for(&owner, netuid, side, collateral, &hotkey).unwrap();
    assert_ok!(Lending::open(
        RuntimeOrigin::signed(owner),
        netuid,
        side,
        collateral,
        hotkey,
        quote.principal,
        if side == Side::Short {
            quote.opening_value
        } else {
            0
        },
    ));
    quote
}

fn assert_single_position(owner: U256, hotkey: U256, netuid: NetUid) {
    let position = Positions::<Test>::get(owner, netuid).unwrap();
    let escrow = Lending::position_account(&owner, netuid);
    assert_eq!(Positions::<Test>::iter().count(), 1);
    assert_eq!(
        pallet_lending::OpenByNetuid::<Test>::iter_prefix(netuid).count(),
        1
    );
    assert_eq!(
        pallet_lending::EscrowOwner::<Test>::get(netuid, escrow),
        Some(owner)
    );
    assert_eq!(pallet_lending::PositionCount::<Test>::get(netuid), 1);
    assert_eq!(pallet_lending::TotalPositions::<Test>::get(), 1);
    assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(hotkey), 1);
    assert_eq!(pallet_lending::Due::<Test>::iter().count(), 1);
    assert!(pallet_lending::Due::<Test>::contains_key(
        position.due,
        (owner, netuid)
    ));
    assert_eq!(pallet_lending::NextDue::<Test>::get(), Some(position.due));
    assert_live_stake_total();
    assert_total_alpha_staked_invariant(netuid);
}

#[test]
fn growing_alpha_loan_preserves_coupon_and_one_position_then_closes_combined_debt() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(210);
        let original_vault = Vaults::<Test>::get(netuid).unwrap();
        open(owner, hotkey, netuid, Side::Short);
        let first = Positions::<Test>::get(owner, netuid).unwrap();
        let elapsed = 100;
        System::set_block_number(System::block_number() + elapsed);
        pallet_lending::References::<Test>::mutate(netuid, |reference| {
            reference.as_mut().unwrap().price = substrate_fixed::types::U64F64::from_num(2);
        });
        let burn = burn_account();
        let burn_before = Balances::free_balance(burn);
        let interest_numerator = u128::from(first.annual_interest) * u128::from(elapsed);
        let paid = (interest_numerator / u128::from(LendingBlocksPerYear::get())) as u64;
        let remainder = (interest_numerator % u128::from(LendingBlocksPerYear::get())) as u64;
        let added_collateral = 2 * COLLATERAL;
        let added = grow(owner, hotkey, netuid, Side::Short, added_collateral);
        let combined = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(combined.principal, first.principal + added.principal);
        assert_eq!(
            combined.collateral,
            first.collateral - paid + added_collateral
        );
        assert_eq!(combined.proceeds, 0);
        assert_eq!(stake(&owner, &hotkey, netuid), combined.principal);
        assert_eq!(
            combined.annual_interest,
            first.annual_interest + added.annual_interest
        );
        assert_eq!(combined.interest_remainder, remainder);
        assert_eq!(combined.last_accrued, System::block_number());
        assert_eq!(combined.due, first.due);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_tao, paid);
        assert!(System::events().iter().any(|record| record.event
            == RuntimeEvent::Lending(pallet_lending::Event::Increased {
                owner,
                netuid,
                side: Side::Short,
                principal: added.principal,
                collateral: added_collateral,
                proceeds: 0,
                annual_interest: added.annual_interest,
            })));
        let escrow = Lending::position_account(&owner, netuid);
        assert_eq!(
            Balances::free_balance(escrow).to_u64(),
            combined.collateral + combined.proceeds
        );
        assert_single_position(owner, hotkey, netuid);
        Lending::on_idle(System::block_number(), meter().remaining());
        assert_eq!(
            Balances::free_balance(burn),
            burn_before + TaoBalance::from(paid)
        );
        let closing = Lending::quote_close(&owner, netuid, false).unwrap();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            false,
            closing.payment,
            closing.refund,
        ));
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.available_alpha, original_vault.available_alpha);
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_eq!(pallet_lending::PositionCount::<Test>::get(netuid), 0);
        assert_eq!(pallet_lending::TotalPositions::<Test>::get(), 0);
        assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(hotkey), 0);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn real_long_loop_grows_one_loan_under_combined_funded_ltv_and_burns_interest() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(211);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        let initial_alpha = stake(&owner, &hotkey, netuid);
        let original_vault = Vaults::<Test>::get(netuid).unwrap();
        open(owner, hotkey, netuid, Side::Long);
        let first = Positions::<Test>::get(owner, netuid).unwrap();
        let bought = buy(&owner, &hotkey, netuid, first.principal).to_u64();
        let before_grow = Vaults::<Test>::get(netuid).unwrap();
        let supply = redemption_supply(netuid);
        let combined_collateral = first.collateral + bought;
        let expected = (u128::from(combined_collateral) * u128::from(before_grow.available_tao)
            - 4 * u128::from(first.principal) * supply)
            / (4 * supply + u128::from(combined_collateral));
        let added = grow(owner, hotkey, netuid, Side::Long, bought);
        assert_eq!(u128::from(added.principal), expected);
        let combined = Positions::<Test>::get(owner, netuid).unwrap();
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(combined.principal, first.principal + added.principal);
        assert_eq!(combined.collateral, combined_collateral);
        assert_eq!(
            combined.annual_interest,
            first.annual_interest + added.annual_interest
        );
        assert_eq!(combined.proceeds, 0);
        assert_eq!(combined.due, first.due);
        assert_eq!(
            vault.available_tao,
            before_grow.available_tao - added.principal
        );
        assert_eq!(vault.outstanding_tao, combined.principal);
        assert!(
            4 * u128::from(combined.principal) * supply
                <= u128::from(combined.collateral) * u128::from(vault.available_tao)
        );
        assert_single_position(owner, hotkey, netuid);
        let coupon = (u128::from(combined.annual_interest)
            * u128::from(LendingInterestPeriod::get())
            / u128::from(LendingBlocksPerYear::get())) as u64;
        let proceeds =
            <SubtensorModule as LendingPoolInterface<U256>>::quote_sell(netuid, coupon.into())
                .unwrap();
        let burn_before = Balances::free_balance(burn_account());
        System::set_block_number(combined.due);
        Lending::on_idle(System::block_number(), meter().remaining());
        assert_eq!(
            Balances::free_balance(burn_account()),
            burn_before + proceeds
        );
        let closing = Lending::quote_close(&owner, netuid, false).unwrap();
        assert_eq!(closing.payment, combined.principal);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            false,
            closing.payment,
            closing.refund,
        ));
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_tao, 0);
        assert_eq!(vault.available_tao, original_vault.available_tao);
        let total_interest = (u128::from(combined.annual_interest)
            * u128::from(LendingInterestPeriod::get()))
        .div_ceil(u128::from(LendingBlocksPerYear::get())) as u64;
        assert_eq!(
            stake(&owner, &hotkey, netuid),
            initial_alpha + bought - total_interest
        );
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn growing_long_adds_a_new_fixed_alpha_coupon_without_repricing_the_old_loan() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(215);
        buy(&owner, &hotkey, netuid, 500 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let first = Positions::<Test>::get(owner, netuid).unwrap();
        let elapsed = 100;
        System::set_block_number(System::block_number() + elapsed);
        pallet_lending::References::<Test>::mutate(netuid, |reference| {
            reference.as_mut().unwrap().price = substrate_fixed::types::U64F64::from_num(2);
        });
        let wallet = Balances::free_balance(owner);
        let source_alpha = stake(&owner, &hotkey, netuid);
        let paid = (u128::from(first.annual_interest) * u128::from(elapsed)
            / u128::from(LendingBlocksPerYear::get())) as u64;
        let added = grow(owner, hotkey, netuid, Side::Long, COLLATERAL);
        let combined = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(
            combined.annual_interest,
            first.annual_interest + added.annual_interest
        );
        assert!(combined.annual_interest > combined.principal.div_ceil(2));
        assert_eq!(combined.collateral, first.collateral - paid + COLLATERAL);
        assert_eq!(combined.due, first.due);
        assert_eq!(
            Balances::free_balance(owner),
            wallet + TaoBalance::from(added.principal)
        );
        assert_eq!(stake(&owner, &hotkey, netuid), source_alpha - COLLATERAL);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_alpha, paid);
        assert_eq!(
            stake(
                &Lending::position_account(&owner, netuid),
                &Lending::custody_hotkey().unwrap(),
                netuid
            ),
            combined.collateral
        );
        assert_single_position(owner, hotkey, netuid);
    });
}

#[test]
fn growing_position_rejects_side_hotkey_and_quote_mismatches_without_charging_interest() {
    for side in [Side::Short, Side::Long] {
        new_test_ext(1).execute_with(|| {
            let netuid = funded_market();
            let (owner, hotkey) = borrower(212);
            let (_, different_hotkey) = borrower(213);
            if side == Side::Long {
                buy(&owner, &hotkey, netuid, 500 * UNIT);
            }
            open(owner, hotkey, netuid, side);
            System::set_block_number(System::block_number() + 100);
            let wrong_side = if side == Side::Short {
                Side::Long
            } else {
                Side::Short
            };
            for (requested_side, requested_hotkey) in
                [(wrong_side, hotkey), (side, different_hotkey)]
            {
                assert_noop!(
                    Lending::open(
                        RuntimeOrigin::signed(owner),
                        netuid,
                        requested_side,
                        COLLATERAL,
                        requested_hotkey,
                        0,
                        0,
                    ),
                    pallet_lending::Error::<Test>::PositionExists
                );
            }
            assert_noop!(
                Lending::open(
                    RuntimeOrigin::signed(owner),
                    netuid,
                    side,
                    COLLATERAL,
                    hotkey,
                    u64::MAX,
                    0,
                ),
                pallet_lending::Error::<Test>::BelowMinimumBorrow
            );
            if side == Side::Short {
                assert_noop!(
                    Lending::open(
                        RuntimeOrigin::signed(owner),
                        netuid,
                        side,
                        COLLATERAL,
                        hotkey,
                        0,
                        u64::MAX,
                    ),
                    pallet_lending::Error::<Test>::BelowMinimumProceeds
                );
            }
            assert_single_position(owner, hotkey, netuid);
        });
    }
}

#[test]
fn failed_growth_transfer_rolls_back_accrued_coupon_and_real_collateral() {
    for side in [Side::Short, Side::Long] {
        new_test_ext(1).execute_with(|| {
            let netuid = funded_market();
            let (owner, hotkey) = borrower(214);
            if side == Side::Long {
                buy(&owner, &hotkey, netuid, 500 * UNIT);
            }
            open(owner, hotkey, netuid, side);
            System::set_block_number(System::block_number() + 100);
            assert!(Lending::quote_open_for(&owner, netuid, side, COLLATERAL, &hotkey).is_ok());
            let empty = if side == Side::Short {
                owner
            } else {
                Lending::reserve_account(netuid)
            };
            let balance = SubtensorModule::get_coldkey_balance(&empty);
            assert_ok!(SubtensorModule::transfer_tao(
                &empty,
                &U256::from(999),
                balance
            ));
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert!(
                Lending::open(
                    RuntimeOrigin::signed(owner),
                    netuid,
                    side,
                    COLLATERAL,
                    hotkey,
                    0,
                    0,
                )
                .is_err()
            );
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
            assert_single_position(owner, hotkey, netuid);
        });
    }
}

#[test]
fn grown_long_deregisters_as_one_merged_debt_against_ordinary_funded_redemption() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(216);
        buy(&owner, &hotkey, netuid, 500 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        grow(owner, hotkey, netuid, Side::Long, COLLATERAL);
        let combined = Positions::<Test>::get(owner, netuid).unwrap();
        assert_single_position(owner, hotkey, netuid);
        System::set_block_number(combined.due);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        run_destroy_alpha_get_total_and_settle(netuid);
        let funded = Positions::<Test>::get(owner, netuid).unwrap().proceeds;
        assert!(funded > combined.principal);
        let wallet_after_ordinary_payout = Balances::free_balance(owner);
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_tao, 0);
        assert_eq!(vault.available_tao, combined.principal);
        assert_eq!(vault.lost_tao, 0);
        assert_eq!(
            Balances::free_balance(owner),
            wallet_after_ordinary_payout + TaoBalance::from(funded - combined.principal)
        );
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_eq!(pallet_lending::PositionCount::<Test>::get(netuid), 0);
        assert_eq!(pallet_lending::TotalPositions::<Test>::get(), 0);
        assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(hotkey), 0);
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn lending_redemption_supply_counts_actual_claims_and_future_pool_claims() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(190);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(20 * UNIT));
        let expected = u128::from(SubnetAlphaIn::<Test>::get(netuid).to_u64())
            + u128::from(SubnetProtocolAlpha::<Test>::get(netuid).to_u64())
            + u128::from(TotalAlphaStaked::<Test>::get(netuid).to_u64());
        assert_eq!(redemption_supply(netuid), expected);
        let loan = Lending::quote_open(netuid, Side::Long, COLLATERAL).unwrap();

        // The issuance tracker can include burned alpha and emissions that have
        // not reached a holder. Neither constitutes an actual redemption claim.
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(u64::MAX));
        assert_eq!(redemption_supply(netuid), expected);
        assert_eq!(
            Lending::quote_open(netuid, Side::Long, COLLATERAL).unwrap(),
            loan
        );

        // Legacy deregistration excludes unsold pool alpha today. Lending still
        // counts it because buyers can acquire eligible claims before closure.
        TaoInRefundDeploymentBlock::<Test>::put(1);
        assert_eq!(redemption_supply(netuid), expected);
        NetworkRegisteredAt::<Test>::insert(netuid, 2);
        assert_eq!(redemption_supply(netuid), expected);
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn lending_redemption_supply_tracks_real_burns_instead_of_alpha_out() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(191);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        let supply = redemption_supply(netuid);
        let issued = SubnetAlphaOut::<Test>::get(netuid);
        let staked = TotalAlphaStaked::<Test>::get(netuid);
        let burned = AlphaBalance::from(10 * UNIT);
        assert_ok!(SubtensorModule::do_burn_alpha(
            RuntimeOrigin::signed(owner),
            hotkey,
            burned,
            netuid,
        ));
        assert_eq!(SubnetAlphaOut::<Test>::get(netuid), issued);
        assert_eq!(TotalAlphaStaked::<Test>::get(netuid), staked - burned);
        assert_eq!(
            redemption_supply(netuid),
            supply - u128::from(burned.to_u64())
        );
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn lending_redemption_supply_rejects_missing_empty_and_partial_backfill() {
    new_test_ext(1).execute_with(|| {
        let missing = NetUid::from(999);
        // Even stale counters cannot make a removed subnet eligible for lending.
        SubnetAlphaIn::<Test>::insert(missing, AlphaBalance::from(1));
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::redemption_alpha_supply(missing),
            Err(Error::<Test>::SubnetNotExists.into())
        );
        let netuid = market(true);
        SubnetAlphaIn::<Test>::remove(netuid);
        SubnetProtocolAlpha::<Test>::remove(netuid);
        TotalAlphaStaked::<Test>::remove(netuid);
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::redemption_alpha_supply(netuid),
            Err(Error::<Test>::AmountTooLow.into())
        );

        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1));
        assert_eq!(redemption_supply(netuid), 1);
        crate::migrations::migrate_total_alpha_staked::migrate_total_alpha_staked::<Test>();
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::redemption_alpha_supply(netuid),
            Err(Error::<Test>::LendingUnavailable.into())
        );
    });
}

#[test]
fn lending_redemption_supply_sums_without_saturating_token_balances() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(u64::MAX));
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(u64::MAX));
        TotalAlphaStaked::<Test>::insert(netuid, AlphaBalance::from(u64::MAX));
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(
            netuid,
            AlphaBalance::from(u64::MAX),
        );
        assert_eq!(redemption_supply(netuid), 4 * u128::from(u64::MAX));
    });
}

#[test]
fn alpha_loan_basis_counts_only_guaranteed_claims_for_each_subnet_era() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(17));
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(
            netuid,
            AlphaBalance::from(23),
        );
        pallet_subtensor_swap::BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(29));
        TotalAlphaStaked::<Test>::insert(netuid, AlphaBalance::from(u64::MAX));
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(u64::MAX));
        assert_eq!(
            alpha_redemption_basis(netuid, 31),
            ((POOL + 29).into(), u128::from(POOL + 17 + 23 + 31))
        );

        TaoInRefundDeploymentBlock::<Test>::put(1);
        assert_eq!(alpha_redemption_basis(netuid, 31), ((POOL + 29).into(), 17));
        SubnetProtocolAlpha::<Test>::remove(netuid);
        assert_eq!(
            alpha_redemption_basis(netuid, u64::MAX),
            ((POOL + 29).into(), 0)
        );
    });
}

#[test]
fn alpha_loan_basis_matches_saturating_ordinary_pool_balances() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(u64::MAX - 1));
        pallet_subtensor_swap::BalancerTaoReservoir::<Test>::insert(netuid, TaoBalance::from(2));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(u64::MAX - 3));
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(2));
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(
            netuid,
            AlphaBalance::from(2),
        );
        assert_eq!(
            alpha_redemption_basis(netuid, 2),
            (u64::MAX.into(), u128::from(u64::MAX))
        );
        TaoInRefundDeploymentBlock::<Test>::put(1);
        assert_eq!(alpha_redemption_basis(netuid, 2), (u64::MAX.into(), 2));

        let missing = NetUid::from(999);
        SubnetProtocolAlpha::<Test>::insert(missing, AlphaBalance::from(u64::MAX));
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::alpha_loan_redemption_basis(
                missing,
                AlphaBalance::from(u64::MAX),
            ),
            Err(Error::<Test>::SubnetNotExists.into())
        );
    });
}

#[test]
fn legacy_free_alpha_cannot_drain_a_funded_pot_without_guaranteed_protocol_claims() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        TaoInRefundDeploymentBlock::<Test>::put(1);
        let (owner, hotkey) = borrower(197);
        let vault = Vaults::<Test>::get(netuid).unwrap();
        let account = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let owner_cash = Balances::free_balance(owner);
        let reserve_cash = Balances::free_balance(account);
        let reserve_alpha = stake(&account, &custody, netuid);
        assert_eq!(
            SubnetTAO::<Test>::get(netuid).to_u64() + vault.available_tao,
            POOL
        );
        assert_eq!(alpha_redemption_basis(netuid, vault.available_alpha).1, 0);
        assert_eq!(
            Lending::quote_open(netuid, Side::Short, COLLATERAL),
            Err(pallet_lending::Error::<Test>::InsufficientRedemptionBacking.into())
        );
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid,
                Side::Short,
                COLLATERAL,
                hotkey,
                0,
                0
            ),
            pallet_lending::Error::<Test>::InsufficientRedemptionBacking
        );
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_eq!(Vaults::<Test>::get(netuid).unwrap(), vault);
        assert_eq!(Balances::free_balance(owner), owner_cash);
        assert_eq!(Balances::free_balance(account), reserve_cash);
        assert_eq!(stake(&account, &custody, netuid), reserve_alpha);
        assert_eq!(stake(&owner, &hotkey, netuid), 0);
        assert_total_alpha_staked_invariant(netuid);
        assert_live_stake_total();
    });
}

#[test]
fn funded_alpha_grants_and_growth_reserve_a_quarter_of_collateral_for_redemption() {
    for legacy in [false, true] {
        new_test_ext(1).execute_with(|| {
            let netuid = funded_market();
            if legacy {
                TaoInRefundDeploymentBlock::<Test>::put(1);
                // A real protocol claim is counted in both ordinary payout eras.
                SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(POOL));
            }
            let (owner, hotkey) = borrower(198);
            open(owner, hotkey, netuid, Side::Short);
            grow(owner, hotkey, netuid, Side::Short, COLLATERAL);
            let loan = Positions::<Test>::get(owner, netuid).unwrap();
            let vault = Vaults::<Test>::get(netuid).unwrap();
            let (pool_pot, n) = alpha_redemption_basis(netuid, vault.available_alpha);
            let pot = pool_pot.to_u64().saturating_add(vault.available_tao);
            let funded = (u128::from(loan.principal) * u128::from(pot)).div_ceil(n)
                + u128::from(loan.principal);
            assert!(funded <= u128::from(loan.collateral / 4));
            assert_eq!(stake(&owner, &hotkey, netuid), loan.principal);
            assert_single_position(owner, hotkey, netuid);
            assert_total_alpha_staked_invariant(netuid);
        });
    }
}

#[test]
fn long_open_waits_for_the_redemption_supply_backfill() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(193);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        assert!(Lending::quote_open(netuid, Side::Long, COLLATERAL).is_ok());
        crate::migrations::migrate_total_alpha_staked::migrate_total_alpha_staked::<Test>();
        assert_eq!(
            Lending::quote_open(netuid, Side::Long, COLLATERAL),
            Err(pallet_lending::Error::<Test>::RedemptionUnavailable.into())
        );
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid,
                Side::Long,
                COLLATERAL,
                hotkey,
                0,
                0,
            ),
            pallet_lending::Error::<Test>::RedemptionUnavailable
        );
        assert_total_alpha_staked_invariant(netuid);
        assert_live_stake_total();
    });
}

#[test]
fn lending_supply_bounds_real_fractional_share_redemption_for_both_subnet_eras() {
    for legacy in [false, true] {
        new_test_ext(1).execute_with(|| {
            let netuid = funded_market();
            if legacy {
                TaoInRefundDeploymentBlock::<Test>::put(1);
            }
            let (first, hotkey) = borrower(194);
            let (second, _) = borrower(195);
            let (third, _) = borrower(196);
            buy(&first, &hotkey, netuid, 137 * UNIT + 13);
            buy(&second, &hotkey, netuid, 211 * UNIT + 17);

            // Nominator dividends revalue existing shares; a later deposit then
            // receives fractional shares at that new value per share.
            let dividend = AlphaBalance::from(7 * UNIT + 19);
            SubtensorModule::resolve_to_alpha_out(SubtensorModule::mint_alpha(netuid, dividend));
            SubtensorModule::increase_stake_for_hotkey_on_subnet(&hotkey, netuid, dividend);
            buy(&third, &hotkey, netuid, 173 * UNIT + 23);
            for owner in [first, second, third] {
                assert!(stake(&owner, &hotkey, netuid) > 0);
            }
            assert_total_alpha_staked_invariant(netuid);
            let before_buffer = redemption_supply(netuid);
            let buffered = AlphaBalance::from(91 * UNIT + 29);
            pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(netuid, buffered);
            let supply = redemption_supply(netuid);
            assert_eq!(supply, before_buffer + u128::from(buffered.to_u64()));
            assert_ok!(SubtensorModule::do_dissolve_network(netuid));
            let mut status = dissolve_cleanup_status(netuid);
            status
                .set_phase(crate::subnets::dissolution::DissolveCleanupPhase::LendingSettleShorts);
            let mut stage = WeightMeter::with_limit(
                <Test as frame_system::Config>::DbWeight::get().reads_writes(7, 5),
            );
            SubtensorModule::clean_up_data_for_one_dissolved_network(&mut stage, &mut status);
            assert_eq!(
                <Test as Config>::SwapInterface::protocol_alpha_reservoir(netuid),
                AlphaBalance::ZERO
            );
            assert!(
                pallet_lending::Dissolutions::<Test>::get(netuid)
                    .unwrap()
                    .reserves_returned
            );
            assert!(
                SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                    netuid,
                    &mut meter(),
                    None,
                    &mut status,
                )
                .0
            );
            let denominator = status.subnet_total_alpha_value.unwrap();
            assert!(
                supply >= denominator,
                "lending denominator {supply} must cover actual payout claims {denominator}"
            );
            if legacy {
                assert!(supply > denominator);
            }
        });
    }
}

#[test]
fn long_quote_uses_post_withdrawal_funded_backing_and_excludes_active_amm_tao() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(192);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
        let supply = redemption_supply(netuid);
        let expected = u128::from(COLLATERAL) * u128::from(before_vault.available_tao)
            / (4 * supply + u128::from(COLLATERAL));
        let quote = Lending::quote_open(netuid, Side::Long, COLLATERAL).unwrap();
        assert_eq!(u128::from(quote.principal), expected);
        assert!(quote.principal < COLLATERAL / 4);
        assert_eq!(quote.annual_interest, quote.principal);

        // Real buying adds TAO to the active AMM, which sellers can extract.
        // It must not increase the protected backing used for TAO lending.
        let active_tao = SubnetTAO::<Test>::get(netuid);
        buy(&owner, &hotkey, netuid, 1_000 * UNIT);
        assert!(SubnetTAO::<Test>::get(netuid) > active_tao);
        assert_eq!(redemption_supply(netuid), supply);
        assert_eq!(
            Lending::quote_open(netuid, Side::Long, COLLATERAL).unwrap(),
            quote
        );
        let pool = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
        );
        open(owner, hotkey, netuid, Side::Long);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(position.principal, quote.principal);
        assert_eq!(
            vault.available_tao,
            before_vault.available_tao - quote.principal
        );
        assert!(
            4 * u128::from(position.principal) * supply
                <= u128::from(position.collateral) * u128::from(vault.available_tao)
        );
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            ),
            pool
        );
        assert_total_alpha_staked_invariant(netuid);
        assert_live_stake_total();
    });
}

#[test]
fn a_new_long_cannot_spend_backing_needed_by_an_existing_loan_after_interest() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (first, first_hotkey) = borrower(197);
        let (second, second_hotkey) = borrower(198);
        buy(&first, &first_hotkey, netuid, 200 * UNIT);
        buy(&second, &second_hotkey, netuid, 200 * UNIT);
        open(first, first_hotkey, netuid, Side::Long);
        let position = Positions::<Test>::get(first, netuid).unwrap();
        assert!(Lending::quote_open(netuid, Side::Long, COLLATERAL).is_ok());

        // Interest has accrued but has not yet been collected by the weekly hook.
        // Once 80% of collateral is due, even the protected reserve would not
        // cover this loan; the quote must not lend its remaining backing away.
        let elapsed =
            (u128::from(position.collateral) * u128::from(LendingBlocksPerYear::get()) * 4
                / (u128::from(position.annual_interest) * 5)) as u64;
        System::set_block_number(System::block_number() + elapsed);
        assert_eq!(
            Lending::quote_open(netuid, Side::Long, COLLATERAL),
            Err(pallet_lending::Error::<Test>::InsufficientRedemptionBacking.into())
        );
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(second),
                netuid,
                Side::Long,
                COLLATERAL,
                second_hotkey,
                0,
                0,
            ),
            pallet_lending::Error::<Test>::InsufficientRedemptionBacking
        );
        // Valuation does not collect fees or liquidate the existing position.
        assert_eq!(Positions::<Test>::get(first, netuid).unwrap(), position);
        assert!(!Positions::<Test>::contains_key(second, netuid));
        assert_total_alpha_staked_invariant(netuid);
        assert_live_stake_total();
    });
}

#[test]
fn lending_fee_burn_uses_canonical_address_without_changing_issuance_or_inventory() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (payer, _) = borrower(160);
        let burn = burn_account();
        let before_payer = Balances::free_balance(payer);
        let before_burn = Balances::free_balance(burn);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let vault = Vaults::<Test>::get(netuid).unwrap();
        let pool = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
        );

        assert_ok!(
            <SubtensorModule as LendingPoolInterface<U256>>::burn_interest_tao(&payer, UNIT.into())
        );
        assert_eq!(
            Balances::free_balance(payer),
            before_payer - TaoBalance::from(UNIT)
        );
        assert_eq!(
            Balances::free_balance(burn),
            before_burn + TaoBalance::from(UNIT)
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap(), vault);
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            ),
            pool
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn failed_lending_fee_burn_preserves_all_currency_and_storage() {
    new_test_ext(1).execute_with(|| {
        let (payer, _) = borrower(161);
        let burn = burn_account();
        let balance = SubtensorModule::get_coldkey_balance(&payer);
        assert_noop!(
            <SubtensorModule as LendingPoolInterface<U256>>::burn_interest_tao(
                &payer,
                balance + TaoBalance::from(1)
            ),
            Error::<Test>::InsufficientTaoBalance
        );
        add_balance_to_coldkey_account(&burn, UNIT.into());
        assert_noop!(
            <SubtensorModule as LendingPoolInterface<U256>>::burn_interest_tao(&burn, 1.into()),
            Error::<Test>::InsufficientTaoBalance
        );
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_ok!(
            <SubtensorModule as LendingPoolInterface<U256>>::burn_interest_tao(
                &burn,
                TaoBalance::ZERO
            )
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn lending_fee_burn_rolls_back_sender_dust_and_underfunded_burn_address() {
    new_test_ext(1).execute_with(|| {
        ExistentialDeposit::set(500.into());
        let burn = burn_account();
        let payer = U256::from(16_162);
        add_balance_to_coldkey_account(&payer, 999.into());
        assert_eq!(Balances::free_balance(burn), TaoBalance::ZERO);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(
            <SubtensorModule as LendingPoolInterface<U256>>::burn_interest_tao(&payer, 499.into())
                .is_err()
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);

        add_balance_to_coldkey_account(&burn, 500.into());
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        // The canonical transfer would additionally reap 498 rao. None of that
        // unrelated balance may disappear as a side effect of burning a fee.
        assert_noop!(
            <SubtensorModule as LendingPoolInterface<U256>>::burn_interest_tao(&payer, 501.into()),
            Error::<Test>::InsufficientTaoBalance
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(Balances::free_balance(payer), 999.into());
        assert_eq!(Balances::free_balance(burn), 500.into());
        assert_eq!(TotalIssuance::<Test>::get(), Balances::total_issuance());
    });
}

#[test]
fn alpha_loan_quote_rejects_stale_value_after_a_real_purchase_changes_depth() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (first, first_hotkey) = borrower(180);
        open(first, first_hotkey, netuid, Side::Short);
        let (owner, hotkey) = borrower(181);
        let quoted = Lending::quote_open(netuid, Side::Short, COLLATERAL).unwrap();

        // Delivery itself performs no sale. A separate real purchase reduces
        // what the reference purchase can buy; the caller's value floor holds.
        let (other, other_hotkey) = borrower(182);
        open(other, other_hotkey, netuid, Side::Short);
        buy(&other, &other_hotkey, netuid, 1_000 * UNIT);
        let current = Lending::quote_open(netuid, Side::Short, COLLATERAL).unwrap();
        assert!(current.principal < quoted.principal);
        assert!(current.opening_value < quoted.opening_value);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid,
                Side::Short,
                COLLATERAL,
                hotkey,
                0,
                quoted.opening_value,
            ),
            pallet_lending::Error::<Test>::BelowMinimumProceeds
        );
        assert!(!Positions::<Test>::contains_key(owner, netuid));

        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner),
            netuid,
            Side::Short,
            COLLATERAL,
            hotkey,
            current.principal,
            current.opening_value,
        ));
        assert_eq!(Positions::<Test>::get(owner, netuid).unwrap().proceeds, 0);
        assert_total_alpha_staked_invariant(netuid);
        assert_live_stake_total();
    });
}

fn assert_live_stake_total() {
    let active = SubnetTAO::<Test>::iter()
        .filter(|(netuid, _)| SubtensorModule::if_subnet_exist(*netuid))
        .fold(TaoBalance::ZERO, |total, (_, value)| {
            total.saturating_add(value)
        });
    assert_eq!(TotalStake::<Test>::get(), active);
}

#[test]
fn funding_conserves_issuance_and_materializes_exact_custody() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let alpha_supply =
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(SubnetAlphaOut::<Test>::get(netuid));
        let price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        assert_ok!(SubtensorModule::fund_unreachable_reserves(netuid, true));
        let vault = Vaults::<Test>::get(netuid).unwrap();
        let account = Lending::reserve_account(netuid);
        let hotkey = Lending::custody_hotkey().unwrap();
        assert!(vault.available_tao > 0 && vault.available_alpha > 0);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&account).to_u64(),
            vault.available_tao
        );
        assert_eq!(stake(&account, &hotkey, netuid), vault.available_alpha);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(SubnetAlphaOut::<Test>::get(netuid)),
            alpha_supply
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_eq!(
            <Test as Config>::SwapInterface::current_alpha_price(netuid),
            price
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_ok!(SubtensorModule::fund_unreachable_reserves(netuid, true));
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn unfunded_extraction_rolls_back_curve_balances_and_custody() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(false);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(SubtensorModule::fund_unreachable_reserves(netuid, true).is_err());
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert!(!Vaults::<Test>::contains_key(netuid));
    });
}

#[test]
fn price_impact_tuning_conserves_real_assets_and_appends_a_new_vault() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let alpha_supply =
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(SubnetAlphaOut::<Test>::get(netuid));
        let price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        let (tao, alpha) =
            <SubtensorModule as LendingPoolInterface<U256>>::tune_min_price_impact(netuid, 100)
                .unwrap();
        assert!(!tao.is_zero() && !alpha.is_zero());
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.available_tao, tao.to_u64());
        assert_eq!(vault.available_alpha, alpha.to_u64());
        assert_eq!(
            SubnetTAO::<Test>::get(netuid).saturating_add(tao),
            POOL.into()
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(alpha),
            POOL.into()
        );
        let custody = Lending::custody_hotkey().unwrap();
        let account = Lending::reserve_account(netuid);
        assert_eq!(Balances::free_balance(account), tao);
        assert_eq!(stake(&account, &custody, netuid), alpha.to_u64());
        let after_price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        assert!(after_price.abs_diff(price).to_bits() <= price.to_bits() / UNIT as u128 + 1);
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(SubnetAlphaOut::<Test>::get(netuid)),
            alpha_supply
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn price_impact_tuning_preserves_live_short_and_long_contracts_and_redemption_backing() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (short_owner, short_hotkey) = borrower(240);
        let (long_owner, long_hotkey) = borrower(241);
        open(short_owner, short_hotkey, netuid, Side::Short);
        buy(&long_owner, &long_hotkey, netuid, 2 * COLLATERAL);
        open(long_owner, long_hotkey, netuid, Side::Long);
        let short = Positions::<Test>::get(short_owner, netuid).unwrap();
        let long = Positions::<Test>::get(long_owner, netuid).unwrap();
        let reference = pallet_lending::References::<Test>::get(netuid);
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
        let before_curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid).unwrap();
        let before_price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        let before_supply = redemption_supply(netuid);
        let before_basis = alpha_redemption_basis(netuid, before_vault.available_alpha);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let alpha_supply =
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(SubnetAlphaOut::<Test>::get(netuid));
        assert_ok!(Lending::set_min_price_impact(
            RuntimeOrigin::root(),
            netuid,
            100
        ));
        let after_vault = Vaults::<Test>::get(netuid).unwrap();
        let after_curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid).unwrap();
        assert_ne!(after_curve, before_curve);
        assert_eq!(pallet_lending::MinPriceImpactBps::<Test>::get(netuid), 100);
        assert!(after_vault.available_tao > before_vault.available_tao);
        assert!(after_vault.available_alpha > before_vault.available_alpha);
        assert_eq!(after_vault.outstanding_tao, before_vault.outstanding_tao);
        assert_eq!(
            after_vault.outstanding_alpha,
            before_vault.outstanding_alpha
        );
        assert_eq!(after_vault.pending_tao, before_vault.pending_tao);
        assert_eq!(after_vault.pending_alpha, before_vault.pending_alpha);
        assert_eq!(
            Positions::<Test>::get(short_owner, netuid),
            Some(short.clone())
        );
        assert_eq!(
            Positions::<Test>::get(long_owner, netuid),
            Some(long.clone())
        );
        assert_eq!(pallet_lending::References::<Test>::get(netuid), reference);
        assert_eq!(stake(&short_owner, &short_hotkey, netuid), short.principal);
        assert_eq!(redemption_supply(netuid), before_supply);
        let after_basis = alpha_redemption_basis(netuid, after_vault.available_alpha);
        assert_eq!(after_basis.1, before_basis.1);
        assert_eq!(
            after_basis.0.to_u64() + after_vault.available_tao,
            before_basis.0.to_u64() + before_vault.available_tao
        );
        let after_price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        assert!(
            after_price.abs_diff(before_price).to_bits()
                <= before_price.to_bits() / UNIT as u128 + 1
        );
        let (buy_ratio, sell_ratio) = after_curve
            .reference_price_impact(
                SubnetAlphaIn::<Test>::get(netuid).to_u64(),
                SubnetTAO::<Test>::get(netuid).to_u64(),
                500 * UNIT,
                SwapMinimumReserve::get().get(),
            )
            .unwrap();
        assert!(buy_ratio >= U64F64::from_num(1.01));
        assert!(sell_ratio <= U64F64::from_num(0.99));
        // The actual swap engine, with fees excluded, fully fills the reference
        // after extraction; mathematical phase capacity alone is insufficient.
        let purchase = Swap::swap(
            netuid,
            GetAlphaForTao::<Test>::with_amount(500 * UNIT),
            Swap::max_price::<TaoBalance>(),
            true,
            true,
        )
        .unwrap();
        assert_eq!(purchase.amount_paid_in.to_u64(), 500 * UNIT);
        assert_eq!(purchase.fee_paid.to_u64(), 0);
        let equivalent_alpha =
            u64::try_from((u128::from(500 * UNIT) << 64).div_ceil(after_price.to_bits())).unwrap();
        let sale = Swap::swap(
            netuid,
            GetTaoForAlpha::<Test>::with_amount(equivalent_alpha),
            Swap::min_price::<TaoBalance>(),
            true,
            true,
        )
        .unwrap();
        assert_eq!(sale.amount_paid_in.to_u64(), equivalent_alpha);
        assert_eq!(sale.fee_paid.to_u64(), 0);
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).saturating_add(SubnetAlphaOut::<Test>::get(netuid)),
            alpha_supply
        );
        let account = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        assert_eq!(
            Balances::free_balance(account).to_u64(),
            after_vault.available_tao
        );
        assert_eq!(
            stake(&account, &custody, netuid),
            after_vault.available_alpha
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);

        // Debt stays repayable in its original asset after the governance change.
        for owner in [short_owner, long_owner] {
            let quote = Lending::quote_close(&owner, netuid, false).unwrap();
            assert_ok!(Lending::close(
                RuntimeOrigin::signed(owner),
                netuid,
                false,
                quote.payment,
                quote.refund,
            ));
        }
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().outstanding_tao, 0);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().outstanding_alpha, 0);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn price_impact_lowering_and_disabling_never_widen_or_refund_vault_assets() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        assert_ok!(Lending::set_min_price_impact(
            RuntimeOrigin::root(),
            netuid,
            200
        ));
        let (owner, hotkey) = borrower(243);
        add_balance_to_coldkey_account(&owner, POOL.into());
        let curve_before_buy =
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid).unwrap();
        let capacity = curve_before_buy
            .max_buy_input_with_reserve_floor(
                SubnetAlphaIn::<Test>::get(netuid).to_u64(),
                SubnetTAO::<Test>::get(netuid).to_u64(),
                SwapMinimumReserve::get().get(),
            )
            .unwrap();
        // Move to a point where another full 500-TAO purchase no longer fits.
        // Relaxing an existing policy remains possible without reopening walls.
        buy(&owner, &hotkey, netuid, capacity - 100 * UNIT);
        let curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid);
        let vault = Vaults::<Test>::get(netuid);
        let reserves = (
            SubnetAlphaIn::<Test>::get(netuid),
            SubnetTAO::<Test>::get(netuid),
        );
        assert!(
            curve
                .as_ref()
                .unwrap()
                .reference_price_impact(
                    reserves.0.to_u64(),
                    reserves.1.to_u64(),
                    500 * UNIT,
                    SwapMinimumReserve::get().get(),
                )
                .is_err()
        );
        for target in [100, 1, 0] {
            assert_ok!(Lending::set_min_price_impact(
                RuntimeOrigin::root(),
                netuid,
                target
            ));
            assert_eq!(
                pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
                curve
            );
            assert_eq!(Vaults::<Test>::get(netuid), vault);
            assert_eq!(
                (
                    SubnetAlphaIn::<Test>::get(netuid),
                    SubnetTAO::<Test>::get(netuid)
                ),
                reserves
            );
            assert_eq!(
                pallet_lending::MinPriceImpactBps::<Test>::get(netuid),
                target
            );
        }
        // Automatic vault admission retains its one-time extraction policy.
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_ok!(SubtensorModule::fund_unreachable_reserves(netuid, true));
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn price_impact_thin_boundary_failure_rolls_back_target_curve_and_inventory() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        setup_reserves(netuid, (500 * UNIT).into(), (500 * UNIT).into());
        TotalStake::<Test>::put(TaoBalance::from(500 * UNIT));
        assert_ok!(SubtensorModule::fund_unreachable_reserves(netuid, true));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(Lending::set_min_price_impact(RuntimeOrigin::root(), netuid, 100).is_err());
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn price_impact_unfunded_custody_failure_rolls_back_the_entire_adapter_transaction() {
    new_test_ext(1).execute_with(|| {
        let netuid = market(false);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(
            <SubtensorModule as LendingPoolInterface<U256>>::tune_min_price_impact(netuid, 100)
                .is_err()
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert!(!Vaults::<Test>::contains_key(netuid));
    });
}

#[test]
fn price_impact_existing_vault_funding_failure_preserves_old_loans_and_target() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(242);
        open(owner, hotkey, netuid, Side::Short);
        let account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        assert_ok!(Balances::force_set_balance(
            RuntimeOrigin::root(),
            account,
            TaoBalance::ZERO,
        ));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(Lending::set_min_price_impact(RuntimeOrigin::root(), netuid, 100).is_err());
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn price_impact_adapter_rejects_subnet_lifecycle_locks() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        DissolveCleanupQueue::<Test>::put(vec![netuid]);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(
            <SubtensorModule as LendingPoolInterface<U256>>::tune_min_price_impact(netuid, 100)
                .is_err()
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn capacity_deferred_pool_stays_intact_and_is_admitted_after_a_slot_frees() {
    use crate::migrations::migrate_pool_lending::{MIGRATION_NAME, Migration};
    use frame_support::traits::OnRuntimeUpgrade;
    use pallet_lending::{LendingInterface, Vault, VaultCount};

    new_test_ext(1).execute_with(|| {
        let netuid = market(true);
        assert_ok!(Swap::maybe_initialize_palswap(netuid, None));
        // Empty existing vaults still occupy bounded reference-update slots.
        for id in 2_u16..=257 {
            Vaults::<Test>::insert(NetUid::from(id), Vault::default());
        }
        VaultCount::<Test>::put(256);
        assert!(!<Lending as LendingInterface<U256>>::has_funding_capacity());
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_ok!(SubtensorModule::fund_unreachable_reserves(netuid, true));
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        let curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid);
        let issuance = Balances::total_issuance();
        #[cfg(feature = "try-runtime")]
        let snapshot = Migration::<Test>::pre_upgrade().unwrap();
        Migration::<Test>::on_runtime_upgrade();
        #[cfg(feature = "try-runtime")]
        assert_ok!(Migration::<Test>::post_upgrade(snapshot));
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert!(pallet_lending::Enabled::<Test>::get());
        assert!(!Vaults::<Test>::contains_key(netuid));
        assert_eq!(SubnetTAO::<Test>::get(netuid).to_u64(), POOL);
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid).to_u64(), POOL);
        assert_eq!(
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            curve
        );
        assert_eq!(
            pallet_subtensor_swap::ExtractedReserves::<Test>::get(netuid),
            Default::default()
        );

        assert_ok!(Lending::finish_dissolution(NetUid::from(2)));
        assert!(<Lending as LendingInterface<U256>>::has_funding_capacity());
        SubtensorModule::fund_one_new_lending_vault();
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert!(vault.available_alpha > 0 && vault.available_tao > 0);
        assert_eq!(VaultCount::<Test>::get(), 256);
        assert_eq!(Balances::total_issuance(), issuance);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).to_u64() + vault.available_alpha,
            POOL
        );
        assert_eq!(
            SubnetTAO::<Test>::get(netuid).to_u64() + vault.available_tao,
            POOL
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn partial_funding_failure_keeps_migration_incomplete_and_retry_preserves_existing_vault() {
    use crate::migrations::migrate_pool_lending::{MIGRATION_NAME, Migration};
    use frame_support::traits::OnRuntimeUpgrade;

    new_test_ext(1).execute_with(|| {
        let existing = market(true);
        assert_ok!(SubtensorModule::fund_unreachable_reserves(existing, true));
        let previous = Vaults::<Test>::get(existing).unwrap();
        let previous_curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(existing);
        let failing = NetUid::from(2);
        add_network(failing, 360, 0);
        SubnetMechanism::<Test>::insert(failing, 1);
        setup_reserves(failing, POOL.into(), POOL.into());
        TotalStake::<Test>::mutate(|total| *total = total.saturating_add(POOL.into()));
        assert_ok!(Swap::maybe_initialize_palswap(failing, None));
        let failing_curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(failing);
        Migration::<Test>::on_runtime_upgrade();
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert!(!pallet_lending::Enabled::<Test>::get());
        assert!(!Vaults::<Test>::contains_key(failing));
        assert_eq!(Vaults::<Test>::get(existing), Some(previous.clone()));
        assert_eq!(
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(failing),
            failing_curve
        );
        assert_eq!(SubnetTAO::<Test>::get(failing).to_u64(), POOL);
        assert_eq!(SubnetAlphaIn::<Test>::get(failing).to_u64(), POOL);

        let account = SubtensorModule::get_subnet_account_id(failing).unwrap();
        add_balance_to_coldkey_account(&account, POOL.into());
        #[cfg(feature = "try-runtime")]
        let snapshot = Migration::<Test>::pre_upgrade().unwrap();
        Migration::<Test>::on_runtime_upgrade();
        #[cfg(feature = "try-runtime")]
        assert_ok!(Migration::<Test>::post_upgrade(snapshot));
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert!(pallet_lending::Enabled::<Test>::get());
        assert_eq!(Vaults::<Test>::get(existing), Some(previous));
        assert_eq!(
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(existing),
            previous_curve
        );
        assert!(Vaults::<Test>::contains_key(failing));
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(existing);
        assert_total_alpha_staked_invariant(failing);
    });
}

#[test]
fn alpha_loan_is_delivered_without_a_swap_and_can_move_before_fixed_token_repayment() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(100);
        let balance_before = SubtensorModule::get_coldkey_balance(&owner).to_u64();
        let alpha_before = Vaults::<Test>::get(netuid).unwrap().available_alpha;
        let pool_before = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
        );
        open(owner, hotkey, netuid, Side::Short);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let escrow = Lending::position_account(&owner, netuid);
        let custody = Lending::custody_hotkey().unwrap();
        assert_eq!(stake(&owner, &hotkey, netuid), position.principal);
        assert_eq!(stake(&escrow, &custody, netuid), 0);
        assert_eq!(position.proceeds, 0);
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid)
            ),
            pool_before,
            "opening lends real alpha without selling it"
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            balance_before - COLLATERAL
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow).to_u64(),
            COLLATERAL
        );
        assert_eq!(
            Vaults::<Test>::get(netuid).unwrap().available_alpha,
            alpha_before - position.principal
        );
        let (recipient, recipient_hotkey) = borrower(102);
        assert_ok!(
            <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                &owner,
                &hotkey,
                &recipient,
                &recipient_hotkey,
                netuid,
                position.principal.into(),
                true,
                false
            )
        );
        assert_eq!(stake(&owner, &hotkey, netuid), 0);
        assert_eq!(
            stake(&recipient, &recipient_hotkey, netuid),
            position.principal
        );
        assert_ok!(
            <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                &recipient,
                &recipient_hotkey,
                &owner,
                &hotkey,
                netuid,
                position.principal.into(),
                true,
                false
            )
        );
        let quote = Lending::quote_close(&owner, netuid, true).unwrap();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            true,
            quote.payment,
            quote.refund
        ));
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            balance_before - COLLATERAL + quote.refund
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow),
            TaoBalance::ZERO
        );
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.available_alpha, alpha_before);
        assert_eq!(stake(&owner, &hotkey, netuid), 0);
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid)
            ),
            pool_before
        );
        assert_eq!(
            stake(&Lending::reserve_account(netuid), &custody, netuid),
            vault.available_alpha
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn free_alpha_redemption_and_terminal_recovery_use_the_original_pot_for_both_subnet_eras() {
    for legacy in [false, true] {
        new_test_ext(1).execute_with(|| {
            let netuid = funded_market();
            if legacy {
                TaoInRefundDeploymentBlock::<Test>::put(1);
                SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(POOL));
            }
            let (owner, hotkey) = borrower(280);
            let (recipient, recipient_hotkey) = borrower(281);
            open(owner, hotkey, netuid, Side::Short);
            let loan = Positions::<Test>::get(owner, netuid).unwrap();
            assert_ok!(
                <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                    &owner,
                    &hotkey,
                    &recipient,
                    &recipient_hotkey,
                    netuid,
                    loan.principal.into(),
                    true,
                    false,
                )
            );
            pallet_lending::References::<Test>::mutate(netuid, |reference| {
                reference.as_mut().unwrap().price = substrate_fixed::types::U64F64::from_num(0.01);
            });
            SubnetFastMovingPrice::<Test>::insert(
                netuid,
                substrate_fixed::types::U64F64::from_num(0.02),
            );
            assert_ok!(SubtensorModule::do_dissolve_network(netuid));
            let trigger = System::block_number();
            System::set_block_number(trigger + 10 * LendingInterestPeriod::get());
            assert!(Lending::settle_shorts(netuid, &mut meter()));
            assert_eq!(
                Positions::<Test>::get(owner, netuid).unwrap().collateral,
                loan.collateral
            );
            let pot = SubnetTAO::<Test>::get(netuid).to_u64();
            let recipient_before = Balances::free_balance(recipient).to_u64();
            let owner_before = Balances::free_balance(owner).to_u64();
            let mut status = run_destroy_alpha_get_total_and_settle(netuid);
            let n = status.subnet_total_alpha_value.unwrap();
            assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 1);
            assert_eq!(
                n,
                u128::from(loan.principal)
                    + u128::from(SubnetProtocolAlpha::<Test>::get(netuid).to_u64())
                    + if legacy {
                        0
                    } else {
                        u128::from(SubnetAlphaIn::<Test>::get(netuid).to_u64())
                    },
            );
            let ordinary = Balances::free_balance(recipient).to_u64() - recipient_before;
            assert_eq!(
                ordinary,
                (u128::from(loan.principal) * u128::from(pot) / n) as u64
            );
            assert!(ordinary <= loan.collateral / 4);
            assert_eq!(Vaults::<Test>::get(netuid).unwrap().available_tao, 0);
            assert!(Positions::<Test>::contains_key(owner, netuid));
            let funded_base = (u128::from(loan.principal) * u128::from(pot)).div_ceil(n) as u64;
            let owed = funded_base.max(pot.min(funded_base + 1));
            let recovered = owed.min(loan.collateral);
            assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
            assert_eq!(
                SubnetTAO::<Test>::get(netuid).to_u64(),
                pot,
                "principal recovery cannot fund the ordinary redemption pot"
            );
            assert_eq!(
                Balances::free_balance(owner).to_u64(),
                owner_before + loan.collateral - recovered
            );
            let vault = Vaults::<Test>::get(netuid).unwrap();
            assert_eq!(vault.available_tao, recovered);
            assert_eq!(vault.outstanding_alpha, 0);
            assert_eq!(vault.lost_alpha, 0);
            status.set_phase(
                crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesAlpha,
            );
            assert!(
                SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter(), &mut status)
                    .0
            );
            assert_eq!(
                Balances::free_balance(Lending::recovery_account()).to_u64(),
                recovered
            );
            assert!(!DissolutionEligibleAlphaRows::<Test>::contains_key(netuid));
            assert_eq!(stake(&recipient, &recipient_hotkey, netuid), 0);
        });
    }
}

#[test]
fn split_alpha_holder_rounding_cannot_exceed_the_terminal_funded_allowance() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        TaoInRefundDeploymentBlock::<Test>::put(1);
        let (owner, hotkey) = borrower(300);
        // High collateral safely admits a legacy loan close to its guaranteed
        // protocol denominator, making split-row rounding visible at a dust pot.
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::from(10 * UNIT));
        let collateral = 4 * POOL;
        add_balance_to_coldkey_account(&owner, collateral.into());
        let quote = Lending::quote_open(netuid, Side::Short, collateral).unwrap();
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner),
            netuid,
            Side::Short,
            collateral,
            hotkey,
            quote.principal,
            quote.opening_value,
        ));
        let loan = Positions::<Test>::get(owner, netuid).unwrap();
        let piece = loan.principal / 3;
        let mut loan_holders = Vec::new();
        for i in 0..3 {
            let (recipient, _) = borrower(301 + i);
            let amount = if i == 2 {
                loan.principal - 2 * piece
            } else {
                piece
            };
            assert_ok!(
                <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                    &owner,
                    &hotkey,
                    &recipient,
                    &hotkey,
                    netuid,
                    amount.into(),
                    true,
                    false,
                )
            );
            loan_holders.push(recipient);
        }
        // All rows share one hotkey/page. Each borrowed row has a strictly larger
        // remainder than the seven other rows, so all three win a rounding atom.
        for i in 0..7 {
            let (holder, _) = borrower(310 + i);
            let alpha = AlphaBalance::from(piece - 1);
            SubtensorModule::resolve_to_alpha_out(SubtensorModule::mint_alpha(netuid, alpha));
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &holder, netuid, alpha,
            );
        }
        pallet_lending::References::<Test>::mutate(netuid, |reference| {
            reference.as_mut().unwrap().price = substrate_fixed::types::U64F64::from_bits(1);
        });
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let subnet = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let cash = Balances::free_balance(subnet).to_u64();
        assert_ok!(SubtensorModule::transfer_tao(
            &subnet,
            &owner,
            (cash - 4).into()
        ));
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(4));
        let before: Vec<_> = loan_holders
            .iter()
            .map(|holder| Balances::free_balance(holder).to_u64())
            .collect();
        let status = run_destroy_alpha_get_total_and_settle(netuid);
        let n = status.subnet_total_alpha_value.unwrap();
        let base = (u128::from(loan.principal) * 4).div_ceil(n) as u64;
        assert_eq!(base, 1);
        let paid: u64 = loan_holders
            .iter()
            .zip(before)
            .map(|(holder, balance)| Balances::free_balance(holder).to_u64() - balance)
            .sum();
        assert_eq!(
            paid, 3,
            "a position-sized ceil alone misses split-row rounding"
        );
        assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 10);
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().available_tao, 4);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().lost_alpha, 0);
    });
}

#[test]
fn alpha_borrower_can_sell_and_repledge_the_real_sale_cash_into_the_same_loan() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(330);
        open(owner, hotkey, netuid, Side::Short);
        let first = Positions::<Test>::get(owner, netuid).unwrap();
        let cash = <SubtensorModule as OrderSwapInterface<U256>>::sell_alpha(
            &owner,
            &hotkey,
            netuid,
            first.principal.into(),
            TaoBalance::ZERO,
            true,
        )
        .unwrap()
        .to_u64();
        assert!(cash > 0);
        assert_eq!(stake(&owner, &hotkey, netuid), 0);
        let added = grow(owner, hotkey, netuid, Side::Short, cash);
        let merged = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(merged.principal, first.principal + added.principal);
        assert_eq!(merged.collateral, first.collateral + cash);
        assert_eq!(merged.proceeds, 0);
        assert_eq!(stake(&owner, &hotkey, netuid), added.principal);
        assert_eq!(
            Balances::free_balance(Lending::position_account(&owner, netuid)).to_u64(),
            merged.collateral
        );
        assert_single_position(owner, hotkey, netuid);
    });
}

#[test]
fn eligible_redemption_rows_resume_without_double_counting_and_basis_uses_the_completed_scan() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        for id in 340..343 {
            let (owner, hotkey) = borrower(id);
            buy(&owner, &hotkey, netuid, 100 * UNIT);
        }
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        // A new denominator scan owns its initialization; stale generation
        // bookkeeping cannot survive its first processed page.
        DissolutionEligibleAlphaRows::<Test>::insert(netuid, 99);
        let mut status = dissolve_cleanup_status(netuid);
        let mut cursor = None;
        let mut done = false;
        for expected in 1..=3 {
            let mut page = WeightMeter::with_limit(
                <Test as frame_system::Config>::DbWeight::get().reads_writes(2, 1),
            );
            (done, cursor) = SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut page,
                cursor,
                &mut status,
            );
            assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), expected);
            if done {
                break;
            }
        }
        if !done {
            (done, cursor) = SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut meter(),
                cursor,
                &mut status,
            );
        }
        assert!(done);
        assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 3);
        let completed = status.subnet_total_alpha_value;
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut meter(),
                cursor,
                &mut status,
            )
            .0
        );
        assert_eq!(status.subnet_total_alpha_value, completed);
        assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 3);
        freeze_basis(netuid, &status);
        assert_eq!(
            pallet_lending::RedemptionBases::<Test>::get(netuid),
            Some((
                SubnetTAO::<Test>::get(netuid).to_u64(),
                completed.unwrap(),
                3,
            ))
        );
    });
}

#[test]
fn dissolution_uses_the_two_ema_marks_and_ignores_a_current_spot_pump() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::fast_alpha_price(netuid),
            None
        );
        SubnetFastMovingPrice::<Test>::insert(netuid, substrate_fixed::types::U64F64::from_num(0));
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::fast_alpha_price(netuid),
            None
        );
        let (owner, hotkey) = borrower(350);
        buy(&owner, &hotkey, netuid, 1_900 * UNIT);
        let slow = substrate_fixed::types::U64F64::from_num(1.01);
        let fast = substrate_fixed::types::U64F64::from_num(1.005);
        assert!(<Test as Config>::SwapInterface::current_alpha_price(netuid) > slow);
        pallet_lending::References::<Test>::mutate(netuid, |reference| {
            reference.as_mut().unwrap().price = slow;
        });
        SubnetFastMovingPrice::<Test>::insert(netuid, fast);
        assert_eq!(
            <SubtensorModule as LendingPoolInterface<U256>>::fast_alpha_price(netuid),
            Some(fast)
        );
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert_eq!(
            pallet_lending::Dissolutions::<Test>::get(netuid)
                .unwrap()
                .price,
            slow
        );
    });
}

#[test]
fn free_alpha_delivery_rejects_an_unknown_hotkey_without_consuming_custody() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, _) = borrower(360);
        let unknown = U256::from(999_999);
        assert!(!SubtensorModule::hotkey_account_exists(&unknown));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid,
                Side::Short,
                COLLATERAL,
                unknown,
                0,
                0
            ),
            Error::<Test>::HotKeyAccountNotExists,
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn a_public_alpha_donation_to_a_short_escrow_is_refunded_after_terminal_debt() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(370);
        let (donor, donated_hotkey) = borrower(371);
        let donation = buy(&donor, &donated_hotkey, netuid, 50 * UNIT);
        open(owner, hotkey, netuid, Side::Short);
        let loan = Positions::<Test>::get(owner, netuid).unwrap();
        let escrow = Lending::position_account(&owner, netuid);
        assert_ok!(SubtensorModule::transfer_stake(
            RuntimeOrigin::signed(donor),
            escrow,
            donated_hotkey,
            netuid,
            netuid,
            donation,
        ));
        assert!(stake(&escrow, &donated_hotkey, netuid) > 0);
        assert_eq!(Positions::<Test>::get(owner, netuid).unwrap().proceeds, 0);
        SubnetFastMovingPrice::<Test>::insert(netuid, substrate_fixed::types::U64F64::from_num(2));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let mut status = run_destroy_alpha_get_total_and_settle(netuid);
        let funded = Positions::<Test>::get(owner, netuid).unwrap().proceeds;
        assert_eq!(
            Balances::free_balance(escrow).to_u64(),
            loan.collateral + funded
        );
        assert!(funded > 0);
        assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 2);
        let original_pot = SubnetTAO::<Test>::get(netuid);
        let before = Balances::free_balance(owner).to_u64();
        let owed = 2 * loan.principal;
        assert!(owed < loan.collateral);
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        assert_eq!(
            Balances::free_balance(owner).to_u64(),
            before + loan.collateral - owed + funded
        );
        assert_eq!(Balances::free_balance(escrow), TaoBalance::ZERO);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().available_tao, owed);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().lost_alpha, 0);
        assert_eq!(SubnetTAO::<Test>::get(netuid), original_pot);
        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesAlpha);
        assert!(
            SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter(), &mut status).0
        );
        assert_eq!(stake(&escrow, &donated_hotkey, netuid), 0);
        assert_eq!(
            Balances::free_balance(Lending::recovery_account()).to_u64(),
            owed
        );
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn completed_denominator_scan_retries_the_basis_hook_before_any_ordinary_payout() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(380);
        open(owner, hotkey, netuid, Side::Short);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let mut status = dissolve_cleanup_status(netuid);
        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue);
        // The first pass finishes one hotkey; the second reaches the scan end
        // but has no weight for the lending hook. Both must retain the cursor.
        for _ in 0..2 {
            let mut page = WeightMeter::with_limit(
                <Test as frame_system::Config>::DbWeight::get().reads_writes(2, 1),
            );
            assert!(!SubtensorModule::clean_up_data_for_one_dissolved_network(&mut page, &mut status).0);
            assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 1);
            assert_eq!(status.phase, crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue);
            assert!(status.last_key.is_some());
            assert!(!pallet_lending::RedemptionBases::<Test>::contains_key(netuid));
        }
        let completed = status.subnet_total_alpha_value.unwrap();
        let wallet_before = Balances::free_balance(owner);
        let mut page = WeightMeter::with_limit(
            <Test as frame_system::Config>::DbWeight::get().reads_writes(5, 4),
        );
        assert!(!SubtensorModule::clean_up_data_for_one_dissolved_network(&mut page, &mut status).0);
        assert_eq!(DissolutionEligibleAlphaRows::<Test>::get(netuid), 1);
        assert_eq!(status.subnet_total_alpha_value, Some(completed));
        assert_eq!(status.phase, crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesSettleStakes);
        assert_eq!(pallet_lending::RedemptionBases::<Test>::get(netuid), Some((
            SubnetTAO::<Test>::get(netuid).to_u64(), completed, 1,
        )));
        assert_eq!(Balances::free_balance(owner), wallet_before);
        assert!(SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter(), &mut status).0);
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert!(!DissolutionEligibleAlphaRows::<Test>::contains_key(netuid));
    });
}

#[test]
fn wallet_alpha_repayment_closes_short_without_an_amm_trade() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(101);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Short);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let user_alpha = stake(&owner, &hotkey, netuid);
        let pool = (
            SubnetAlphaIn::<Test>::get(netuid),
            SubnetTAO::<Test>::get(netuid),
        );
        let quote = Lending::quote_close(&owner, netuid, true).unwrap();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            true,
            quote.payment,
            quote.refund
        ));
        assert_eq!(
            stake(&owner, &hotkey, netuid),
            user_alpha - position.principal
        );
        assert_eq!(
            (
                SubnetAlphaIn::<Test>::get(netuid),
                SubnetTAO::<Test>::get(netuid)
            ),
            pool
        );
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().outstanding_alpha, 0);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn wallet_alpha_repayment_below_spot_minimum_preserves_unrelated_stake_locks() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(125);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Short);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let opening_price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        let opening_value = opening_price
            .saturating_mul(substrate_fixed::types::U64F64::from_num(position.principal))
            .to_num::<u64>();
        assert!(opening_value >= DefaultMinStake::<Test>::get().to_u64());
        let user_alpha = stake(&owner, &hotkey, netuid);
        assert!(user_alpha > position.principal);
        let escrow = Lending::position_account(&owner, netuid);
        let reserve = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let reserve_alpha = stake(&reserve, &custody, netuid);
        let vault_before = Vaults::<Test>::get(netuid).unwrap();

        // Model a price crash with fresh fixture reserves, retaining all real
        // alpha shares and transferring the removed backing TAO out of the pot.
        let alpha_in = SubnetAlphaIn::<Test>::get(netuid);
        let crashed_tao = TaoBalance::from(
            (u128::from(DefaultMinStake::<Test>::get().to_u64()) * u128::from(alpha_in.to_u64())
                / (u128::from(position.principal) * 10)) as u64,
        );
        assert!(crashed_tao > TaoBalance::ZERO);
        let subnet = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let backed_tao = SubtensorModule::get_coldkey_balance(&subnet);
        assert!(backed_tao > crashed_tao);
        assert_ok!(SubtensorModule::transfer_tao(
            &subnet,
            &U256::from(999),
            backed_tao.saturating_sub(crashed_tao)
        ));
        setup_reserves(netuid, crashed_tao, alpha_in);
        <Test as Config>::SwapInterface::init_swap(netuid, None);
        assert!(pallet_subtensor_swap::SwapSuperellipse::<Test>::contains_key(netuid));
        TotalStake::<Test>::put(crashed_tao);
        let crashed_price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        let current_value = crashed_price
            .saturating_mul(substrate_fixed::types::U64F64::from_num(position.principal))
            .to_num::<u64>();
        assert!(crashed_price < opening_price);
        assert!(current_value > 0);
        assert!(current_value < DefaultMinStake::<Test>::get().to_u64());
        let quote = Lending::quote_close(&owner, netuid, true).unwrap();
        assert_eq!(quote.payment, position.principal);
        assert_eq!(quote.refund, position.collateral + position.proceeds);
        assert_noop!(
            <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                &owner,
                &hotkey,
                &reserve,
                &custody,
                netuid,
                position.principal.into(),
                true,
                false
            ),
            Error::<Test>::AmountTooLow
        );

        // Fixed-alpha repayment still respects unrelated registration collateral.
        let protected = user_alpha - position.principal + 1;
        MinerCollateral::<Test>::insert(
            (netuid, hotkey, owner),
            MinerCollateralState {
                locked: protected.into(),
                drain_ratio: substrate_fixed::types::U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid, owner, AlphaBalance::from(protected));
        assert_noop!(
            Lending::close(
                RuntimeOrigin::signed(owner),
                netuid,
                true,
                quote.payment,
                quote.refund
            ),
            Error::<Test>::StakeUnavailable
        );
        MinerCollateral::<Test>::remove((netuid, hotkey, owner));
        ColdkeyMinerCollateral::<Test>::remove(netuid, owner);
        assert_ok!(SubtensorModule::do_lock_stake(
            &owner,
            netuid,
            &hotkey,
            protected.into()
        ));
        assert_noop!(
            Lending::close(
                RuntimeOrigin::signed(owner),
                netuid,
                true,
                quote.payment,
                quote.refund
            ),
            Error::<Test>::StakeUnavailable
        );
        // Leave exactly the fixed principal available, keeping the rest locked.
        SubtensorModule::force_reduce_lock(&owner, netuid, AlphaBalance::from(1));
        let remaining_lock = SubtensorModule::get_current_locked(&owner, netuid);
        assert_eq!(remaining_lock.to_u64(), user_alpha - position.principal);
        let balance = SubtensorModule::get_coldkey_balance(&owner).to_u64();
        let pool = (
            SubnetAlphaIn::<Test>::get(netuid),
            SubnetTAO::<Test>::get(netuid),
        );
        let issuance = TotalIssuance::<Test>::get();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            true,
            quote.payment,
            quote.refund
        ));
        assert_eq!(
            stake(&owner, &hotkey, netuid),
            user_alpha - position.principal
        );
        assert_eq!(
            stake(&reserve, &custody, netuid),
            reserve_alpha + position.principal
        );
        assert_eq!(
            SubtensorModule::get_current_locked(&owner, netuid),
            remaining_lock
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            balance + quote.refund
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow),
            TaoBalance::ZERO
        );
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert!(!pallet_lending::OpenByNetuid::<Test>::contains_key(
            netuid, owner
        ));
        assert!(!pallet_lending::EscrowOwner::<Test>::contains_key(
            netuid, escrow
        ));
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(
            vault.available_alpha,
            vault_before.available_alpha + position.principal
        );
        assert_eq!(
            (
                SubnetAlphaIn::<Test>::get(netuid),
                SubnetTAO::<Test>::get(netuid)
            ),
            pool
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn long_receives_free_tao_and_repays_fixed_principal_for_alpha() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(102);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        let before_alpha = stake(&owner, &hotkey, netuid);
        let before_tao = SubtensorModule::get_coldkey_balance(&owner).to_u64();
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
        open(owner, hotkey, netuid, Side::Long);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let escrow = Lending::position_account(&owner, netuid);
        let custody = Lending::custody_hotkey().unwrap();
        assert_eq!(stake(&owner, &hotkey, netuid), before_alpha - COLLATERAL);
        assert_eq!(stake(&escrow, &custody, netuid), COLLATERAL);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            before_tao + position.principal
        );
        assert_eq!(
            Vaults::<Test>::get(netuid).unwrap().available_tao,
            before_vault.available_tao - position.principal
        );
        let quote = Lending::quote_close(&owner, netuid, false).unwrap();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            false,
            quote.payment,
            quote.refund
        ));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            before_tao
        );
        assert_eq!(stake(&owner, &hotkey, netuid), before_alpha);
        assert_eq!(stake(&escrow, &custody, netuid), 0);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap(), before_vault);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn failed_long_cash_transfer_rolls_back_real_alpha_share_transfer() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(103);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        let vault = Lending::reserve_account(netuid);
        let available = SubtensorModule::get_coldkey_balance(&vault);
        assert_ok!(SubtensorModule::transfer_tao(
            &vault,
            &U256::from(999),
            available
        ));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid,
                Side::Long,
                COLLATERAL,
                hotkey,
                0,
                0,
            )
            .is_err()
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn failed_close_refund_rolls_back_interest_in_currency_and_alpha_shares() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        for (id, side) in [(104, Side::Short), (105, Side::Long)] {
            let (owner, hotkey) = borrower(id);
            if side == Side::Long {
                buy(&owner, &hotkey, netuid, 200 * UNIT);
            }
            open(owner, hotkey, netuid, side);
            System::set_block_number(System::block_number() + 100);
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert_noop!(
                Lending::close(
                    RuntimeOrigin::signed(owner),
                    netuid,
                    false,
                    u64::MAX,
                    u64::MAX
                ),
                pallet_lending::Error::<Test>::BelowMinimumRefund
            );
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        }
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn weekly_short_coupon_burns_tao_without_trading_or_replenishing_inventory() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(106);
        open(owner, hotkey, netuid, Side::Short);
        let before = Positions::<Test>::get(owner, netuid).unwrap();
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
        let burn = burn_account();
        let burn_before = Balances::free_balance(burn);
        let reserve = Lending::reserve_account(netuid);
        let reserve_tao = Balances::free_balance(reserve);
        let escrow = Lending::position_account(&owner, netuid);
        let escrow_tao = Balances::free_balance(escrow);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let pool = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
        );
        System::set_block_number(before.due);
        Lending::on_idle(
            System::block_number(),
            Weight::from_parts(u64::MAX, u64::MAX),
        );
        let after = Positions::<Test>::get(owner, netuid).unwrap();
        let paid = (u128::from(before.annual_interest) * u128::from(LendingInterestPeriod::get())
            / u128::from(LendingBlocksPerYear::get())) as u64;
        assert_eq!(after.principal, before.principal);
        assert_eq!(after.collateral, before.collateral - paid);
        assert_eq!(after.proceeds, before.proceeds);
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault, before_vault);
        assert_eq!(
            Balances::free_balance(burn),
            burn_before + TaoBalance::from(paid)
        );
        assert_eq!(Balances::free_balance(reserve), reserve_tao);
        assert_eq!(
            Balances::free_balance(escrow),
            escrow_tao - TaoBalance::from(paid)
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            ),
            pool
        );
        assert_eq!(
            stake(
                &Lending::reserve_account(netuid),
                &Lending::custody_hotkey().unwrap(),
                netuid
            ),
            vault.available_alpha
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn weekly_long_coupon_sells_real_alpha_and_burns_tao_without_replenishing_inventory() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(163);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let before = Positions::<Test>::get(owner, netuid).unwrap();
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
        let reserve = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let reserve_tao = Balances::free_balance(reserve);
        let reserve_alpha = stake(&reserve, &custody, netuid);
        let burn = burn_account();
        let burn_before = Balances::free_balance(burn);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let paid = (u128::from(before.annual_interest) * u128::from(LendingInterestPeriod::get())
            / u128::from(LendingBlocksPerYear::get())) as u64;
        let proceeds =
            <SubtensorModule as LendingPoolInterface<U256>>::quote_sell(netuid, paid.into())
                .unwrap();
        assert!(proceeds > TaoBalance::ZERO);
        let pool_tao = SubnetTAO::<Test>::get(netuid);
        let pool_alpha = SubnetAlphaIn::<Test>::get(netuid);

        System::set_block_number(before.due);
        Lending::on_idle(
            System::block_number(),
            Weight::from_parts(u64::MAX, u64::MAX),
        );
        let after = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(after.principal, before.principal);
        assert_eq!(after.collateral, before.collateral - paid);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap(), before_vault);
        assert_eq!(Balances::free_balance(burn), burn_before + proceeds);
        assert_eq!(Balances::free_balance(reserve), reserve_tao);
        assert_eq!(stake(&reserve, &custody, netuid), reserve_alpha);
        assert!(SubnetTAO::<Test>::get(netuid) < pool_tao);
        assert!(SubnetAlphaIn::<Test>::get(netuid) > pool_alpha);
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn failed_long_coupon_burn_rolls_back_swap_and_retries_backed_pending_alpha() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(164);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let before = Positions::<Test>::get(owner, netuid).unwrap();
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
        let reserve = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let reserve_tao = Balances::free_balance(reserve);
        let reserve_alpha = stake(&reserve, &custody, netuid);
        let burn = burn_account();
        assert_eq!(Balances::free_balance(burn), TaoBalance::ZERO);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let paid = (u128::from(before.annual_interest) * u128::from(LendingInterestPeriod::get())
            / u128::from(LendingBlocksPerYear::get())) as u64;
        let proceeds =
            <SubtensorModule as LendingPoolInterface<U256>>::quote_sell(netuid, paid.into())
                .unwrap();
        assert!(proceeds > TaoBalance::ZERO && proceeds < TaoBalance::from(UNIT));
        let pool = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
        );
        // The real alpha sale can execute, but its cash output cannot create an
        // empty burn account. The pending fee remains physically backed alpha.
        ExistentialDeposit::set(UNIT.into());
        System::set_block_number(before.due);
        Lending::on_idle(
            System::block_number(),
            Weight::from_parts(u64::MAX, u64::MAX),
        );
        let after = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(after.principal, before.principal);
        assert_eq!(after.collateral, before.collateral - paid);
        let pending = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(pending.pending_alpha, paid);
        assert_eq!(pending.available_alpha, before_vault.available_alpha);
        assert_eq!(pending.available_tao, before_vault.available_tao);
        assert_eq!(Balances::free_balance(burn), TaoBalance::ZERO);
        assert_eq!(Balances::free_balance(reserve), reserve_tao);
        assert_eq!(stake(&reserve, &custody, netuid), reserve_alpha + paid);
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            ),
            pool
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);

        ExistentialDeposit::set(1.into());
        Lending::on_idle(
            System::block_number(),
            Weight::from_parts(u64::MAX, u64::MAX),
        );
        assert_eq!(Vaults::<Test>::get(netuid).unwrap(), before_vault);
        assert_eq!(Balances::free_balance(burn), proceeds);
        assert_eq!(Balances::free_balance(reserve), reserve_tao);
        assert_eq!(stake(&reserve, &custody, netuid), reserve_alpha);
        assert_eq!(Positions::<Test>::get(owner, netuid).unwrap(), after);
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn collateral_exhaustion_forfeits_fixed_debt_and_cleans_custody() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (short_owner, short_hotkey) = borrower(112);
        let (long_owner, long_hotkey) = borrower(113);
        buy(&long_owner, &long_hotkey, netuid, 200 * UNIT);
        open(short_owner, short_hotkey, netuid, Side::Short);
        open(long_owner, long_hotkey, netuid, Side::Long);
        let short_principal = Positions::<Test>::get(short_owner, netuid)
            .unwrap()
            .principal;
        let long_principal = Positions::<Test>::get(long_owner, netuid)
            .unwrap()
            .principal;
        let issuance = TotalIssuance::<Test>::get();
        let exhaustion_blocks = [short_owner, long_owner]
            .into_iter()
            .map(|owner| {
                let position = Positions::<Test>::get(owner, netuid).unwrap();
                (u128::from(position.collateral) * u128::from(LendingBlocksPerYear::get()))
                    .div_ceil(u128::from(position.annual_interest)) as u64
            })
            .max()
            .unwrap();
        System::set_block_number(System::block_number() + exhaustion_blocks + 1);
        Lending::on_idle(
            System::block_number(),
            Weight::from_parts(u64::MAX, u64::MAX),
        );
        assert!(!Positions::<Test>::contains_key(short_owner, netuid));
        assert!(!Positions::<Test>::contains_key(long_owner, netuid));
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.outstanding_tao, 0);
        assert_eq!(vault.lost_alpha, short_principal);
        assert_eq!(vault.lost_tao, long_principal);
        assert_eq!(
            stake(&short_owner, &short_hotkey, netuid),
            short_principal,
            "forfeiture records credit loss without confiscating freely delivered alpha"
        );
        assert_eq!(pallet_lending::TotalPositions::<Test>::get(), 0);
        let custody = Lending::custody_hotkey().unwrap();
        for owner in [short_owner, long_owner] {
            let escrow = Lending::position_account(&owner, netuid);
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&escrow),
                TaoBalance::ZERO
            );
            assert_eq!(stake(&escrow, &custody, netuid), 0);
        }
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn deregistration_burns_only_funded_fees_and_preserves_principal_recovery() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (short_owner, short_hotkey) = borrower(165);
        let (long_owner, long_hotkey) = borrower(166);
        buy(&long_owner, &long_hotkey, netuid, 200 * UNIT);
        open(short_owner, short_hotkey, netuid, Side::Short);
        open(long_owner, long_hotkey, netuid, Side::Long);
        let short = Positions::<Test>::get(short_owner, netuid).unwrap();
        let long = Positions::<Test>::get(long_owner, netuid).unwrap();
        let vault_before = Vaults::<Test>::get(netuid).unwrap();
        let reserve = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let burn = burn_account();
        let before_burn = SubtensorModule::get_coldkey_balance(&burn).to_u64();
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let pool_tao = SubnetTAO::<Test>::get(netuid).to_u64();
        let pool_alpha = SubnetAlphaIn::<Test>::get(netuid).to_u64();
        let protocol_tao = <Test as Config>::SwapInterface::protocol_tao_reservoir(netuid).to_u64();
        let protocol_alpha =
            <Test as Config>::SwapInterface::protocol_alpha_reservoir(netuid).to_u64();
        let curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid);
        let coupon = |annual: u64| {
            (u128::from(annual) * u128::from(LendingInterestPeriod::get()))
                .div_ceil(u128::from(LendingBlocksPerYear::get())) as u64
        };
        let short_fee = coupon(short.annual_interest);
        let long_fee = coupon(long.annual_interest);
        System::set_block_number(short.due);
        SubnetFastMovingPrice::<Test>::insert(netuid, substrate_fixed::types::U64F64::from_num(2));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        let frozen = pallet_lending::Dissolutions::<Test>::get(netuid).unwrap();
        assert_eq!(frozen.price, substrate_fixed::types::U64F64::from_num(2));
        let short_recovery = frozen
            .price
            .checked_mul(substrate_fixed::types::U64F64::from_num(short.principal))
            .unwrap()
            .ceil()
            .to_num::<u64>();
        let mut staged = dissolve_cleanup_status(netuid);
        staged.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::LendingSettleShorts);
        let mut stage_budget = WeightMeter::with_limit(
            <Test as frame_system::Config>::DbWeight::get().reads_writes(6, 4),
        );
        SubtensorModule::clean_up_data_for_one_dissolved_network(&mut stage_budget, &mut staged);
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let returned = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(returned.available_tao, 0);
        assert_eq!(returned.available_alpha, 0);
        assert_eq!(returned.pending_tao, 0);
        assert_eq!(returned.pending_alpha, long_fee);
        assert_eq!(returned.outstanding_tao, long.principal);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&reserve),
            TaoBalance::ZERO
        );
        assert_eq!(stake(&reserve, &custody, netuid), long_fee);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&burn).to_u64(),
            before_burn + short_fee
        );
        assert_eq!(
            SubnetTAO::<Test>::get(netuid).to_u64(),
            pool_tao + protocol_tao + vault_before.available_tao
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).to_u64(),
            pool_alpha + protocol_alpha + vault_before.available_alpha
        );
        assert_eq!(
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            curve
        );
        assert_total_alpha_staked_invariant(netuid);

        // The fee stake participates in the same ordinary funded denominator as
        // the long collateral. Its receipt is burned; recovered loan principal is not.
        let denominator = SubnetAlphaIn::<Test>::get(netuid).to_u64() as u128
            + SubnetProtocolAlpha::<Test>::get(netuid).to_u64() as u128
            + TotalAlphaStaked::<Test>::get(netuid).to_u64() as u128;
        let pot = SubnetTAO::<Test>::get(netuid).to_u64();
        let fee_floor = (u128::from(long_fee) * u128::from(pot) / denominator) as u64;
        assert!(fee_floor > 0);
        let mut status = run_destroy_alpha_get_total_and_settle(netuid);
        assert_eq!(status.subnet_total_alpha_value, Some(denominator));
        let long_burn = System::events()
            .iter()
            .filter_map(|record| match &record.event {
                RuntimeEvent::Lending(pallet_lending::Event::InterestBurned {
                    netuid: subnet,
                    side: Side::Long,
                    tao,
                }) if *subnet == netuid => Some(*tao),
                _ => None,
            })
            .sum::<u64>();
        assert!((fee_floor..=fee_floor + 1).contains(&long_burn));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&burn).to_u64(),
            before_burn + short_fee + long_burn
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&reserve),
            TaoBalance::ZERO
        );
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_alpha, long_fee);
        let funded = Positions::<Test>::get(long_owner, netuid).unwrap().proceeds;
        assert!(funded > long.principal);
        let owner_before = SubtensorModule::get_coldkey_balance(&long_owner);
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&long_owner),
            owner_before + TaoBalance::from(funded - long.principal)
        );
        let recovered = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(recovered.available_tao, long.principal + short_recovery);
        assert_eq!(recovered.pending_alpha, 0);
        assert_eq!(recovered.pending_tao, 0);
        assert_eq!(recovered.outstanding_alpha, 0);
        assert_eq!(recovered.outstanding_tao, 0);
        assert_eq!(recovered.lost_tao, 0);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&reserve).to_u64(),
            long.principal + short_recovery
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);

        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesAlpha);
        assert!(
            SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter(), &mut status).0
        );
        assert!(!Vaults::<Test>::contains_key(netuid));
        assert!(!pallet_lending::Dissolutions::<Test>::contains_key(netuid));
        assert_eq!(pallet_lending::PositionCount::<Test>::get(netuid), 0);
        assert_eq!(pallet_lending::TotalPositions::<Test>::get(), 0);
        assert_eq!(stake(&reserve, &custody, netuid), 0);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&reserve),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&Lending::recovery_account()).to_u64(),
            long.principal + short_recovery
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&burn).to_u64(),
            before_burn + short_fee + long_burn
        );
        assert_eq!(TotalIssuance::<Test>::get(), Balances::total_issuance());
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn failed_terminal_fee_burn_keeps_available_inventory_until_a_safe_retry() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(167);
        open(owner, hotkey, netuid, Side::Short);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let before = Vaults::<Test>::get(netuid).unwrap();
        let reserve = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let pool = (
            SubnetTAO::<Test>::get(netuid),
            SubnetAlphaIn::<Test>::get(netuid),
        );
        let fee = (u128::from(position.annual_interest) * u128::from(LendingInterestPeriod::get()))
            .div_ceil(u128::from(LendingBlocksPerYear::get())) as u64;
        assert!(fee > 0 && fee < UNIT);
        System::set_block_number(position.due);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        let issuance = TotalIssuance::<Test>::get();
        // The fee can enter an existing vault, but cannot initialize the empty
        // canonical burn address. No available principal may be returned first.
        ExistentialDeposit::set(UNIT.into());
        assert!(!Lending::settle_shorts(netuid, &mut meter()));
        let pending = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(pending.available_alpha, before.available_alpha);
        assert_eq!(pending.available_tao, before.available_tao);
        assert_eq!(pending.pending_tao, fee);
        assert_eq!(pending.outstanding_alpha, position.principal);
        let frozen_position = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(frozen_position.collateral, position.collateral - fee);
        assert_eq!(frozen_position.principal, position.principal);
        assert!(
            !pallet_lending::Dissolutions::<Test>::get(netuid)
                .unwrap()
                .reserves_returned
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&reserve).to_u64(),
            pending.available_tao + fee
        );
        assert_eq!(stake(&reserve, &custody, netuid), pending.available_alpha);
        assert_eq!(
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid)
            ),
            pool
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&burn_account()),
            TaoBalance::ZERO
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);

        ExistentialDeposit::set(1.into());
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&burn_account()).to_u64(),
            fee
        );
        assert_eq!(
            SubnetTAO::<Test>::get(netuid),
            pool.0 + TaoBalance::from(pending.available_tao)
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            pool.1 + AlphaBalance::from(pending.available_alpha)
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&reserve),
            TaoBalance::ZERO
        );
        assert_eq!(stake(&reserve, &custody, netuid), 0);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_tao, 0);
        assert!(
            pallet_lending::Dissolutions::<Test>::get(netuid)
                .unwrap()
                .reserves_returned
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), issuance);
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn terminal_alpha_fee_burn_failure_rolls_back_the_whole_funded_payout_page() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(168);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        System::set_block_number(position.due);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let reserve = Lending::reserve_account(netuid);
        let pending = Vaults::<Test>::get(netuid).unwrap().pending_alpha;
        assert!(pending > 0);
        let mut status = dissolve_cleanup_status(netuid);
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut meter(),
                None,
                &mut status
            )
            .0
        );
        // A genuine one-rao donation must not be reaped by the subsequent fee
        // burn. Fund the canonical address separately, then raise ED to expose it.
        assert_ok!(SubtensorModule::transfer_tao(&owner, &reserve, 1.into()));
        assert_ok!(SubtensorModule::transfer_tao(
            &owner,
            &burn_account(),
            500.into()
        ));
        ExistentialDeposit::set(500.into());
        freeze_basis(netuid, &status);
        let before_status = status.clone();
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let (done, cursor) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
            netuid,
            &mut meter(),
            None,
            &mut status,
        );
        assert!(!done);
        assert!(cursor.is_none());
        assert_eq!(status, before_status);
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(SubtensorModule::get_coldkey_balance(&reserve), 1.into());
        assert_eq!(Positions::<Test>::get(owner, netuid).unwrap().proceeds, 0);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_alpha, pending);

        ExistentialDeposit::set(1.into());
        freeze_basis(netuid, &status);
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                netuid,
                &mut meter(),
                None,
                &mut status
            )
            .0
        );
        assert!(SubtensorModule::get_coldkey_balance(&burn_account()).to_u64() > 500);
        assert_eq!(SubtensorModule::get_coldkey_balance(&reserve), 1.into());
        assert!(Positions::<Test>::get(owner, netuid).unwrap().proceeds > 0);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().available_tao, 0);
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_alpha, pending);
        assert_eq!(TotalIssuance::<Test>::get(), Balances::total_issuance());
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn deregistration_funds_the_pot_before_valuing_long_collateral() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (short_owner, short_hotkey) = borrower(107);
        let (long_owner, long_hotkey) = borrower(108);
        buy(&long_owner, &long_hotkey, netuid, 200 * UNIT);
        open(short_owner, short_hotkey, netuid, Side::Short);
        open(long_owner, long_hotkey, netuid, Side::Long);
        let short = Positions::<Test>::get(short_owner, netuid).unwrap();
        let long = Positions::<Test>::get(long_owner, netuid).unwrap();
        let vault_before = Vaults::<Test>::get(netuid).unwrap();
        let tao_before = SubnetTAO::<Test>::get(netuid).to_u64();
        let alpha_before = SubnetAlphaIn::<Test>::get(netuid).to_u64();
        let protocol_tao = <Test as Config>::SwapInterface::protocol_tao_reservoir(netuid).to_u64();
        let protocol_alpha =
            <Test as Config>::SwapInterface::protocol_alpha_reservoir(netuid).to_u64();
        let curve = pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid);
        let long_balance = SubtensorModule::get_coldkey_balance(&long_owner).to_u64();
        SubnetFastMovingPrice::<Test>::insert(netuid, substrate_fixed::types::U64F64::from_num(2));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        let frozen = pallet_lending::Dissolutions::<Test>::get(netuid).unwrap();
        let short_owed = frozen
            .price
            .checked_mul(substrate_fixed::types::U64F64::from_num(short.principal))
            .unwrap()
            .ceil()
            .to_num::<u64>();
        let mut staged = dissolve_cleanup_status(netuid);
        staged.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::LendingSettleShorts);
        let mut stage_budget = WeightMeter::with_limit(<Test as frame_system::Config>::DbWeight::get().reads_writes(6, 4));
        SubtensorModule::clean_up_data_for_one_dissolved_network(&mut stage_budget, &mut staged);
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        assert!(Positions::<Test>::contains_key(short_owner, netuid));
        assert_eq!(
            SubnetTAO::<Test>::get(netuid).to_u64(),
            tao_before + protocol_tao + vault_before.available_tao
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid).to_u64(),
            alpha_before + protocol_alpha + vault_before.available_alpha
        );
        assert_eq!(
            pallet_subtensor_swap::SwapSuperellipse::<Test>::get(netuid),
            curve,
            "terminal settlement executes no AMM swaps"
        );
        assert_eq!(TotalStake::<Test>::get(), TaoBalance::ZERO);
        let denominator = SubnetAlphaIn::<Test>::get(netuid).to_u64() as u128
            + SubnetProtocolAlpha::<Test>::get(netuid).to_u64() as u128
            + TotalAlphaStaked::<Test>::get(netuid).to_u64() as u128;
        let pot = SubnetTAO::<Test>::get(netuid).to_u64();
        let mut status = dissolve_cleanup_status(netuid);
        let mut weights = meter();
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut weights,
                None,
                &mut status
            )
            .0
        );
        assert_eq!(status.subnet_total_alpha_value, Some(denominator));
        let held = stake(&Lending::position_account(&long_owner, netuid), &Lending::custody_hotkey().unwrap(), netuid);
        let minimum_paid = (u128::from(held) * u128::from(pot) / denominator) as u64;
        let unencumbered_alpha = stake(&long_owner, &long_hotkey, netuid);
        let minimum_ordinary = (u128::from(unencumbered_alpha) * u128::from(pot) / denominator) as u64;
        assert!(minimum_paid > long.principal);
        freeze_basis(netuid, &status);
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                netuid,
                &mut weights,
                None,
                &mut status
            )
            .0
        );
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        let refund = System::events().iter().find_map(|record| match &record.event {
            RuntimeEvent::Lending(pallet_lending::Event::DissolutionSettled { owner, tao_refund, .. }) if owner == &long_owner => Some(*tao_refund),
            _ => None,
        }).unwrap();
        assert!(
            (minimum_paid - long.principal..=minimum_paid + 1 - long.principal).contains(&refund),
            "funded long refund {refund}, floor payout {minimum_paid}, debt {}, actual alpha {held}, pot {pot}, denominator {denominator}", long.principal,
        );
        let ordinary_payout = SubtensorModule::get_coldkey_balance(&long_owner).to_u64() - long_balance - refund;
        assert!((minimum_ordinary..=minimum_ordinary + 1).contains(&ordinary_payout));
        assert!(!Positions::<Test>::contains_key(long_owner, netuid));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&Lending::position_account(&long_owner, netuid)),
            TaoBalance::ZERO
        );
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.outstanding_tao, 0);
        assert_eq!(vault.available_tao, long.principal + short_owed);
        assert_eq!(vault.lost_tao, 0);
        assert_ok!(Lending::finish_dissolution(netuid));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&Lending::recovery_account()).to_u64(),
            long.principal + short_owed
        );
        assert!(!Vaults::<Test>::contains_key(netuid));
    });
}

#[test]
fn deregistration_accumulates_donated_secondary_hotkey_payouts_before_retiring_a_long() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(123);
        let (donor, donated_hotkey) = borrower(124);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let escrow = Lending::position_account(&owner, netuid);
        let custody = Lending::custody_hotkey().unwrap();
        let donation = buy(&donor, &donated_hotkey, netuid, 100 * UNIT);
        assert_ne!(donated_hotkey, custody);
        assert_ok!(
            <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                &donor,
                &donated_hotkey,
                &escrow,
                &donated_hotkey,
                netuid,
                donation,
                true,
                false
            )
        );
        let held = stake(&escrow, &custody, netuid);
        let donated = stake(&escrow, &donated_hotkey, netuid);
        assert!(held > 0 && donated > 0);
        assert_total_alpha_staked_invariant(netuid);

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let mut status = dissolve_cleanup_status(netuid);
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut meter(),
                None,
                &mut status
            )
            .0
        );
        let denominator = status.subnet_total_alpha_value.unwrap();
        // Neither hotkey alone repays the debt, but both funded payouts together
        // cover it and leave a surplus. Remove real cash when reducing the pot.
        let target_redemption = position.principal + position.principal / 5;
        let pot = (u128::from(target_redemption) * denominator)
            .div_ceil(u128::from(held) + u128::from(donated)) as u64;
        let subnet = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let current = SubtensorModule::get_coldkey_balance(&subnet).to_u64();
        assert!(pot <= current);
        assert_ok!(SubtensorModule::transfer_tao(
            &subnet,
            &U256::from(999),
            (current - pot).into()
        ));
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(pot));
        let custody_floor = (u128::from(held) * u128::from(pot) / denominator) as u64;
        let donated_floor = (u128::from(donated) * u128::from(pot) / denominator) as u64;
        assert!(custody_floor + 1 < position.principal);
        assert!(donated_floor + 1 < position.principal);
        assert!(custody_floor + donated_floor > position.principal);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();

        // One hotkey per page prevents ordinary payout aggregation from hiding
        // a premature retirement on the first callback to this escrow coldkey.
        freeze_basis(netuid, &status);
        let mut cursor = None;
        let mut done = false;
        let mut funded_pages = 0;
        for _ in 0..16 {
            let before = SubtensorModule::get_coldkey_balance(&escrow).to_u64();
            let mut page =
                WeightMeter::with_limit(<Test as frame_system::Config>::DbWeight::get().reads(1));
            (done, cursor) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                netuid,
                &mut page,
                cursor,
                &mut status,
            );
            let funded = SubtensorModule::get_coldkey_balance(&escrow).to_u64();
            if funded > before {
                funded_pages += 1;
                assert!(funded - before < position.principal);
            }
            let pending = Positions::<Test>::get(owner, netuid).unwrap();
            assert_eq!(pending.proceeds, funded);
            assert_eq!(pending.principal, position.principal);
            assert_eq!(
                pallet_lending::EscrowOwner::<Test>::get(netuid, escrow),
                Some(owner)
            );
            if done {
                break;
            }
        }
        assert!(done, "ordinary stake payouts must finish across pages");
        assert_eq!(funded_pages, 2);
        let funded = SubtensorModule::get_coldkey_balance(&escrow).to_u64();
        assert!(
            (custody_floor + donated_floor..=custody_floor + donated_floor + 2).contains(&funded)
        );
        assert!(funded > position.principal);
        assert_eq!(
            Vaults::<Test>::get(netuid).unwrap().outstanding_tao,
            position.principal
        );
        assert!(!System::events().iter().any(|record| matches!(
            &record.event,
            RuntimeEvent::Lending(pallet_lending::Event::DissolutionSettled { owner: settled, .. })
                if settled == &owner
        )));
        let owner_before_settlement = SubtensorModule::get_coldkey_balance(&owner).to_u64();
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        let refund = funded - position.principal;
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            owner_before_settlement + refund
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow),
            TaoBalance::ZERO
        );
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.available_tao, position.principal);
        assert_eq!(vault.outstanding_tao, 0);
        assert_eq!(vault.lost_tao, 0);
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert!(!pallet_lending::OpenByNetuid::<Test>::contains_key(
            netuid, owner
        ));
        assert!(!pallet_lending::EscrowOwner::<Test>::contains_key(
            netuid, escrow
        ));
        assert!(!pallet_lending::Due::<Test>::contains_key(
            position.due,
            (owner, netuid)
        ));
        assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(hotkey), 0);
        assert_eq!(pallet_lending::PositionCount::<Test>::get(netuid), 0);
        assert_eq!(pallet_lending::TotalPositions::<Test>::get(), 0);
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        let settlements = System::events()
            .iter()
            .filter(|record| {
                record.event
                    == RuntimeEvent::Lending(pallet_lending::Event::DissolutionSettled {
                        owner,
                        netuid,
                        side: Side::Long,
                        principal_recovered: position.principal,
                        principal_lost: 0,
                        tao_refund: refund,
                    })
            })
            .count();
        assert_eq!(settlements, 1);

        // Resume the ordinary deregistration after long settlement, including
        // destruction of both alpha stake rows and final recovery transfer.
        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesAlpha);
        assert!(
            SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter(), &mut status).0
        );
        assert_eq!(stake(&escrow, &custody, netuid), 0);
        assert_eq!(stake(&escrow, &donated_hotkey, netuid), 0);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&Lending::reserve_account(netuid)),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&Lending::recovery_account()).to_u64(),
            position.principal
        );
        assert!(!Vaults::<Test>::contains_key(netuid));
        assert!(!pallet_lending::Dissolutions::<Test>::contains_key(netuid));
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
    });
}

#[test]
fn zero_funded_redemption_records_loss_without_minting_a_refund() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(109);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let principal = Positions::<Test>::get(owner, netuid).unwrap().principal;
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        // Model a genuinely empty funded payout pot, rather than a mark-price promise.
        let account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let pot = SubtensorModule::get_coldkey_balance(&account);
        assert_ok!(SubtensorModule::transfer_tao(
            &account,
            &U256::from(999),
            pot
        ));
        SubnetTAO::<Test>::insert(netuid, TaoBalance::ZERO);
        let balance = SubtensorModule::get_coldkey_balance(&owner);
        let issuance = TotalIssuance::<Test>::get();
        run_destroy_alpha_get_total_and_settle(netuid);
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        assert_eq!(SubtensorModule::get_coldkey_balance(&owner), balance);
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.lost_tao, principal);
        assert_eq!(vault.outstanding_tao, 0);
        assert!(!Positions::<Test>::contains_key(owner, netuid));
    });
}

#[test]
fn deregistration_collects_frozen_long_interest_after_the_network_is_removed() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(117);
        buy(&owner, &hotkey, netuid, 200 * UNIT);
        open(owner, hotkey, netuid, Side::Long);
        let before = Positions::<Test>::get(owner, netuid).unwrap();
        let custody = Lending::custody_hotkey().unwrap();
        let escrow = Lending::position_account(&owner, netuid);
        System::set_block_number(before.due);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(!SubtensorModule::if_subnet_exist(netuid));
        assert!(
            Lending::settle_shorts(netuid, &mut meter()),
            "terminal alpha custody transfers must work after removal"
        );
        let coupon = (u128::from(before.annual_interest) * u128::from(LendingInterestPeriod::get()))
            .div_ceil(u128::from(LendingBlocksPerYear::get())) as u64;
        let after = Positions::<Test>::get(owner, netuid).unwrap();
        assert_eq!(after.collateral, COLLATERAL - coupon);
        assert_eq!(stake(&escrow, &custody, netuid), after.collateral);
        System::set_block_number(System::block_number() + LendingInterestPeriod::get());
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        assert_eq!(
            Positions::<Test>::get(owner, netuid).unwrap().collateral,
            after.collateral,
            "terminal accrual freezes at the deregistration trigger"
        );
        assert_eq!(TotalStake::<Test>::get(), TaoBalance::ZERO);
    });
}

#[test]
fn outstanding_positions_block_key_moves() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(110);
        open(owner, hotkey, netuid, Side::Short);
        assert_noop!(
            SubtensorModule::do_swap_coldkey(&owner, &U256::from(111)),
            Error::<Test>::LendingPositionsOpen
        );
        assert_noop_ignore_postinfo!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(owner),
                &hotkey,
                &U256::from(11_111),
                Some(netuid),
                false
            ),
            Error::<Test>::LendingPositionsOpen
        );
        let quote = Lending::quote_close(&owner, netuid, false).unwrap();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            false,
            quote.payment,
            0
        ));
        assert!(!Positions::<Test>::contains_key(owner, netuid));
    });
}

#[test]
fn nominated_loan_sources_follow_actual_hotkey_stake_migration() {
    for keep_stake in [false, true] {
        new_test_ext(1).execute_with(|| {
            let netuid = funded_market();
            let (validator, validator_hotkey) = borrower(114);
            register_ok_neuron(netuid, validator_hotkey, validator, 0);
            let (owner, _) = borrower(115);
            System::set_block_number(System::block_number() + HotkeySwapOnSubnetInterval::get());
            buy(&owner, &validator_hotkey, netuid, 200 * UNIT);
            let original_alpha = stake(&owner, &validator_hotkey, netuid);
            open(owner, validator_hotkey, netuid, Side::Long);
            assert!(!Positions::<Test>::contains_prefix(validator));
            let new_hotkey = U256::from(11_116);
            assert_ok!(SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(validator),
                &validator_hotkey,
                &new_hotkey,
                Some(netuid),
                keep_stake
            ));
            let saved_hotkey = if keep_stake {
                validator_hotkey
            } else {
                new_hotkey
            };
            let other_hotkey = if keep_stake {
                new_hotkey
            } else {
                validator_hotkey
            };
            assert_eq!(
                Positions::<Test>::get(owner, netuid).unwrap().hotkey,
                saved_hotkey
            );
            assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(saved_hotkey), 1);
            assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(other_hotkey), 0);
            let quote = Lending::quote_close(&owner, netuid, false).unwrap();
            assert_ok!(Lending::close(
                RuntimeOrigin::signed(owner),
                netuid,
                false,
                quote.payment,
                quote.refund
            ));
            assert_eq!(pallet_lending::LoanHotkeys::<Test>::get(saved_hotkey), 0);
            assert_eq!(stake(&owner, &saved_hotkey, netuid), original_alpha);
            assert_total_alpha_staked_invariant(netuid);
        });
    }
}

#[test]
fn funded_basket_holding_converts_before_lending_inventory_returns() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        add_network(NetUid::ROOT, 360, 0);
        let (validator, hotkey) = borrower(118);
        let alpha = buy(&validator, &hotkey, netuid, 10 * UNIT);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        assert_ok!(<SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(&validator, &hotkey, &escrow, &hotkey, netuid, alpha, true, false));
        BasketShares::<Test>::insert(hotkey, alpha.to_u64());
        BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));
        let shares = BasketShares::<Test>::get(hotkey);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert_eq!(crate::subnets::dissolution::DissolveCleanupStatus::new(netuid).phase, crate::subnets::dissolution::DissolveCleanupPhase::SubnetBasketHoldingsToRoot);
        run_block_idle();
        assert_eq!(stake(&escrow, &hotkey, netuid), 0);
        assert!(stake(&escrow, &hotkey, NetUid::ROOT) > 0, "basket alpha must become funded root cash before returned vault balances change the terminal reserves");
        assert_eq!(BasketShares::<Test>::get(hotkey), shares);
        assert!(!Vaults::<Test>::contains_key(netuid));
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(NetUid::ROOT);
    });
}

#[test]
fn oversized_basket_holdings_receive_funded_root_payouts_under_their_original_hotkeys() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        add_network(NetUid::ROOT, 360, 0);
        let (first_owner, first_hotkey) = borrower(126);
        let (second_owner, second_hotkey) = borrower(127);
        let escrow = SubtensorModule::get_beta_escrow_account_id();
        let curve = pallet_subtensor_swap::Pallet::<Test>::superellipse(netuid).unwrap();
        let sell_cap = curve
            .max_sell_input(
                SubnetAlphaIn::<Test>::get(netuid).to_u64(),
                SubnetTAO::<Test>::get(netuid).to_u64(),
            )
            .unwrap();
        assert!(sell_cap > 0);
        let holdings = [
            (first_owner, first_hotkey, sell_cap * 2 + 1, 100, 11),
            (second_owner, second_hotkey, sell_cap * 3 + 2, 200, 17),
        ];
        for (owner, hotkey, held, shares, claimed) in holdings {
            let alpha = AlphaBalance::from(held);
            SubtensorModule::resolve_to_alpha_out(SubtensorModule::mint_alpha(netuid, alpha));
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &escrow, netuid, alpha,
            );
            BasketShares::<Test>::insert(hotkey, shares);
            BasketRate::<Test>::insert(hotkey, I96F32::from_num(1));
            BasketClaimed::<Test>::insert(hotkey, owner, claimed);
            assert_eq!(stake(&escrow, &hotkey, netuid), held);
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert!(SubtensorModule::swap_basket_alpha_for_tao_chunks(netuid, alpha).is_err());
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        }
        assert_total_alpha_staked_invariant(netuid);
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(
            SubtensorModule::convert_subnet_basket_holdings_to_root(netuid, &mut meter(), None).0
        );
        for (_, hotkey, held, _, _) in holdings {
            assert_eq!(stake(&escrow, &hotkey, netuid), held);
            assert_eq!(stake(&escrow, &hotkey, NetUid::ROOT), 0);
        }

        // Restore latent buffers and lending inventory through the production
        // stage, stopping before generic stake valuation and destruction.
        let mut status = dissolve_cleanup_status(netuid);
        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::LendingSettleShorts);
        let mut stage = WeightMeter::with_limit(
            <Test as frame_system::Config>::DbWeight::get().reads_writes(7, 5),
        );
        SubtensorModule::clean_up_data_for_one_dissolved_network(&mut stage, &mut status);
        assert!(
            pallet_lending::Dissolutions::<Test>::get(netuid)
                .unwrap()
                .reserves_returned
        );
        assert_eq!(TotalStake::<Test>::get(), TaoBalance::ZERO);
        let pot = SubnetTAO::<Test>::get(netuid).to_u64();
        assert_eq!(
            SubtensorModule::get_coldkey_balance(
                &SubtensorModule::get_subnet_account_id(netuid).unwrap()
            )
            .to_u64(),
            pot
        );
        assert!(
            SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                netuid,
                &mut meter(),
                None,
                &mut status
            )
            .0
        );
        let denominator = status.subnet_total_alpha_value.unwrap();
        let root = SubtensorModule::get_subnet_account_id(NetUid::ROOT).unwrap();
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        freeze_basis(netuid, &status);
        let mut cursor = None;
        let mut done = false;
        let mut expected_root = 0;
        let mut paid_hotkeys = Vec::new();
        for _ in 0..16 {
            let mut page =
                WeightMeter::with_limit(<Test as frame_system::Config>::DbWeight::get().reads(1));
            (done, cursor) = SubtensorModule::destroy_alpha_in_out_stakes_settle_stakes(
                netuid,
                &mut page,
                cursor,
                &mut status,
            );
            for (_, hotkey, held, _, _) in holdings {
                let credited = stake(&escrow, &hotkey, NetUid::ROOT);
                if credited > 0 && !paid_hotkeys.contains(&hotkey) {
                    let floor = (u128::from(held) * u128::from(pot) / denominator) as u64;
                    assert_eq!(credited, floor);
                    // The fund must never expose both the redeemed alpha and
                    // its new ROOT cash between ordinary cleanup pages.
                    assert_eq!(stake(&escrow, &hotkey, netuid), 0);
                    assert_eq!(
                        SubtensorModule::get_validator_basket_nav_tao(&hotkey).to_u64(),
                        credited
                    );
                    paid_hotkeys.push(hotkey);
                    expected_root += credited;
                }
            }
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&root).to_u64(),
                expected_root
            );
            assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT).to_u64(), expected_root);
            assert_eq!(TotalStake::<Test>::get().to_u64(), expected_root);
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&escrow),
                TaoBalance::ZERO
            );
            if done {
                break;
            }
        }
        assert!(done, "oversized basket holdings must not stall dissolution");
        assert_eq!(paid_hotkeys.len(), 2);
        assert_eq!(
            status.subnet_distributed_tao,
            Some(u128::from(expected_root))
        );
        assert_eq!(TotalIssuance::<Test>::get(), issuance);
        assert_eq!(Balances::total_issuance(), currency_issuance);
        for (owner, hotkey, _, shares, claimed) in holdings {
            assert_eq!(BasketShares::<Test>::get(hotkey), shares);
            assert_eq!(BasketRate::<Test>::get(hotkey), I96F32::from_num(1));
            assert_eq!(BasketClaimed::<Test>::get(hotkey, owner), claimed);
        }
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::AlphaInOutStakesAlpha);
        assert!(
            SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter(), &mut status).0
        );
        for (owner, hotkey, _, shares, claimed) in holdings {
            assert_eq!(stake(&escrow, &hotkey, netuid), 0);
            assert!(stake(&escrow, &hotkey, NetUid::ROOT) > 0);
            assert_eq!(BasketShares::<Test>::get(hotkey), shares);
            assert_eq!(BasketRate::<Test>::get(hotkey), I96F32::from_num(1));
            assert_eq!(BasketClaimed::<Test>::get(hotkey, owner), claimed);
        }
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&root).to_u64(),
            expected_root
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow),
            TaoBalance::ZERO
        );
        assert!(!Vaults::<Test>::contains_key(netuid));
        assert!(!pallet_lending::Dissolutions::<Test>::contains_key(netuid));
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
        assert_total_alpha_staked_invariant(NetUid::ROOT);
    });
}

#[test]
fn dissolution_restores_latent_reserves_only_after_the_last_amm_stage() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let price = <Test as Config>::SwapInterface::current_alpha_price(netuid);
        let before_tao = SubnetTAO::<Test>::get(netuid);
        let before_alpha = SubnetAlphaIn::<Test>::get(netuid);
        let vault = Vaults::<Test>::get(netuid).unwrap();
        let tao = TaoBalance::from(10 * UNIT);
        let alpha = AlphaBalance::from(20 * UNIT);
        pallet_subtensor_swap::BalancerTaoReservoir::<Test>::insert(netuid, tao);
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(netuid, alpha);
        add_balance_to_coldkey_account(
            &SubtensorModule::get_subnet_account_id(netuid).unwrap(),
            tao,
        );
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert_eq!(SubnetTAO::<Test>::get(netuid), before_tao);
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), before_alpha);
        assert_eq!(
            <Test as Config>::SwapInterface::current_alpha_price(netuid),
            price
        );
        assert_eq!(
            <Test as Config>::SwapInterface::protocol_tao_reservoir(netuid),
            tao
        );
        assert_eq!(
            <Test as Config>::SwapInterface::protocol_alpha_reservoir(netuid),
            alpha
        );
        let mut status = dissolve_cleanup_status(netuid);
        status.set_phase(crate::subnets::dissolution::DissolveCleanupPhase::LendingSettleShorts);
        let mut stage_budget = WeightMeter::with_limit(
            <Test as frame_system::Config>::DbWeight::get().reads_writes(4, 4),
        );
        SubtensorModule::clean_up_data_for_one_dissolved_network(&mut stage_budget, &mut status);
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        assert_eq!(
            SubnetTAO::<Test>::get(netuid),
            before_tao
                .saturating_add(tao)
                .saturating_add(vault.available_tao.into())
        );
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            before_alpha
                .saturating_add(alpha)
                .saturating_add(vault.available_alpha.into())
        );
        assert_eq!(
            <Test as Config>::SwapInterface::protocol_tao_reservoir(netuid),
            TaoBalance::ZERO
        );
        assert_eq!(
            <Test as Config>::SwapInterface::protocol_alpha_reservoir(netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(TotalStake::<Test>::get(), TaoBalance::ZERO);
    });
}

#[test]
fn funded_terminal_surplus_below_real_ed_does_not_recreate_a_reaped_owner() {
    new_test_ext(1).execute_with(|| {
        ExistentialDeposit::set(500.into());
        let netuid = funded_market();
        let (owner, hotkey) = borrower(120);
        let collateral = buy(&owner, &hotkey, netuid, 100 * UNIT).to_u64();
        let quote = Lending::quote_open(netuid, Side::Long, collateral).unwrap();
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner),
            netuid,
            Side::Long,
            collateral,
            hotkey,
            quote.principal,
            0,
        ));
        assert_eq!(stake(&owner, &hotkey, netuid), 0);
        let balance = SubtensorModule::get_coldkey_balance(&owner);
        assert_ok!(SubtensorModule::transfer_tao(
            &owner,
            &U256::from(999),
            balance
        ));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner),
            TaoBalance::ZERO
        );
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(Lending::settle_shorts(netuid, &mut meter()));
        let escrow = Lending::position_account(&owner, netuid);
        let held = stake(&escrow, &Lending::custody_hotkey().unwrap(), netuid);
        let denominator = SubnetAlphaIn::<Test>::get(netuid).to_u64() as u128
            + SubnetProtocolAlpha::<Test>::get(netuid).to_u64() as u128
            + TotalAlphaStaked::<Test>::get(netuid).to_u64() as u128;
        let target =
            (u128::from(quote.principal + 1) * denominator).div_ceil(u128::from(held)) as u64;
        let subnet = SubtensorModule::get_subnet_account_id(netuid).unwrap();
        let current = SubtensorModule::get_coldkey_balance(&subnet).to_u64();
        assert!(target <= current);
        assert_ok!(SubtensorModule::transfer_tao(
            &subnet,
            &U256::from(999),
            (current - target).into()
        ));
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(target));
        let status = run_destroy_alpha_get_total_and_settle(netuid);
        assert_eq!(
            status.subnet_distributed_tao,
            Some(u128::from(quote.principal + 1))
        );
        assert!(Lending::settle_remaining_longs(netuid, &mut meter()));
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow),
            TaoBalance::ZERO
        );
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.available_tao, quote.principal);
        assert_eq!(vault.lost_tao, 0);
        assert!(System::events().iter().any(|record| record.event
            == RuntimeEvent::Lending(pallet_lending::Event::DustForfeited {
                netuid,
                recipient: owner,
                tao: 1
            })));
        assert_ok!(Lending::finish_dissolution(netuid));
    });
}

#[test]
fn tiny_first_tao_coupon_into_an_alpha_only_vault_is_explicitly_recycled() {
    new_test_ext(1).execute_with(|| {
        ExistentialDeposit::set(500.into());
        let netuid = market(true);
        let (source, source_hotkey) = borrower(121);
        let alpha = buy(&source, &source_hotkey, netuid, 200 * UNIT);
        assert_ok!(Lending::initialize_custody());
        let vault = Lending::reserve_account(netuid);
        let custody = Lending::custody_hotkey().unwrap();
        assert_ok!(
            <SubtensorModule as OrderSwapInterface<U256>>::transfer_staked_alpha(
                &source,
                &source_hotkey,
                &vault,
                &custody,
                netuid,
                alpha,
                true,
                false
            )
        );
        assert_ok!(Lending::fund_reserves(
            netuid,
            TaoBalance::ZERO,
            alpha,
            substrate_fixed::types::U64F64::from_num(1),
            true
        ));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&vault),
            TaoBalance::ZERO
        );
        let (owner, hotkey) = borrower(122);
        let quote = Lending::quote_open(netuid, Side::Short, 10 * UNIT).unwrap();
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner),
            netuid,
            Side::Short,
            10 * UNIT,
            hotkey,
            quote.principal,
            0,
        ));
        let coupon = u128::from(quote.annual_interest)
            .div_ceil(u128::from(LendingBlocksPerYear::get())) as u64;
        assert!(coupon > 0 && coupon < 500);
        System::set_block_number(System::block_number() + 1);
        let issuance = TotalIssuance::<Test>::get();
        let currency_issuance = Balances::total_issuance();
        let close = Lending::quote_close(&owner, netuid, false).unwrap();
        assert_eq!(
            TotalIssuance::<Test>::get(),
            issuance,
            "quoting cannot recycle real assets"
        );
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            false,
            close.payment,
            close.refund
        ));
        assert!(!Positions::<Test>::contains_key(owner, netuid));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&vault),
            TaoBalance::ZERO
        );
        assert_eq!(Vaults::<Test>::get(netuid).unwrap().pending_tao, 0);
        assert_eq!(
            TotalIssuance::<Test>::get(),
            issuance.saturating_sub(coupon.into())
        );
        assert_eq!(
            Balances::total_issuance(),
            currency_issuance.saturating_sub(coupon.into())
        );
        assert!(System::events().iter().any(|record| record.event
            == RuntimeEvent::Lending(pallet_lending::Event::DustForfeited {
                netuid,
                recipient: vault,
                tao: coupon
            })));
        assert_total_alpha_staked_invariant(netuid);
        assert_live_stake_total();
    });
}
