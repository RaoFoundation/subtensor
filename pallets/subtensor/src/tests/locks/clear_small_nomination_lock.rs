#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Clear small nomination checks lock.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 16: Clear small nomination checks lock
// =========================================================================

#[test]
fn test_clear_small_nomination_checks_lock() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(100);
        let owner_hotkey = U256::from(101);
        let netuid = setup_subnet_with_stake(owner_coldkey, owner_hotkey, 100_000_000_000);

        // Set up a nominator (different coldkey, does NOT own the hotkey)
        let nominator = U256::from(200);
        add_balance_to_coldkey_account(&nominator, 100_000_000_000u64.into());
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &nominator,
            &owner_hotkey
        ));
        SubtensorModule::stake_into_subnet(
            &owner_hotkey,
            &nominator,
            netuid,
            50_000_000_000u64.into(),
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        let nominator_alpha = get_alpha(&owner_hotkey, &nominator, netuid);
        assert!(nominator_alpha > AlphaBalance::ZERO);

        // Nominator locks their full stake
        let nominator_total = SubtensorModule::total_coldkey_alpha_on_subnet(&nominator, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &nominator,
            netuid,
            &owner_hotkey,
            nominator_total,
        ));

        // Set a high nominator min stake so the current stake is "small"
        SubtensorModule::set_nominator_min_required_stake(u64::MAX);

        // clear_small_nomination removes the lock and unstakes alpha
        SubtensorModule::clear_small_nomination_if_required(&owner_hotkey, &nominator, netuid);

        // Nominator alpha has been removed despite lock
        let nominator_alpha_after = get_alpha(&owner_hotkey, &nominator, netuid);
        assert_eq!(nominator_alpha_after, AlphaBalance::ZERO);

        // Lock entry doesn't exist anymore
        assert!(
            Lock::<Test>::iter_prefix((nominator, netuid))
                .next()
                .is_none()
        );

        // Hotkey lock should also be removed
        let hotkey_lock = HotkeyLock::<Test>::get(netuid, owner_hotkey);
        assert!(hotkey_lock.is_none());
    });
}

#[test]
// If one coldkey has a large nomination on one hotkey and a tiny nomination on another,
// clearing the tiny nomination should reduce the lock state only by that tiny alpha amount.
fn test_clear_small_nomination_reduces_only_tiny_amount_from_lock_state() {
    new_test_ext(1).execute_with(|| {
        // Large stake, subnet owner, and large lock receiver
        let coldkey_large = U256::from(100);
        let hotkey_large = U256::from(101);
        let netuid = setup_subnet_with_stake(coldkey_large, hotkey_large, 100_000_000_000);

        let coldkey_tiny = U256::from(102);
        let hotkey_tiny = U256::from(103);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey_tiny,
            &hotkey_tiny
        ));

        // Coldkey that is going to stake and lock
        let nominator = U256::from(200);
        let large_tao = TaoBalance::from(50_000_000_000u64);
        let tiny_tao = TaoBalance::from(1_000_000u64);
        add_balance_to_coldkey_account(&nominator, large_tao + tiny_tao);

        // Create one large nomination and one tiny nomination on the same subnet.
        SubtensorModule::stake_into_subnet(
            &hotkey_large,
            &nominator,
            netuid,
            large_tao,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        SubtensorModule::stake_into_subnet(
            &hotkey_tiny,
            &nominator,
            netuid,
            tiny_tao,
            <Test as Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        DecayingLock::<Test>::insert(nominator, netuid, false);

        let large_alpha_before = get_alpha(&hotkey_large, &nominator, netuid);
        let tiny_alpha_before = get_alpha(&hotkey_tiny, &nominator, netuid);
        assert!(large_alpha_before > tiny_alpha_before);

        // Lock against the large nomination hotkey and seed non-zero unlocked_mass + conviction
        // so we can verify each field is reduced only by the tiny nomination's alpha amount.
        let total_before = SubtensorModule::total_coldkey_alpha_on_subnet(&nominator, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &nominator,
            netuid,
            &hotkey_large,
            total_before,
        ));

        let conviction_before = U64F64::from_num(tiny_alpha_before.to_u64() + 2_000);
        let last_update = SubtensorModule::get_current_block_as_u64();
        Lock::<Test>::insert(
            (nominator, netuid, hotkey_large),
            LockState {
                locked_mass: total_before,
                conviction: conviction_before,
                last_update,
            },
        );
        HotkeyLock::<Test>::insert(
            netuid,
            hotkey_large,
            LockState {
                locked_mass: total_before,
                conviction: conviction_before,
                last_update,
            },
        );

        // Force the tiny nomination to qualify as "small" and clear only that nomination.
        SubtensorModule::set_nominator_min_required_stake(u64::MAX);
        SubtensorModule::clear_small_nomination_if_required(&hotkey_tiny, &nominator, netuid);

        // The large nomination stays, the tiny one is removed.
        let large_alpha_after = get_alpha(&hotkey_large, &nominator, netuid);
        let tiny_alpha_after = get_alpha(&hotkey_tiny, &nominator, netuid);
        assert_eq!(large_alpha_after, large_alpha_before);
        assert!(!large_alpha_after.is_zero());
        assert_eq!(tiny_alpha_after, AlphaBalance::ZERO);

        // Only the tiny alpha amount should be shaved off the coldkey lock state.
        // Conviction is reduced proportionally
        let lock_after = Lock::<Test>::get((nominator, netuid, hotkey_large)).unwrap();
        assert!(!lock_after.locked_mass.is_zero());
        assert_eq!(lock_after.locked_mass, total_before - tiny_alpha_before);
        assert!(lock_after.conviction != U64F64::from_num(0));
        let expected_conviction = conviction_before.to_num::<f64>()
            * (1. - u64::from(tiny_alpha_before) as f64 / u64::from(total_before) as f64);
        assert_abs_diff_eq!(
            lock_after.conviction.to_num::<f64>(),
            expected_conviction,
            epsilon = expected_conviction / 1000000.
        );

        // The aggregate hotkey lock on the locked hotkey should also only shrink by the tiny amount.
        let hotkey_lock_after = HotkeyLock::<Test>::get(netuid, hotkey_large).unwrap();
        assert_eq!(
            hotkey_lock_after.locked_mass,
            total_before - tiny_alpha_before
        );
        assert_abs_diff_eq!(
            hotkey_lock_after.conviction.to_num::<f64>(),
            expected_conviction,
            epsilon = expected_conviction / 1000000.
        );
    });
}
