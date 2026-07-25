#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Green-path — lock queries.

use super::helpers::*;
use super::prelude::*;

// =========================================================================
// GROUP 2: Green-path — lock queries
// =========================================================================

#[test]
fn test_get_current_locked_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);
        assert_eq!(
            SubtensorModule::get_current_locked(&coldkey, netuid),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn test_get_conviction_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);
        assert_eq!(
            SubtensorModule::get_conviction(&coldkey, netuid),
            U64F64::from_num(0)
        );
    });
}

#[test]
fn test_get_coldkey_lock_rolls_forward() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            5000u64.into(),
        ));

        let initial_lock =
            SubtensorModule::get_coldkey_lock(&coldkey, netuid).expect("coldkey lock should exist");
        assert_eq!(initial_lock.conviction, U64F64::from_num(0));

        step_block(1000);

        let rolled_lock =
            SubtensorModule::get_coldkey_lock(&coldkey, netuid).expect("coldkey lock should exist");
        assert_eq!(rolled_lock.locked_mass, initial_lock.locked_mass);
        assert!(rolled_lock.conviction > initial_lock.conviction);
        assert_eq!(
            rolled_lock.last_update,
            SubtensorModule::get_current_block_as_u64()
        );
    });
}

#[test]
fn test_get_coldkey_lock_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = subtensor_runtime_common::NetUid::from(1);

        assert!(SubtensorModule::get_coldkey_lock(&coldkey, netuid).is_none());
    });
}

#[test]
fn test_available_to_unstake_no_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, total);
    });
}

#[test]
fn test_available_to_unstake_with_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let lock_amount = total / 2.into();
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, total - lock_amount);
    });
}

#[test]
fn test_available_to_unstake_fully_locked() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, total,
        ));

        let available = SubtensorModule::available_to_unstake(&coldkey, netuid);
        assert_eq!(available, AlphaBalance::ZERO);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_empty_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let result = SubtensorModule::get_stake_availability_for_coldkeys(Vec::new(), None);
        assert!(result.is_empty());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_empty_netuids() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(Vec::new()));
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&coldkey));
        assert!(result.get(&coldkey).unwrap().is_empty());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_filters_empty_rows() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));

        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&coldkey));
        assert!(result.get(&coldkey).unwrap().is_empty());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_stake_without_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));

        assert_eq!(result.len(), 1);
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();
        assert_eq!(availability.total(), total);
        assert_eq!(availability.locked(), AlphaBalance::ZERO);
        assert_eq!(availability.available(), total);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_partial_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let lock_amount = total / 2.into();

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();

        assert_eq!(availability.total(), total);
        assert_eq!(
            availability.locked(),
            SubtensorModule::get_current_locked(&coldkey, netuid)
        );
        assert_eq!(availability.available(), total - availability.locked());
    });
}

#[test]
fn test_stake_availability_for_coldkeys_fully_locked() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);

        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey, netuid, &hotkey, total,
        ));

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();

        assert_eq!(availability.total(), total);
        assert_eq!(availability.locked(), total);
        assert_eq!(availability.available(), AlphaBalance::ZERO);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_preserves_coldkey_grouping() {
    new_test_ext(1).execute_with(|| {
        let coldkey_a = U256::from(1);
        let hotkey_a = U256::from(2);
        let coldkey_b = U256::from(3);
        let hotkey_b = U256::from(4);
        let netuid_a = setup_subnet_with_stake(coldkey_a, hotkey_a, 100_000_000_000);
        let netuid_b = setup_subnet_with_stake(coldkey_b, hotkey_b, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey_a, coldkey_b],
            Some(vec![netuid_a, netuid_b]),
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result.get(&coldkey_a).unwrap().len(), 1);
        assert!(result.get(&coldkey_a).unwrap().contains_key(&netuid_a));
        assert_eq!(result.get(&coldkey_b).unwrap().len(), 1);
        assert!(result.get(&coldkey_b).unwrap().contains_key(&netuid_b));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_none_netuids_uses_all_subnets() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], None);

        assert_eq!(result.len(), 1);
        assert!(result.get(&coldkey).unwrap().contains_key(&netuid));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_one_coldkey_two_subnets() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);
        let netuid_a = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);
        let netuid_b = setup_subnet_with_stake(coldkey, hotkey_b, 100_000_000_000);
        let total_a = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid_a);
        let total_b = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid_b);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid_a, netuid_b]),
        );

        assert_eq!(result.len(), 1);
        let subnets = result.get(&coldkey).unwrap();
        assert_eq!(subnets.len(), 2);
        assert!(subnets.contains_key(&netuid_a));
        assert!(subnets.contains_key(&netuid_b));

        let row_a = subnets.get(&netuid_a).unwrap();
        assert_eq!(row_a.total(), total_a);
        assert_eq!(row_a.locked(), AlphaBalance::ZERO);
        assert_eq!(row_a.available(), total_a);

        let row_b = subnets.get(&netuid_b).unwrap();
        assert_eq!(row_b.total(), total_b);
        assert_eq!(row_b.locked(), AlphaBalance::ZERO);
        assert_eq!(row_b.available(), total_b);
    });
}

#[test]
fn test_stake_availability_for_coldkeys_filters_to_requested_netuid() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey_a = U256::from(2);
        let hotkey_b = U256::from(3);
        let netuid_a = setup_subnet_with_stake(coldkey, hotkey_a, 100_000_000_000);
        let netuid_b = setup_subnet_with_stake(coldkey, hotkey_b, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid_b]),
        );

        assert_eq!(result.len(), 1);
        let subnets = result.get(&coldkey).unwrap();
        assert_eq!(subnets.len(), 1);
        assert!(subnets.contains_key(&netuid_b));
        assert!(!subnets.contains_key(&netuid_a));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_dedups_netuids() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid, netuid]),
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result.get(&coldkey).unwrap().len(), 1);
        assert!(result.get(&coldkey).unwrap().contains_key(&netuid));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_skips_nonexistent_netuid() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let nonexistent = subtensor_runtime_common::NetUid::from(99);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![nonexistent]),
        );
        assert_eq!(result.len(), 1);
        assert!(result.get(&coldkey).unwrap().is_empty());

        // Mix real + fake requires at least two subnets on chain so len(requested) <= subnet_count.
        let subnet_owner_coldkey = U256::from(2001);
        let subnet_owner_hotkey = U256::from(2002);
        let _other_netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        let result = SubtensorModule::get_stake_availability_for_coldkeys(
            vec![coldkey],
            Some(vec![netuid, nonexistent]),
        );
        assert_eq!(result.len(), 1);
        let subnets = result.get(&coldkey).unwrap();
        assert_eq!(subnets.len(), 1);
        assert!(subnets.contains_key(&netuid));
        assert!(!subnets.contains_key(&nonexistent));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_rejects_oversized_netuid_list() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let subnet_count = SubtensorModule::get_all_subnet_netuids().len();
        let requested: Vec<subtensor_runtime_common::NetUid> = (0..=subnet_count as u16)
            .map(subtensor_runtime_common::NetUid::from)
            .collect();

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(requested));
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&coldkey));
        assert!(result.get(&coldkey).unwrap().is_empty());

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        assert_eq!(result.get(&coldkey).unwrap().len(), 1);
        assert!(result.get(&coldkey).unwrap().contains_key(&netuid));
    });
}

#[test]
fn test_stake_availability_for_coldkeys_uses_rolled_forward_lock() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = setup_subnet_with_stake(coldkey, hotkey, 100_000_000_000);
        let total = SubtensorModule::total_coldkey_alpha_on_subnet(&coldkey, netuid);
        let lock_amount = total / 2.into();

        DecayingLock::<Test>::remove(coldkey, netuid);
        assert_ok!(SubtensorModule::do_lock_stake(
            &coldkey,
            netuid,
            &hotkey,
            lock_amount,
        ));
        let raw_lock = Lock::<Test>::get((coldkey, netuid, hotkey)).unwrap();

        step_block(1000);

        let result =
            SubtensorModule::get_stake_availability_for_coldkeys(vec![coldkey], Some(vec![netuid]));
        let availability = result.get(&coldkey).unwrap().get(&netuid).unwrap();
        let rolled_locked = SubtensorModule::get_current_locked(&coldkey, netuid);

        assert!(rolled_locked < raw_lock.locked_mass);
        assert_eq!(availability.locked(), rolled_locked);
        assert_eq!(availability.available(), total - rolled_locked);
    });
}
