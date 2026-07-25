#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
//! Tests for share-pool lazy migration and Alpha data-ops used by staking.

use approx::assert_abs_diff_eq;
use share_pool::SafeFloat;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::AlphaBalance;

use super::super::mock::*;
use crate::*;

// cargo test --package pallet-subtensor --lib -- tests::staking::sharepool::test_lazy_sharepool_migration_get_stake_reads_from_deprecated_alpha_map --exact --nocapture
#[test]
fn test_lazy_sharepool_migration_get_stake_reads_from_deprecated_alpha_map() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated Alpha map
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::from(stake)
        );
    });
}

#[test]
fn test_lazy_sharepool_migration_get_stake_reads_from_alpha_v2_map() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to AlphaV2 map
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), SafeFloat::from(1_u64));
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, SafeFloat::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::from(stake)
        );
    });
}

#[test]
fn test_lazy_sharepool_migration_get_stake_reads_from_cross_alpha_maps() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to Alpha map
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        // but total shares are in TotalHotkeySharesV2 map (already migrated)
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, SafeFloat::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid),
            AlphaBalance::from(stake)
        );
    });
}

#[test]
fn test_lazy_sharepool_migration_staking_causes_migration() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated Alpha map
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Stake more via stake_into_subnet
        increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake.into(), netuid);

        // Verify that deprecated v1 map values are gone
        assert!(Alpha::<Test>::try_get((&hotkey, &coldkey, netuid)).is_err());
        assert!(TotalHotkeyShares::<Test>::try_get(hotkey, netuid).is_err());

        // Verify that v2 map values are present
        let migrated_share = AlphaV2::<Test>::get((&hotkey, &coldkey, netuid));
        let migrated_denominator = TotalHotkeySharesV2::<Test>::get(hotkey, netuid);

        assert_abs_diff_eq!(
            f64::from((migrated_share.div(&migrated_denominator)).unwrap()),
            1.0,
            epsilon = 0.000000000000001
        );
    });
}

#[test]
fn test_sharepool_dataops_get_value_v1() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated Alpha map
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let actual_value = share_pool.get_value(&coldkey);

        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_get_value_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to AlphaV2 map
        let share = sf_from_u64(1_u64);
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), share.clone());
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let actual_value = share_pool.get_value(&coldkey);

        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_get_value_mixed_v1_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated Alpha map and new THS v2 map
        let share = sf_from_u64(1_u64);
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let actual_value = share_pool.get_value(&coldkey);

        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_get_value_mixed_v2_v1() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to new AlphaV2 map and deprecated THS map
        let share = sf_from_u64(1_u64);
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), share);
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let actual_value = share_pool.get_value(&coldkey);

        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_get_value_from_shares_v1() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated THS map
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value_from_shares
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let current_share = SafeFloat::from(U64F64::from(1_u64));
        let actual_value = share_pool.get_value_from_shares(current_share);

        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_get_value_from_shares_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to new THS v2 map
        let share = sf_from_u64(1_u64);
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value_from_shares
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let current_share = SafeFloat::from(U64F64::from(1_u64));
        let actual_value = share_pool.get_value_from_shares(current_share);

        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_update_value_for_all() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to new AlphaV2 map
        let share = sf_from_u64(1_u64);
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), share.clone());
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and call update_value_for_all
        let mut share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        share_pool.update_value_for_all(stake as i64);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, stake * 2);

        share_pool.update_value_for_all(-(stake as i64));
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, stake);
    });
}

#[test]
fn test_sharepool_dataops_update_value_for_one_v1_with_migration() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated Alpha and THS maps
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and call update_value_for_one
        let mut share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        share_pool.update_value_for_one(&coldkey, stake as i64);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, stake * 2);

        // Verify deletion from deprecated
        assert!(!Alpha::<Test>::contains_key((hotkey, coldkey, netuid)));
        assert!(!TotalHotkeyShares::<Test>::contains_key(hotkey, netuid));
    });
}

#[test]
fn test_sharepool_dataops_update_value_for_one_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to new AlphaV2 and THS maps
        let share = sf_from_u64(1_u64);
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), share.clone());
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and call update_value_for_one
        let mut share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        share_pool.update_value_for_one(&coldkey, stake as i64);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, stake * 2);
    });
}

#[test]
fn test_sharepool_dataops_update_value_for_one_mixed_v1_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to deprecated Alpha and new THS v2 maps
        let share = sf_from_u64(1_u64);
        Alpha::<Test>::insert((hotkey, coldkey, netuid), U64F64::from(1_u64));
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and call update_value_for_one
        let mut share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        share_pool.update_value_for_one(&coldkey, stake as i64);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, stake * 2);

        // Verify deletion from deprecated
        assert!(!Alpha::<Test>::contains_key((hotkey, coldkey, netuid)));
    });
}

#[test]
fn test_sharepool_dataops_update_value_for_one_mixed_v2_v1() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        let stake = 200_000_u64;

        // add stake to new AlphaV2 and deprecated THS maps
        let share = sf_from_u64(1_u64);
        AlphaV2::<Test>::insert((hotkey, coldkey, netuid), share);
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and call update_value_for_one
        let mut share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        share_pool.update_value_for_one(&coldkey, stake as i64);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, stake * 2);

        // Verify deletion from deprecated
        assert!(!TotalHotkeyShares::<Test>::contains_key(hotkey, netuid));
    });
}

#[test]
fn test_sharepool_dataops_get_value_returns_zero_on_non_existing_v1() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        let stake = 200_000_u64;

        // add to deprecated THS map, but no value in Alpha map
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, 0_u64);
    });
}

#[test]
fn test_sharepool_dataops_get_value_returns_zero_on_non_existing_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        let stake = 200_000_u64;

        // add to THSV2 map, but no value in AlphaV2 map
        let share = sf_from_u64(1_u64);
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let actual_value = share_pool.get_value(&coldkey);
        assert_eq!(actual_value, 0_u64);
    });
}

#[test]
fn test_sharepool_dataops_try_get_value_returns_err_on_non_existing_v1() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        let stake = 200_000_u64;

        // add to deprecated THS map, but no value in Alpha map
        TotalHotkeyShares::<Test>::insert(hotkey, netuid, U64F64::from(1_u64));
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let maybe_actual_value = share_pool.try_get_value(&coldkey);
        assert!(maybe_actual_value.is_err());
    });
}

#[test]
fn test_sharepool_dataops_try_get_value_returns_err_on_non_existing_v2() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);

        let netuid = add_dynamic_network(&hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        let stake = 200_000_u64;

        // add to THSV2 map, but no value in AlphaV2 map
        let share = sf_from_u64(1_u64);
        TotalHotkeySharesV2::<Test>::insert(hotkey, netuid, share);
        TotalHotkeyAlpha::<Test>::insert(hotkey, netuid, AlphaBalance::from(stake));

        // Get real share pool and read get_value
        let share_pool = SubtensorModule::get_alpha_share_pool(hotkey, netuid);
        let maybe_actual_value = share_pool.try_get_value(&coldkey);
        assert!(maybe_actual_value.is_err());
    });
}
