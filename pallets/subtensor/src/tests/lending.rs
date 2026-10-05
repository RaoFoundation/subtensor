#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use super::mock::*;
use crate::*;
use frame_support::{
    StorageDoubleMap as _, assert_noop, assert_ok, traits::Hooks, weights::WeightMeter,
};
use pallet_lending::{Positions, Side, Vaults};
use sp_core::U256;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::{OrderSwapInterface, SwapHandler};

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

#[test]
fn short_open_rejects_lower_proceeds_even_when_alpha_principal_is_unchanged() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (first, first_hotkey) = borrower(180);
        open(first, first_hotkey, netuid, Side::Short);
        let (owner, hotkey) = borrower(181);
        let quoted = Lending::quote_open(netuid, Side::Short, COLLATERAL).unwrap();

        // Another real sale moves spot below the historical reference. The
        // reference still caps alpha debt at the same quantity, while executable
        // TAO proceeds fall. A principal-only floor cannot protect this caller.
        let (other, other_hotkey) = borrower(182);
        open(other, other_hotkey, netuid, Side::Short);
        let current = Lending::quote_open(netuid, Side::Short, COLLATERAL).unwrap();
        assert_eq!(current.principal, quoted.principal);
        assert!(current.opening_value < quoted.opening_value);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid,
                Side::Short,
                COLLATERAL,
                hotkey,
                quoted.principal,
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
        assert_eq!(
            Positions::<Test>::get(owner, netuid).unwrap().proceeds,
            current.opening_value
        );
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
fn short_is_custodial_and_amm_close_restores_principal() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(100);
        let balance_before = SubtensorModule::get_coldkey_balance(&owner).to_u64();
        let alpha_before = Vaults::<Test>::get(netuid).unwrap().available_alpha;
        open(owner, hotkey, netuid, Side::Short);
        let position = Positions::<Test>::get(owner, netuid).unwrap();
        let escrow = Lending::position_account(&owner, netuid);
        let custody = Lending::custody_hotkey().unwrap();
        assert_eq!(stake(&owner, &hotkey, netuid), 0);
        assert_eq!(stake(&escrow, &custody, netuid), 0);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner).to_u64(),
            balance_before - COLLATERAL
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&escrow).to_u64(),
            COLLATERAL + position.proceeds
        );
        assert_eq!(
            Vaults::<Test>::get(netuid).unwrap().available_alpha,
            alpha_before - position.principal
        );
        let quote = Lending::quote_close(&owner, netuid, false).unwrap();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner),
            netuid,
            false,
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
        assert!(vault.available_alpha >= alpha_before);
        assert_eq!(
            stake(&Lending::reserve_account(netuid), &custody, netuid),
            vault.available_alpha
        );
        assert_live_stake_total();
        assert_total_alpha_staked_invariant(netuid);
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
fn weekly_short_coupon_is_bought_into_real_alpha_reserves() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(106);
        open(owner, hotkey, netuid, Side::Short);
        let before = Positions::<Test>::get(owner, netuid).unwrap();
        let before_vault = Vaults::<Test>::get(netuid).unwrap();
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
        let vault = Vaults::<Test>::get(netuid).unwrap();
        assert_eq!(vault.pending_tao, 0);
        assert!(vault.available_alpha > before_vault.available_alpha);
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
        System::set_block_number(System::block_number() + 5 * LendingBlocksPerYear::get());
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
        assert!(!Positions::<Test>::contains_key(short_owner, netuid));
        assert_eq!(
            SubnetTAO::<Test>::get(netuid).to_u64(),
            tao_before + protocol_tao + vault_before.available_tao + short_owed
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
        assert_eq!(vault.available_tao, long.principal);
        assert_eq!(vault.lost_tao, 0);
        assert_ok!(Lending::finish_dissolution(netuid));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&Lending::recovery_account()).to_u64(),
            long.principal
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
fn outstanding_positions_block_key_moves_and_curve_recalibration() {
    new_test_ext(1).execute_with(|| {
        let netuid = funded_market();
        let (owner, hotkey) = borrower(110);
        open(owner, hotkey, netuid, Side::Short);
        assert_noop!(
            SubtensorModule::sudo_set_pool_slippage(RuntimeOrigin::root(), netuid, 200),
            Error::<Test>::LendingPositionsOpen
        );
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
        assert_ok!(SubtensorModule::sudo_set_pool_slippage(
            RuntimeOrigin::root(),
            netuid,
            200
        ));
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
        let quote = Lending::quote_open(netuid, Side::Short, 5 * UNIT).unwrap();
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner),
            netuid,
            Side::Short,
            5 * UNIT,
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
