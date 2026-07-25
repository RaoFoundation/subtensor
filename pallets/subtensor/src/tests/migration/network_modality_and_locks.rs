#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! network modality removal, subnet limit, lock cost/decay, kappa.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_remove_network_modality() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIGRATION_NAME: &str = "migrate_remove_network_modality";

        // Create multiple networks to test
        let netuids: [NetUid; 3] = [1.into(), 2.into(), 3.into()];
        for netuid in netuids.iter() {
            add_network(*netuid, 1, 0);
        }

        // Set initial storage version to 7 (below target)
        StorageVersion::new(7).put::<Pallet<Test>>();
        assert_eq!(
            Pallet::<Test>::on_chain_storage_version(),
            StorageVersion::new(7)
        );

        // ------------------------------
        // 1. Simulate NetworkModality entries using deprecated storage alias
        // ------------------------------
        // We need to manually create storage entries that would exist for NetworkModality
        // Since NetworkModality was a StorageMap<_, Identity, NetUid, u16>, we simulate this
        let pallet_prefix = twox_128("SubtensorModule".as_bytes());
        let storage_prefix = twox_128("NetworkModality".as_bytes());

        // Create NetworkModality entries for each network
        for (i, netuid) in netuids.iter().enumerate() {
            let mut key = Vec::new();
            key.extend_from_slice(&pallet_prefix);
            key.extend_from_slice(&storage_prefix);
            // Identity encoding for netuid
            key.extend_from_slice(&netuid.encode());

            let modality_value: u16 = (i as u16) + 1; // Different values for testing
            put_raw(&key, &modality_value.encode());

            // Verify the entry was created
            let stored_value = get_raw(&key).expect("NetworkModality entry should exist");
            assert_eq!(
                u16::decode(&mut &stored_value[..]).expect("Failed to decode modality"),
                modality_value
            );
        }

        assert!(
            !HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should not have run yet"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let weight =
            crate::migrations::migrate_remove_network_modality::migrate_remove_network_modality::<
                Test,
            >();

        // ------------------------------
        // 3. Verify migration effects
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIGRATION_NAME.as_bytes().to_vec()),
            "Migration should be marked as run"
        );

        // Verify weight is non-zero
        assert!(!weight.is_zero(), "Migration weight should be non-zero");

        // Verify weight calculation: 1 read (version check) + 1 read (total networks) + N writes (removal) + 1 write (version update)
        let expected_weight = <Test as Config>::DbWeight::get().reads(2)
            + <Test as Config>::DbWeight::get().writes(netuids.len() as u64 + 1);
        assert_eq!(
            weight, expected_weight,
            "Weight calculation should be correct"
        );
    });
}

#[test]
fn test_migrate_remove_network_modality_already_run() {
    new_test_ext(1).execute_with(|| {
        const MIGRATION_NAME: &str = "migrate_remove_network_modality";

        // Mark migration as already run
        HasMigrationRun::<Test>::insert(MIGRATION_NAME.as_bytes().to_vec(), true);

        // Set storage version to 8 (target version)
        StorageVersion::new(8).put::<Pallet<Test>>();
        assert_eq!(
            Pallet::<Test>::on_chain_storage_version(),
            StorageVersion::new(8)
        );

        // Run migration
        let weight =
            crate::migrations::migrate_remove_network_modality::migrate_remove_network_modality::<
                Test,
            >();

        // Should only have read weight for checking migration status
        let expected_weight = <Test as Config>::DbWeight::get().reads(1);
        assert_eq!(
            weight, expected_weight,
            "Second run should only read the migration flag"
        );

        // Verify migration is still marked as run
        assert!(HasMigrationRun::<Test>::get(
            MIGRATION_NAME.as_bytes().to_vec()
        ));
    });
}

#[test]
fn test_migrate_subnet_limit_to_default() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIG_NAME: &[u8] = b"subnet_limit_to_default";

        // Compute a non-default value safely
        let default: u16 = DefaultSubnetLimit::<Test>::get();
        let not_default: u16 = default.wrapping_add(1);

        // ------------------------------
        // 1. Pre-state: ensure a non-default value is stored
        // ------------------------------
        SubnetLimit::<Test>::put(not_default);
        assert_eq!(
            SubnetLimit::<Test>::get(),
            not_default,
            "precondition failed: SubnetLimit should be non-default before migration"
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let w = crate::migrations::migrate_subnet_limit_to_default::migrate_subnet_limit_to_default::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // ------------------------------
        // 3. Verify results
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag not set"
        );

        assert_eq!(
            SubnetLimit::<Test>::get(),
            default,
            "SubnetLimit should be reset to the configured default"
        );
    });
}

#[test]
fn test_migrate_network_lock_reduction_interval_and_decay() {
    new_test_ext(0).execute_with(|| {
        const FOUR_DAYS: u64 = 28_800;
        const EIGHT_DAYS: u64 = 57_600;
        const ONE_WEEK_BLOCKS: u64 = 50_400;

        // ── pre ──────────────────────────────────────────────────────────────
        assert!(
            !HasMigrationRun::<Test>::get(b"migrate_network_lock_reduction_interval".to_vec()),
            "HasMigrationRun should be false before migration"
        );

        // ensure current_block > 0
        step_block(1);
        let current_block_before = Pallet::<Test>::get_current_block_as_u64();

        // ── run migration ────────────────────────────────────────────────────
        let weight = crate::migrations::migrate_network_lock_reduction_interval::migrate_network_lock_reduction_interval::<Test>();
        assert!(!weight.is_zero(), "migration weight should be > 0");

        // ── params & flags ───────────────────────────────────────────────────
        assert_eq!(NetworkLockReductionInterval::<Test>::get(), EIGHT_DAYS);
        assert_eq!(NetworkRateLimit::<Test>::get(), FOUR_DAYS);
        assert_eq!(
            Pallet::<Test>::get_network_last_lock(),
            1_000_000_000_000u64.into(), // 1000 TAO in rao
            "last_lock should be 1_000_000_000_000 rao"
        );

        // last_lock_block should be set one week in the future
        let last_lock_block = Pallet::<Test>::get_network_last_lock_block();
        let expected_block = current_block_before + ONE_WEEK_BLOCKS;
        assert_eq!(
            last_lock_block,
            expected_block,
            "last_lock_block should be current + ONE_WEEK_BLOCKS"
        );

        // registration start block should match the same future block
        assert_eq!(
            NetworkRegistrationStartBlock::<Test>::get(),
            expected_block,
            "NetworkRegistrationStartBlock should equal last_lock_block"
        );

        // lock cost should be 2000 TAO immediately after migration
        let lock_cost_now = Pallet::<Test>::get_network_lock_cost();
        assert_eq!(
            lock_cost_now,
            2_000_000_000_000u64.into(),
            "lock cost should be 2000 TAO right after migration"
        );

        assert!(
            HasMigrationRun::<Test>::get(b"migrate_network_lock_reduction_interval".to_vec()),
            "HasMigrationRun should be true after migration"
        );
    });
}

#[test]
fn test_migrate_restore_subnet_locked_65_128() {
    use sp_runtime::traits::SaturatedConversion;
    new_test_ext(0).execute_with(|| {
        let name = b"migrate_restore_subnet_locked".to_vec();
        assert!(
            !HasMigrationRun::<Test>::get(name.clone()),
            "HasMigrationRun should be false before migration"
        );

        // Expected snapshot for netuids 65..128.
        const EXPECTED: &[(u16, u64)] = &[
            (65, 37_274_536_408),
            (66, 65_230_444_016),
            (67, 114_153_284_032),
            (68, 199_768_252_064),
            (69, 349_594_445_728),
            (70, 349_412_366_216),
            (71, 213_408_488_702),
            (72, 191_341_473_067),
            (73, 246_711_333_592),
            (74, 291_874_466_228),
            (75, 247_485_227_056),
            (76, 291_241_991_316),
            (77, 303_154_601_714),
            (78, 287_407_417_932),
            (79, 254_935_051_664),
            (80, 255_413_055_349),
            (81, 249_790_431_509),
            (82, 261_343_249_180),
            (83, 261_361_408_796),
            (84, 201_938_003_214),
            (85, 264_805_234_604),
            (86, 223_171_973_880),
            (87, 180_397_358_280),
            (88, 270_596_039_760),
            (89, 286_399_608_951),
            (90, 267_684_201_301),
            (91, 284_637_542_762),
            (92, 288_373_410_868),
            (93, 290_836_604_849),
            (94, 270_861_792_144),
            (95, 210_595_055_304),
            (96, 315_263_727_200),
            (97, 158_244_884_792),
            (98, 168_102_223_900),
            (99, 252_153_339_800),
            (100, 378_230_014_000),
            (101, 205_977_765_866),
            (102, 149_434_017_849),
            (103, 135_476_471_008),
            (104, 147_970_415_680),
            (105, 122_003_668_139),
            (106, 133_585_556_570),
            (107, 200_137_144_216),
            (108, 106_767_623_816),
            (109, 124_280_483_748),
            (110, 186_420_726_696),
            (111, 249_855_564_892),
            (112, 196_761_272_984),
            (113, 147_120_048_727),
            (114, 84_021_895_534),
            (115, 98_002_215_656),
            (116, 89_944_262_256),
            (117, 107_183_582_952),
            (118, 110_644_724_664),
            (119, 99_380_483_902),
            (120, 138_829_019_156),
            (121, 111_988_743_976),
            (122, 130_264_686_152),
            (123, 118_034_291_488),
            (124, 79_312_501_676),
            (125, 43_214_310_704),
            (126, 64_755_449_962),
            (127, 97_101_698_382),
            (128, 145_645_807_991),
        ];

        // Run migration
        let weight =
            crate::migrations::migrate_subnet_locked::migrate_restore_subnet_locked::<Test>();
        assert!(!weight.is_zero(), "migration weight should be > 0");

        // Read back storage as (u16 -> u64)
        let actual: BTreeMap<u16, u64> = SubnetLocked::<Test>::iter()
            .map(|(k, v)| (k.saturated_into::<u16>(), u64::from(v)))
            .collect();

        let expected: BTreeMap<u16, u64> = EXPECTED.iter().copied().collect();

        // 1) exact content
        assert_eq!(
            actual, expected,
            "SubnetLocked map mismatch for 65..128 snapshot"
        );

        // 2) count and total
        let expected_len = expected.len();
        let expected_sum: u128 = expected.values().map(|v| *v as u128).sum();

        let count_after = actual.len();
        let sum_after: u128 = actual.values().map(|v| *v as u128).sum();

        assert_eq!(count_after, expected_len, "entry count mismatch");
        assert_eq!(sum_after, expected_sum, "total RAO sum mismatch");

        // 3) migration flag set
        assert!(
            HasMigrationRun::<Test>::get(name.clone()),
            "HasMigrationRun should be true after migration"
        );

        // 4) idempotence
        let before = actual.clone();
        let _again =
            crate::migrations::migrate_subnet_locked::migrate_restore_subnet_locked::<Test>();
        let after: BTreeMap<u16, u64> = SubnetLocked::<Test>::iter()
            .map(|(k, v)| (k.saturated_into::<u16>(), u64::from(v)))
            .collect();
        assert_eq!(
            before, after,
            "re-running the migration should not change storage"
        );
    });
}

#[test]
fn test_migrate_network_lock_cost_2500_sets_price_and_decay() {
    new_test_ext(0).execute_with(|| {
        // ── constants ───────────────────────────────────────────────────────
        const RAO_PER_TAO: u64 = 1_000_000_000;
        const TARGET_COST_TAO: u64 = 2_500;
        const TARGET_COST_RAO: u64 = TARGET_COST_TAO * RAO_PER_TAO;
        const NEW_LAST_LOCK_RAO: u64 = (TARGET_COST_TAO / 2) * RAO_PER_TAO;

        let migration_key = b"migrate_network_lock_cost_2500".to_vec();

        // ── pre ──────────────────────────────────────────────────────────────
        assert!(
            !HasMigrationRun::<Test>::get(migration_key.clone()),
            "HasMigrationRun should be false before migration"
        );

        // Ensure current_block > 0 so mult == 2 in get_network_lock_cost()
        step_block(1);
        let current_block_before = Pallet::<Test>::get_current_block_as_u64();

        // Snapshot interval to ensure migration doesn't change it
        let interval_before = NetworkLockReductionInterval::<Test>::get();

        // ── run migration ────────────────────────────────────────────────────
        let weight = crate::migrations::migrate_network_lock_cost_2500::migrate_network_lock_cost_2500::<Test>();
        assert!(!weight.is_zero(), "migration weight should be > 0");

        // ── asserts: params & flags ─────────────────────────────────────────
        assert_eq!(
            Pallet::<Test>::get_network_last_lock(),
            NEW_LAST_LOCK_RAO.into(),
            "last_lock should be set to 1,250 TAO (in rao)"
        );
        assert_eq!(
            Pallet::<Test>::get_network_last_lock_block(),
            current_block_before,
            "last_lock_block should be set to the current block"
        );

        // Lock cost should be exactly 2,500 TAO immediately after migration
        let lock_cost_now = Pallet::<Test>::get_network_lock_cost();
        assert_eq!(
            lock_cost_now,
            TARGET_COST_RAO.into(),
            "lock cost should be 2,500 TAO right after migration"
        );

        // Interval should be unchanged by this migration
        assert_eq!(
            NetworkLockReductionInterval::<Test>::get(),
            interval_before,
            "lock reduction interval should not be modified by this migration"
        );

        assert!(
            HasMigrationRun::<Test>::get(migration_key.clone()),
            "HasMigrationRun should be true after migration"
        );

        // ── decay check (1 block later) ─────────────────────────────────────
        // Expected: cost = max(min_lock, 2*L - floor(L / eff_interval) * delta_blocks)
        let eff_interval = Pallet::<Test>::get_lock_reduction_interval();
        let per_block_decrement: u64 = if eff_interval == 0 {
            0
        } else {
            NEW_LAST_LOCK_RAO / eff_interval
        };

        let min_lock_rao: u64 = Pallet::<Test>::get_network_min_lock().to_u64();

        step_block(1);
        let expected_after_1: u64 =
            core::cmp::max(min_lock_rao, TARGET_COST_RAO - per_block_decrement);
        let lock_cost_after_1 = Pallet::<Test>::get_network_lock_cost();
        assert_eq!(
            lock_cost_after_1,
            expected_after_1.into(),
            "lock cost should decay by one per-block step after 1 block"
        );

        // ── idempotency: running the migration again should do nothing ──────
        let last_lock_before_rerun = Pallet::<Test>::get_network_last_lock();
        let last_lock_block_before_rerun = Pallet::<Test>::get_network_last_lock_block();
        let cost_before_rerun = Pallet::<Test>::get_network_lock_cost();

        let _weight2 = crate::migrations::migrate_network_lock_cost_2500::migrate_network_lock_cost_2500::<Test>();

        assert!(
            HasMigrationRun::<Test>::get(migration_key.clone()),
            "HasMigrationRun remains true on second run"
        );
        assert_eq!(
            Pallet::<Test>::get_network_last_lock(),
            last_lock_before_rerun,
            "second run should not modify last_lock"
        );
        assert_eq!(
            Pallet::<Test>::get_network_last_lock_block(),
            last_lock_block_before_rerun,
            "second run should not modify last_lock_block"
        );
        assert_eq!(
            Pallet::<Test>::get_network_lock_cost(),
            cost_before_rerun,
            "second run should not change current lock cost"
        );
    });
}

#[test]
fn test_migrate_kappa_map_to_default() {
    new_test_ext(1).execute_with(|| {
        // ------------------------------
        // 0. Constants / helpers
        // ------------------------------
        const MIG_NAME: &[u8] = b"kappa_map_to_default";
        let default: u16 = DefaultKappa::<Test>::get();

        let not_default: u16 = if default == u16::MAX {
            default - 1
        } else {
            default + 1
        };

        // ------------------------------
        // 1. Pre-state: seed using the correct key type (NetUid)
        // ------------------------------
        let n0: NetUid = 0u16.into();
        let n1: NetUid = 1u16.into();
        let n2: NetUid = 42u16.into();

        Kappa::<Test>::insert(n0, not_default);
        Kappa::<Test>::insert(n1, default);
        Kappa::<Test>::insert(n2, not_default);

        assert_eq!(
            Kappa::<Test>::get(n0),
            not_default,
            "precondition failed: Kappa[n0] should be non-default before migration"
        );
        assert_eq!(
            Kappa::<Test>::get(n1),
            default,
            "precondition failed: Kappa[n1] should be default before migration"
        );
        assert_eq!(
            Kappa::<Test>::get(n2),
            not_default,
            "precondition failed: Kappa[n2] should be non-default before migration"
        );

        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // ------------------------------
        // 2. Run migration
        // ------------------------------
        let w =
            crate::migrations::migrate_kappa_map_to_default::migrate_kappa_map_to_default::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // ------------------------------
        // 3. Verify results
        // ------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag not set"
        );

        assert_eq!(
            Kappa::<Test>::get(n0),
            default,
            "Kappa[n0] should be reset to the configured default"
        );
        assert_eq!(
            Kappa::<Test>::get(n1),
            default,
            "Kappa[n1] should remain at the configured default"
        );
        assert_eq!(
            Kappa::<Test>::get(n2),
            default,
            "Kappa[n2] should be reset to the configured default"
        );
    });
}
