#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! subnet volume, first emission block, subtoken, zero hotkey alpha.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_subnet_volume() {
    new_test_ext(1).execute_with(|| {
        // Setup initial state
        let netuid_1 = NetUid::from(1);
        add_network(netuid_1, 1, 0);

        // SubnetValue for netuid 1 key
        let old_key: [u8; 34] = hex_literal::hex!(
            "658faa385070e074c85bf6b568cf05553c3226e141696000b4b239c65bc2b2b40100"
        );

        // Old value in u64 format
        let old_value: u64 = 123_456_789_000_u64;
        put::<u64>(&old_key, &old_value); // Store as u64

        // Ensure it is stored as `u64`
        assert_eq!(get::<u64>(&old_key), Some(old_value));

        // Run migration
        crate::migrations::migrate_subnet_volume::migrate_subnet_volume::<Test>();

        // Verify the value is now stored as `u128`
        let new_value: Option<u128> = get(&old_key);
        let new_value_as_subnet_volume = SubnetVolume::<Test>::get(netuid_1);
        assert_eq!(new_value, Some(old_value as u128));
        assert_eq!(new_value_as_subnet_volume, old_value as u128);

        // Ensure migration does not break when running twice
        let weight_second_run =
            crate::migrations::migrate_subnet_volume::migrate_subnet_volume::<Test>();

        // Verify the value is still stored as `u128`
        let new_value: Option<u128> = get(&old_key);
        assert_eq!(new_value, Some(old_value as u128));
    });
}

#[test]
fn test_migrate_set_first_emission_block_number() {
    new_test_ext(1).execute_with(|| {
    let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
    let block_number = 100;
    for netuid in netuids.iter() {
        add_network(*netuid, 1, 0);
    }
    run_to_block(block_number);
    let weight = crate::migrations::migrate_set_first_emission_block_number::migrate_set_first_emission_block_number::<Test>();

    let expected_weight: Weight = <Test as Config>::DbWeight::get().reads(3) + <Test as Config>::DbWeight::get().writes(netuids.len() as u64);
    assert_eq!(weight, expected_weight);

    assert_eq!(FirstEmissionBlockNumber::<Test>::get(NetUid::ROOT), None);
    for netuid in netuids.iter() {
        assert_eq!(FirstEmissionBlockNumber::<Test>::get(netuid), Some(block_number));
    }
});
}

#[test]
fn test_migrate_set_subtoken_enable() {
    new_test_ext(1).execute_with(|| {
        let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
        let block_number = 100;
        for netuid in netuids.iter() {
            add_network(*netuid, 1, 0);
        }

        let new_netuid = NetUid::from(4);
        add_network_without_emission_block(new_netuid, 1, 0);

        let weight =
            crate::migrations::migrate_set_subtoken_enabled::migrate_set_subtoken_enabled::<Test>();

        let expected_weight: Weight = <Test as Config>::DbWeight::get().reads(1)
            + <Test as Config>::DbWeight::get().writes(netuids.len() as u64 + 2);
        assert_eq!(weight, expected_weight);

        for netuid in netuids.iter() {
            assert!(SubtokenEnabled::<Test>::get(netuid));
        }
        assert!(!SubtokenEnabled::<Test>::get(new_netuid));
    });
}

#[test]
fn test_migrate_remove_zero_total_hotkey_alpha() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_remove_zero_total_hotkey_alpha";
        let netuid = NetUid::from(1u16);

        let hotkey_zero = U256::from(100u64);
        let hotkey_nonzero = U256::from(101u64);

        // Insert one zero-alpha entry and one non-zero entry
        TotalHotkeyAlpha::<Test>::insert(hotkey_zero, netuid, AlphaBalance::ZERO);
        TotalHotkeyAlpha::<Test>::insert(hotkey_nonzero, netuid, AlphaBalance::from(123));

        assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey_zero, netuid), AlphaBalance::ZERO);
        assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey_nonzero, netuid), AlphaBalance::from(123));

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet."
        );

        let weight = crate::migrations::migrate_remove_zero_total_hotkey_alpha::migrate_remove_zero_total_hotkey_alpha::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run."
        );

        assert!(
            !TotalHotkeyAlpha::<Test>::contains_key(hotkey_zero, netuid),
            "Zero-alpha entry should have been removed."
        );

        assert_eq!(TotalHotkeyAlpha::<Test>::get(hotkey_nonzero, netuid), AlphaBalance::from(123));

        assert!(
            !weight.is_zero(),
            "Migration weight should be non-zero."
        );
    });
}
