#![allow(clippy::unwrap_used)]

use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_swap_interface::SwapHandler;

use super::mock::*;
use crate::staking::lock::LockState;
use crate::*;

#[test]
fn test_coldkey_swap_records_lineage() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let c0 = U256::from(1);
        let c1 = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&c0, &hotkey);
        add_balance_to_coldkey_account(&c0, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &c0,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_ok!(SubtensorModule::do_swap_coldkey(&c0, &c1));

        assert_eq!(ColdkeySuccessor::<Test>::get(c0), Some(c1));
        assert_eq!(SubtensorModule::coldkey_root(&c1), c0);
        assert!(SubtensorModule::same_coldkey_lineage(&c0, &c1));
        assert_eq!(SubtensorModule::coldkey_lineage_tip(&c0), c1);
    });
}

#[test]
fn test_coldkey_swap_lineage_chain_and_tip() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let c0 = U256::from(1);
        let c1 = U256::from(2);
        let c2 = U256::from(3);
        let hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&c0, &hotkey);
        add_balance_to_coldkey_account(&c0, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &c0,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_ok!(SubtensorModule::do_swap_coldkey(&c0, &c1));
        assert_ok!(SubtensorModule::do_swap_coldkey(&c1, &c2));

        assert_eq!(ColdkeySuccessor::<Test>::get(c0), Some(c1));
        assert_eq!(ColdkeySuccessor::<Test>::get(c1), Some(c2));
        assert_eq!(SubtensorModule::coldkey_root(&c2), c0);
        assert!(SubtensorModule::same_coldkey_lineage(&c0, &c2));
        assert_eq!(SubtensorModule::coldkey_lineage_tip(&c0), c2);
    });
}

#[test]
fn test_coldkey_lineage_reverse_swap_does_not_cycle() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let c0 = U256::from(1);
        let c1 = U256::from(2);
        let hotkey = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&c0, &hotkey);
        add_balance_to_coldkey_account(&c0, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &c0,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        assert_ok!(SubtensorModule::do_swap_coldkey(&c0, &c1));
        assert_eq!(ColdkeySuccessor::<Test>::get(c0), Some(c1));

        // c0 was killed; fund it again as a fresh destination for the reverse swap.
        add_balance_to_coldkey_account(&c0, ExistentialDeposit::get());
        assert_ok!(SubtensorModule::do_swap_coldkey(&c1, &c0));

        assert!(ColdkeySuccessor::<Test>::get(c0).is_none());
        assert_eq!(ColdkeySuccessor::<Test>::get(c1), Some(c0));
        assert_eq!(SubtensorModule::coldkey_lineage_tip(&c1), c0);
        assert_eq!(SubtensorModule::coldkey_lineage_tip(&c0), c0);
        assert!(SubtensorModule::same_coldkey_lineage(&c0, &c1));
    });
}

#[test]
// Merging an empty attacker source into an already-used victim must not
// overwrite ColdkeyRoot / successor edges — otherwise same_coldkey_lineage
// would become true without the victim's authorization.
fn test_merge_into_used_destination_preserves_destination_lineage() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let attacker = U256::from(1);
        let victim = U256::from(2);
        let victim_hot = U256::from(3);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        // Victim already in use (owns a staking hotkey) and accepts locked alpha
        // so an empty attacker source can merge.
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &victim,
            &victim_hot
        ));
        add_balance_to_coldkey_account(&victim, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &victim_hot,
            &victim,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();
        SubtensorModule::set_accept_locked_alpha(&victim, true);
        assert!(SubtensorModule::coldkey_already_in_use(&victim));

        // Empty attacker: funded only enough to exist; no staking associations.
        add_balance_to_coldkey_account(&attacker, ExistentialDeposit::get());
        assert!(!SubtensorModule::coldkey_already_in_use(&attacker));

        let victim_root_before = SubtensorModule::coldkey_root(&victim);
        assert_eq!(victim_root_before, victim);
        assert!(ColdkeyRoot::<Test>::get(victim).is_none());
        assert!(ColdkeySuccessor::<Test>::get(attacker).is_none());

        assert_ok!(SubtensorModule::do_swap_coldkey(&attacker, &victim));

        // Destination identity unchanged; no shared lineage asserted.
        assert!(ColdkeyRoot::<Test>::get(victim).is_none());
        assert_eq!(SubtensorModule::coldkey_root(&victim), victim_root_before);
        assert!(ColdkeySuccessor::<Test>::get(attacker).is_none());
        assert!(!SubtensorModule::same_coldkey_lineage(&attacker, &victim));
        assert_eq!(SubtensorModule::coldkey_lineage_tip(&victim), victim);
    });
}

/// A previously rotated destination with an established ColdkeyRoot but no
/// remaining staking associations must still keep its lineage (Auditor
/// fid:9411d39b).
#[test]
fn test_merge_preserves_lineage_on_rotated_destination_without_staking() {
    new_test_ext(1).execute_with(|| {
        let attacker = U256::from(1);
        let victim = U256::from(2);
        let prior_root = U256::from(3);

        // Victim looks "fresh" to staking association checks, but already has
        // a recorded swap lineage from a prior rotation.
        ColdkeyRoot::<Test>::insert(victim, prior_root);
        ColdkeySuccessor::<Test>::insert(prior_root, victim);
        assert!(!SubtensorModule::coldkey_already_in_use(&victim));
        assert_eq!(SubtensorModule::coldkey_root(&victim), prior_root);

        add_balance_to_coldkey_account(&attacker, ExistentialDeposit::get());
        assert_ok!(SubtensorModule::do_swap_coldkey(&attacker, &victim));

        assert_eq!(ColdkeyRoot::<Test>::get(victim), Some(prior_root));
        assert_eq!(SubtensorModule::coldkey_root(&victim), prior_root);
        assert!(ColdkeySuccessor::<Test>::get(attacker).is_none());
        assert!(!SubtensorModule::same_coldkey_lineage(&attacker, &victim));
    });
}

#[test]
fn test_coldkey_lineage_rolls_back_with_failed_swap() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let c0 = U256::from(1);
        let c1 = U256::from(2);
        let hotkey = U256::from(3);
        let blocked_hotkey = U256::from(4);
        let stake_amount = DefaultMinStake::<Test>::get().to_u64() * 10;

        let _ = SubtensorModule::create_account_if_non_existent(&c0, &hotkey);
        add_balance_to_coldkey_account(&c0, stake_amount.into());
        SubtensorModule::stake_into_subnet(
            &hotkey,
            &c0,
            netuid,
            stake_amount.into(),
            <Test as crate::Config>::SwapInterface::max_price(),
            false,
        )
        .unwrap();

        SubtensorModule::insert_lock_state(
            &c1,
            netuid,
            &blocked_hotkey,
            LockState {
                locked_mass: 1_000u64.into(),
                conviction: U64F64::from_num(0),
                last_update: SubtensorModule::get_current_block_as_u64(),
            },
        );

        assert_noop!(
            SubtensorModule::do_swap_coldkey(&c0, &c1),
            Error::<Test>::ActiveLockExists
        );
        assert!(ColdkeySuccessor::<Test>::get(c0).is_none());
        assert!(ColdkeyRoot::<Test>::get(c1).is_none());
    });
}
