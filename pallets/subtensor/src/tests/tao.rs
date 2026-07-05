#![allow(
    unused,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]

use super::mock_high_ed::*;
use crate::tests::mock_high_ed;
use crate::*;
use frame_support::{
    assert_noop, assert_ok,
    traits::{
        Imbalance,
        tokens::{Fortitude, Preservation, fungible::Inspect as _},
    },
};
use sp_core::U256;
use sp_runtime::traits::{AccountIdConversion, Zero};
use subtensor_runtime_common::TaoBalance;

const MAX_TAO_ISSUANCE: u64 = 21_000_000_000_000_000_u64;

/// Helper: balances-pallet total issuance.
fn balances_total_issuance() -> TaoBalance {
    <Test as Config>::Currency::total_issuance()
}

/// Helper: subtensor-pallet total issuance.
fn subtensor_total_issuance() -> TaoBalance {
    TotalIssuance::<Test>::get()
}

/// Helper: free/reducible balance view used by tao.rs.
fn reducible_balance(account: &U256) -> TaoBalance {
    SubtensorModule::get_coldkey_balance(account)
}

/// Helper: total balance as seen by the currency implementation.
fn total_balance(account: &U256) -> TaoBalance {
    Balances::total_balance(account)
}

// ----------------------------------------------------
// transfer_tao
// ----------------------------------------------------

#[test]
fn test_transfer_tao_normal_case() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let amount = TaoBalance::from(200);
        add_balance_to_coldkey_account(&origin, ExistentialDeposit::get() * 10.into() + amount);
        let origin_before = total_balance(&origin);
        let dest_before = total_balance(&dest);

        assert!(origin_before >= amount);

        assert_ok!(SubtensorModule::transfer_tao(&origin, &dest, amount));

        assert_eq!(total_balance(&origin), origin_before - amount);
        assert_eq!(total_balance(&dest), dest_before + amount);
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

#[test]
fn test_transfer_tao_zero_balance_zero_amount() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(10_001);
        let dest = U256::from(10_002);

        assert_eq!(total_balance(&origin), 0.into());
        assert_eq!(total_balance(&dest), 0.into());

        assert_ok!(SubtensorModule::transfer_tao(&origin, &dest, 0.into()));

        assert_eq!(total_balance(&origin), 0.into());
        assert_eq!(total_balance(&dest), 0.into());
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

#[test]
fn test_transfer_tao_zero_balance_non_zero_amount_fails() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(10_011);
        let dest = U256::from(10_012);

        assert_eq!(total_balance(&origin), 0.into());

        assert_noop!(
            SubtensorModule::transfer_tao(&origin, &dest, 1u64.into()),
            Error::<Test>::InsufficientBalance
        );

        assert_eq!(total_balance(&origin), 0.into());
        assert_eq!(total_balance(&dest), 0.into());
    });
}

#[test]
fn test_transfer_tao_amount_greater_than_transferrable_fails() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let max_transferrable = reducible_balance(&origin);
        let amount = max_transferrable + 1.into();

        assert_noop!(
            SubtensorModule::transfer_tao(&origin, &dest, amount.into()),
            Error::<Test>::InsufficientBalance
        );
    });
}

#[test]
fn test_transfer_tao_transfer_exactly_transferrable_succeeds() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let amount = reducible_balance(&origin);
        let origin_before = total_balance(&origin);
        let dest_before = total_balance(&dest);

        assert_ok!(SubtensorModule::transfer_tao(&origin, &dest, amount.into()));

        assert_eq!(total_balance(&origin), origin_before - amount);
        assert_eq!(total_balance(&dest), dest_before + amount);
    });
}

#[test]
fn test_transfer_tao_can_reap_origin_when_amount_brings_it_below_ed() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let ed = ExistentialDeposit::get();
        let amount = TaoBalance::from(100);
        let balance_origin = amount + ed - 1.into();
        let balance_dest = ed + 1234.into();
        add_balance_to_coldkey_account(&origin, balance_origin);
        add_balance_to_coldkey_account(&dest, balance_dest);

        assert_ok!(SubtensorModule::transfer_tao(&origin, &dest, amount));

        // With Preservation::Expendable, origin may be reaped.
        assert!(total_balance(&origin).is_zero());
        assert_eq!(total_balance(&dest), amount + balance_dest);

        // Issuance should not change on plain transfer.
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

#[test]
fn test_transfer_tao_to_self_is_ok_and_no_net_balance_change() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let before = total_balance(&who);
        let amount = reducible_balance(&who).min(10.into());

        assert_ok!(SubtensorModule::transfer_tao(&who, &who, amount));

        assert_eq!(total_balance(&who), before);
    });
}

// ----------------------------------------------------
// transfer_all_tao_and_kill
// ----------------------------------------------------

#[test]
fn test_transfer_all_tao_and_kill_normal_case() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let ed = ExistentialDeposit::get();
        let amount = TaoBalance::from(100);
        let balance_origin = amount + ed;
        add_balance_to_coldkey_account(&origin, balance_origin);

        let transferable = reducible_balance(&origin);
        let origin_before = total_balance(&origin);
        let dest_before = total_balance(&dest);

        assert!(!transferable.is_zero());

        assert_ok!(SubtensorModule::transfer_all_tao_and_kill(&origin, &dest));

        assert_eq!(total_balance(&dest), dest_before + transferable);
        assert_eq!(
            total_balance(&origin),
            origin_before.saturating_sub(transferable)
        );
        assert_eq!(reducible_balance(&origin), 0.into());
    });
}

#[test]
fn test_transfer_all_tao_and_kill_non_existing_origin_is_noop() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(20_001);
        let dest = U256::from(20_002);

        assert_eq!(total_balance(&origin), 0.into());
        assert_eq!(reducible_balance(&origin), 0.into());

        let dest_before = total_balance(&dest);

        assert_ok!(SubtensorModule::transfer_all_tao_and_kill(&origin, &dest));

        assert_eq!(total_balance(&origin), 0.into());
        assert_eq!(total_balance(&dest), dest_before);
    });
}

#[test]
fn test_transfer_all_tao_and_kill_preexisting_destination() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let amount_o = TaoBalance::from(200) + ExistentialDeposit::get();
        let amount_d = TaoBalance::from(1000) + ExistentialDeposit::get();
        add_balance_to_coldkey_account(&origin, amount_o);
        add_balance_to_coldkey_account(&dest, amount_d);

        let transferable = reducible_balance(&origin);
        let dest_before = total_balance(&dest);

        assert!(dest_before > 0.into());

        assert_ok!(SubtensorModule::transfer_all_tao_and_kill(&origin, &dest));

        assert_eq!(total_balance(&dest), dest_before + transferable);
        assert_eq!(reducible_balance(&origin), 0.into());
    });
}

#[test]
fn test_transfer_all_tao_and_kill_to_self_is_noop() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let before_total = total_balance(&who);
        let before_reducible = reducible_balance(&who);

        assert_ok!(SubtensorModule::transfer_all_tao_and_kill(&who, &who));

        assert_eq!(total_balance(&who), before_total);
        assert_eq!(reducible_balance(&who), before_reducible);
    });
}

// ----------------------------------------------------
// burn_tao
// ----------------------------------------------------

#[test]
fn test_burn_tao_increases_burn_address_balance() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let burn_address: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();

        let amount = reducible_balance(&coldkey).min(10.into());
        let burn_before = total_balance(&burn_address);
        let coldkey_before = total_balance(&coldkey);

        assert_ok!(SubtensorModule::burn_tao(&coldkey, amount));

        assert_eq!(total_balance(&burn_address), burn_before + amount);
        assert_eq!(total_balance(&coldkey), coldkey_before - amount);

        // burn_tao is just a transfer to burn address, not issuance reduction.
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

#[test]
fn test_burn_tao_zero_amount_is_ok() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let burn_address: U256 = <Test as Config>::BurnAccountId::get().into_account_truncating();

        let burn_before = total_balance(&burn_address);
        let coldkey_before = total_balance(&coldkey);

        assert_ok!(SubtensorModule::burn_tao(&coldkey, 0u64.into()));

        assert_eq!(total_balance(&burn_address), burn_before);
        assert_eq!(total_balance(&coldkey), coldkey_before);
    });
}

#[test]
fn test_burn_tao_insufficient_balance_fails() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(30_001);

        assert_noop!(
            SubtensorModule::burn_tao(&coldkey, 1u64.into()),
            Error::<Test>::InsufficientBalance
        );
    });
}

// ----------------------------------------------------
// recycle_tao / issuance consistency
// ----------------------------------------------------

#[test]
fn test_recycle_tao_reduces_both_balances_and_subtensor_total_issuance() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let max_preserving = SubtensorModule::get_coldkey_balance(&coldkey);
        let amount = max_preserving.min(10.into());

        let coldkey_before = total_balance(&coldkey);
        let balances_ti_before = balances_total_issuance();
        let subtensor_ti_before = subtensor_total_issuance();

        assert_ok!(SubtensorModule::recycle_tao(&coldkey, amount));

        assert_eq!(total_balance(&coldkey), coldkey_before - amount);

        // Balances-pallet withdraw burns supply.
        assert_eq!(balances_total_issuance(), balances_ti_before - amount);

        // Subtensor TI is reduced explicitly in recycle_tao.
        assert_eq!(subtensor_total_issuance(), subtensor_ti_before - amount);

        // End state still aligned.
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

#[test]
fn test_recycle_tao_amount_greater_than_max_preserving_fails() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let max_preserving: u64 = <Test as Config>::Currency::reducible_balance(
            &coldkey,
            frame_support::traits::tokens::Preservation::Preserve,
            frame_support::traits::tokens::Fortitude::Polite,
        )
        .into();

        let too_much = max_preserving.saturating_add(1);

        assert_noop!(
            SubtensorModule::recycle_tao(&coldkey, too_much.into()),
            Error::<Test>::InsufficientBalance
        );

        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

#[test]
fn test_recycle_tao_zero_amount_keeps_issuance_equal() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let balances_before = balances_total_issuance();
        let subtensor_before = subtensor_total_issuance();
        let balance_before = total_balance(&coldkey);

        assert_ok!(SubtensorModule::recycle_tao(&coldkey, 0u64.into()));

        assert_eq!(total_balance(&coldkey), balance_before);
        assert_eq!(balances_total_issuance(), balances_before);
        assert_eq!(subtensor_total_issuance(), subtensor_before);
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

/// This is the invariant you asked for in normal ED=1 test runtime:
/// plain transfers and burns should keep both issuance trackers aligned,
/// and recycle should reduce both by the same amount.
#[test]
fn test_total_issuance_subtensor_matches_balances_across_tao_operations() {
    new_test_ext(1).execute_with(|| {
        let a = U256::from(1);
        let b = U256::from(2);

        let ed = ExistentialDeposit::get();
        let balance = TaoBalance::from(1_000_000) + ed - 1.into();
        add_balance_to_coldkey_account(&a, balance);
        add_balance_to_coldkey_account(&b, balance);

        assert_eq!(balances_total_issuance(), subtensor_total_issuance());

        assert_ok!(SubtensorModule::transfer_tao(&a, &b, 1000.into()));
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());

        assert_ok!(SubtensorModule::burn_tao(&a, 1000.into()));
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());

        let max_preserving: u64 = <Test as Config>::Currency::reducible_balance(
            &a,
            frame_support::traits::tokens::Preservation::Preserve,
            frame_support::traits::tokens::Fortitude::Polite,
        )
        .into();
        let recycle_amount = max_preserving.min(1);
        assert_ok!(SubtensorModule::recycle_tao(&a, recycle_amount.into()));
        assert_eq!(balances_total_issuance(), subtensor_total_issuance());
    });
}

// ----------------------------------------------------
// mint_tao
// ----------------------------------------------------

/// This is expected to fail with the current implementation:
/// mint_tao issues into the balances pallet but does not update
/// SubtensorModule::TotalIssuance.
#[test]
fn test_mint_tao_increases_total_issuance_in_balances_and_subtensor() {
    new_test_ext(1).execute_with(|| {
        let amount = TaoBalance::from(123);

        let balances_before = balances_total_issuance();
        let subtensor_before = subtensor_total_issuance();

        let credit = SubtensorModule::mint_tao(amount);

        assert_eq!(credit.peek(), amount);

        // This one should pass.
        assert_eq!(balances_total_issuance(), balances_before + amount);

        // This one is expected to fail until mint_tao updates TotalIssuance::<T>.
        assert_eq!(subtensor_total_issuance(), subtensor_before + amount);
    });
}

#[test]
fn test_mint_tao_zero_amount() {
    new_test_ext(1).execute_with(|| {
        let balances_before = balances_total_issuance();
        let subtensor_before = subtensor_total_issuance();

        let credit = SubtensorModule::mint_tao(0u64.into());

        assert_eq!(u64::from(credit.peek()), 0);
        assert_eq!(balances_total_issuance(), balances_before);
        assert_eq!(subtensor_total_issuance(), subtensor_before);
    });
}

#[test]
fn test_mint_tao_respects_max_issuance_cap_in_balances() {
    new_test_ext(1).execute_with(|| {
        // We cannot directly force balances-pallet issuance above the cap in every mock,
        // but we *can* set subtensor's mirror and still verify that mint_tao uses the
        // balances-pallet total issuance as its source of truth.
        //
        // This test is mostly a guard that the returned credit is capped by
        // MAX_TAO_ISSUANCE - Currency::total_issuance().
        let balances_before = balances_total_issuance();
        let remaining = TaoBalance::from(MAX_TAO_ISSUANCE) - balances_before;
        let request = remaining + 1000.into();

        let credit = SubtensorModule::mint_tao(request.into());

        assert_eq!(credit.peek(), remaining);
        assert_eq!(balances_total_issuance(), MAX_TAO_ISSUANCE.into());
    });
}

#[test]
fn test_transfer_tao_reaps_origin() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);
        let dest = U256::from(2);

        let ed = ExistentialDeposit::get();
        let balance_origin = TaoBalance::from(3) + ed;
        let amount = TaoBalance::from(2) + ed;
        add_balance_to_coldkey_account(&origin, balance_origin);
        let subtensor_ti_before = subtensor_total_issuance();
        let balances_ti_before = balances_total_issuance();

        assert_ok!(SubtensorModule::transfer_tao(&origin, &dest, amount));

        let subtensor_ti_after = subtensor_total_issuance();
        let balances_ti_after = balances_total_issuance();

        assert_eq!(Balances::total_balance(&origin), 0.into());
        assert_eq!(Balances::total_balance(&dest), amount);
        assert_eq!(balances_ti_before - balances_ti_after, 1.into());
        assert_eq!(subtensor_ti_before - subtensor_ti_after, 1.into());
    });
}

#[test]
fn test_recycle_tao_cannot_cross_preserve_threshold_in_high_ed_runtime() {
    new_test_ext(1).execute_with(|| {
        let origin = U256::from(1);

        let max_preserving =
            Balances::reducible_balance(&origin, Preservation::Preserve, Fortitude::Polite);

        assert_noop!(
            SubtensorModule::recycle_tao(&origin, max_preserving + 1u64.into()),
            Error::<Test>::InsufficientBalance
        );
    });
}
