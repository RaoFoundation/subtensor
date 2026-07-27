#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! staking hotkeys, root TAO/alpha, symbols, registration/nominator settings.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_fix_staking_hot_keys() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_fix_staking_hot_keys";

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should not have run yet"
        );

        // Add some data
        Alpha::<Test>::insert(
            (U256::from(1), U256::from(2), NetUid::ROOT),
            U64F64::from(1_u64),
        );
        // Run migration
        let weight =
            migrations::migrate_fix_staking_hot_keys::migrate_fix_staking_hot_keys::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()),
            "Migration should be marked as completed"
        );

        // Check migration has been marked as run
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        // Verify results
        assert_eq!(
            StakingHotkeys::<Test>::get(U256::from(2)),
            vec![U256::from(1)]
        );
    });
}

#[test]
fn test_migrate_fix_root_subnet_tao() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_fix_root_subnet_tao";

        let mut expected_total_stake = 0_u64;
        // Seed some hotkeys with some fake stake.
        for i in 0..100_000 {
            Owner::<Test>::insert(U256::from(U256::from(i)), U256::from(i + 1_000_000));
            let stake = i + 1_000_000;
            TotalHotkeyAlpha::<Test>::insert(
                U256::from(U256::from(i)),
                NetUid::ROOT,
                AlphaBalance::from(stake),
            );
            expected_total_stake += stake;
        }

        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // Run the migration
        let weight =
            crate::migrations::migrate_fix_root_subnet_tao::migrate_fix_root_subnet_tao::<Test>();

        // Verify the migration ran correctly
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
        assert_eq!(
            SubnetTAO::<Test>::get(NetUid::ROOT),
            expected_total_stake.into()
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_fix_root_tao_and_alpha_in --exact --show-output
#[test]
fn test_migrate_fix_root_tao_and_alpha_in() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_fix_root_tao_and_alpha_in";

        // Set counters initially
        let initial_value = 1_000_000_000_000_u64;
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(initial_value));
        SubnetAlphaIn::<Test>::insert(NetUid::ROOT, AlphaBalance::from(initial_value));
        SubnetAlphaOut::<Test>::insert(NetUid::ROOT, AlphaBalance::from(initial_value));
        SubnetVolume::<Test>::insert(NetUid::ROOT, initial_value as u128);
        TotalStake::<Test>::set(TaoBalance::from(initial_value));

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // Run the migration
        let weight =
            crate::migrations::migrate_fix_root_tao_and_alpha_in::migrate_fix_root_tao_and_alpha_in::<Test>();

        // Verify the migration ran correctly
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // Verify counters have changed
        assert!(SubnetTAO::<Test>::get(NetUid::ROOT) != initial_value.into());
        assert!(SubnetAlphaIn::<Test>::get(NetUid::ROOT) != initial_value.into());
        assert!(SubnetAlphaOut::<Test>::get(NetUid::ROOT) != initial_value.into());
        assert!(SubnetVolume::<Test>::get(NetUid::ROOT) != initial_value as u128);
        assert!(TotalStake::<Test>::get() != initial_value.into());
    });
}

#[test]
fn test_migrate_subnet_symbols() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_subnet_symbols";

        // Create 100 subnets
        for i in 0..100 {
            add_network(i.into(), 1, 0);
        }

        // Shift some symbols
        TokenSymbol::<Test>::insert(
            NetUid::from(21),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(142)),
        );
        TokenSymbol::<Test>::insert(
            NetUid::from(42),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(184)),
        );
        TokenSymbol::<Test>::insert(
            NetUid::from(83),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(242)),
        );
        TokenSymbol::<Test>::insert(
            NetUid::from(99),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(284)),
        );

        // Run the migration
        let weight = crate::migrations::migrate_subnet_symbols::migrate_subnet_symbols::<Test>();

        // Check that the symbols have been corrected
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(21)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(21))
        );
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(42)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(42))
        );
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(83)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(83))
        );
        assert_eq!(
            TokenSymbol::<Test>::get(NetUid::from(99)),
            SubtensorModule::get_symbol_for_subnet(NetUid::from(99))
        );

        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_set_registration_enable() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_set_registration_enable";

        // Create 3 subnets
        let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
        for netuid in netuids.iter() {
            add_network(*netuid, 1, 0);
            // Set registration to false to simulate the need for migration
            SubtensorModule::set_network_registration_allowed(*netuid, false);
        }

        // Sanity check: registration is disabled before migration
        for netuid in netuids.iter() {
            assert!(!SubtensorModule::get_network_registration_allowed(*netuid));
        }

        // Run the migration
        let weight =
            crate::migrations::migrate_set_registration_enable::migrate_set_registration_enable::<
                Test,
            >();

        // After migration, regular registration should be enabled for all subnets except root
        for netuid in netuids.iter() {
            assert!(SubtensorModule::get_network_registration_allowed(*netuid));
        }

        // Migration should be marked as run
        assert!(HasMigrationRun::<Test>::get(
            MIGRATION_NAME.as_bytes().to_vec()
        ));

        // Weight should be non-zero
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
    });
}

#[test]
fn test_migrate_set_nominator_min_stake() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_set_nominator_min_stake";

        let min_nomination_initial = 100_000_000;
        let min_nomination_migrated = 10_000_000;
        NominatorMinRequiredStake::<Test>::set(min_nomination_initial);

        assert_eq!(
            NominatorMinRequiredStake::<Test>::get(),
            min_nomination_initial
        );
        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // Run the migration
        let weight =
            crate::migrations::migrate_set_nominator_min_stake::migrate_set_nominator_min_stake::<
                Test,
            >();

        // Verify the migration ran correctly
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );
        assert!(!weight.is_zero(), "Migration weight should be non-zero");
        assert_eq!(
            NominatorMinRequiredStake::<Test>::get(),
            min_nomination_migrated
        );
    });
}
