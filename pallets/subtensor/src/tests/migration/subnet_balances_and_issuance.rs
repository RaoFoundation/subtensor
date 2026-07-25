#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! subnet balances + total issuance EVM fees.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_subnet_balances() {
    new_test_ext(1).execute_with(|| {
        let netuid1 = NetUid::from(1);
        let netuid2 = NetUid::from(2);
        add_network(netuid1, 1, 0);
        add_network(netuid2, 1, 0);

        // Add network locks
        let lock1 = TaoBalance::from(123_000_000_000_u64);
        let lock2 = TaoBalance::from(321_000_000_000_u64);
        SubnetLocked::<Test>::insert(netuid1, lock1);
        SubnetLocked::<Test>::insert(netuid2, lock2);

        // Add SubnetTAO
        let reserve1 = TaoBalance::from(456_000_000_000_u64);
        let reserve2 = TaoBalance::from(654_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid1, reserve1);
        SubnetTAO::<Test>::insert(netuid2, reserve2);

        // Run migration
        crate::migrations::migrate_subnet_balances::migrate_subnet_balances::<Test>();

        // Test that subnet balances got updated
        let subnet_account_1 = SubtensorModule::get_subnet_account_id(netuid1).unwrap();
        let subnet_account_2 = SubtensorModule::get_subnet_account_id(netuid2).unwrap();
        let balance1 = SubtensorModule::get_coldkey_balance(&subnet_account_1);
        let balance2 = SubtensorModule::get_coldkey_balance(&subnet_account_2);
        let initial_pool_tao = NetworkMinLockCost::<Test>::get();
        assert_eq!(balance1, lock1 + reserve1 - initial_pool_tao);
        assert_eq!(balance2, lock2 + reserve2 - initial_pool_tao);

        // Check migration has been marked as run
        const MIGRATION_NAME: &[u8] = b"migrate_subnet_balances";
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
    });
}

#[test]
fn test_migrate_fix_total_issuance_evm_fees() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &[u8] = b"migrate_fix_total_issuance_evm_fees";
        const DUST_MIGRATION_NAME: &[u8] = b"migrate_fix_total_issuance_after_dust_collection";

        let account = U256::from(42);
        let balances_total_issuance = TaoBalance::from(123_456_789_u64);
        Balances::make_free_balance_be(&account, balances_total_issuance);

        let broken_subtensor_total_issuance = TaoBalance::from(987_654_321_u64);
        TotalIssuance::<Test>::put(broken_subtensor_total_issuance);

        assert_eq!(Balances::total_issuance(), balances_total_issuance);
        assert_eq!(
            TotalIssuance::<Test>::get(),
            broken_subtensor_total_issuance
        );
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));

        let weight = crate::migrations::migrate_fix_total_issuance_evm_fees::migrate_fix_total_issuance_evm_fees::<Test>();

        assert!(!weight.is_zero(), "weight must be non-zero");
        assert_eq!(TotalIssuance::<Test>::get(), balances_total_issuance);
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
        assert!(!HasMigrationRun::<Test>::get(
            DUST_MIGRATION_NAME.to_vec()
        ));

        let second_wrong_value = TaoBalance::from(555_u64);
        TotalIssuance::<Test>::put(second_wrong_value);

        crate::migrations::migrate_fix_total_issuance_evm_fees::migrate_fix_total_issuance_evm_fees::<Test>();

        assert_eq!(TotalIssuance::<Test>::get(), balances_total_issuance);
        assert!(HasMigrationRun::<Test>::get(
            DUST_MIGRATION_NAME.to_vec()
        ));

        let third_wrong_value = TaoBalance::from(777_u64);
        TotalIssuance::<Test>::put(third_wrong_value);

        crate::migrations::migrate_fix_total_issuance_evm_fees::migrate_fix_total_issuance_evm_fees::<Test>();

        assert_eq!(
            TotalIssuance::<Test>::get(),
            third_wrong_value,
            "migration must not run after all known migration keys have run"
        );
    });
}
