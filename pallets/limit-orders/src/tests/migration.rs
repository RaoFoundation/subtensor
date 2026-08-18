#![allow(clippy::unwrap_used)]
//! Tests for the `migrate_register_pallet_hotkey` migration.

use frame_support::{BoundedVec, traits::Hooks};
use sp_runtime::{BuildStorage, traits::AccountIdConversion};
use subtensor_swap_interface::OrderSwapInterface as _;

use crate::{
    HasMigrationRun, LimitOrdersEnabled, MigrationKeyMaxLen,
    migrations::migrate_register_pallet_hotkey,
    tests::mock::{LimitOrdersPalletId, MockSwap, PalletHotkeyAccount, System, Test},
};

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_register_pallet_hotkey".to_vec())
}

/// Minimal externalities: system genesis only, no pallet hotkey pre-registered,
/// `LimitOrdersEnabled` at its storage default (`false`).
fn migration_ext() -> sp_io::TestExternalities {
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

#[test]
fn migration_registers_hotkey_and_marks_run_on_first_call() {
    migration_ext().execute_with(|| {
        let pallet_acct: crate::tests::mock::AccountId =
            LimitOrdersPalletId::get().into_account_truncating();
        let pallet_hotkey = PalletHotkeyAccount::get();

        assert!(!MockSwap::pallet_hotkey_registered(
            &pallet_acct,
            &pallet_hotkey
        ));
        assert!(!HasMigrationRun::<Test>::get(migration_key()));

        migrate_register_pallet_hotkey::<Test>();

        assert!(
            MockSwap::pallet_hotkey_registered(&pallet_acct, &pallet_hotkey),
            "hotkey must be registered after migration"
        );
        assert!(
            HasMigrationRun::<Test>::get(migration_key()),
            "migration must be marked as run"
        );
        // Migration no longer touches LimitOrdersEnabled — value is unchanged.
        assert!(!LimitOrdersEnabled::<Test>::get());
    });
}

#[test]
fn migration_does_not_touch_limit_orders_enabled() {
    migration_ext().execute_with(|| {
        // Enable the pallet before running the migration (simulates a chain
        // that already had it enabled via genesis or admin action).
        LimitOrdersEnabled::<Test>::set(true);

        migrate_register_pallet_hotkey::<Test>();

        assert!(
            LimitOrdersEnabled::<Test>::get(),
            "migration must not change LimitOrdersEnabled"
        );
    });
}

#[test]
fn migration_skips_hotkey_registration_when_already_registered() {
    migration_ext().execute_with(|| {
        let pallet_acct: crate::tests::mock::AccountId =
            LimitOrdersPalletId::get().into_account_truncating();
        let pallet_hotkey = PalletHotkeyAccount::get();
        let _ = MockSwap::register_pallet_hotkey(&pallet_acct, &pallet_hotkey);

        // Must not panic on duplicate registration.
        migrate_register_pallet_hotkey::<Test>();

        assert!(HasMigrationRun::<Test>::get(migration_key()));
    });
}

#[test]
fn migration_is_idempotent() {
    migration_ext().execute_with(|| {
        let pallet_acct: crate::tests::mock::AccountId =
            LimitOrdersPalletId::get().into_account_truncating();
        let pallet_hotkey = PalletHotkeyAccount::get();

        migrate_register_pallet_hotkey::<Test>();
        assert!(MockSwap::pallet_hotkey_registered(
            &pallet_acct,
            &pallet_hotkey
        ));

        // Second run must be a no-op — hotkey stays registered, flag stays set.
        migrate_register_pallet_hotkey::<Test>();
        assert!(MockSwap::pallet_hotkey_registered(
            &pallet_acct,
            &pallet_hotkey
        ));
        assert!(HasMigrationRun::<Test>::get(migration_key()));
    });
}

#[test]
fn on_runtime_upgrade_delegates_to_migration() {
    migration_ext().execute_with(|| {
        assert!(!HasMigrationRun::<Test>::get(migration_key()));

        <crate::Pallet<Test> as Hooks<u64>>::on_runtime_upgrade();

        assert!(HasMigrationRun::<Test>::get(migration_key()));
    });
}
