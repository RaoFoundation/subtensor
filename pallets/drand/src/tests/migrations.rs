use super::*;

#[test]
fn test_migrate_prune_old_pulses() {
    new_test_ext().execute_with(|| {
        let migration_name = BoundedVec::truncate_from(b"migrate_prune_old_pulses".to_vec());
        let pulse = Pulse::default();

        assert_eq!(Pulses::<Test>::iter().count(), 0);
        assert!(!HasMigrationRun::<Test>::get(&migration_name));
        assert_eq!(OldestStoredRound::<Test>::get(), 0);
        assert_eq!(LastStoredRound::<Test>::get(), 0);

        // Test with more pulses than MAX_KEPT_PULSES
        let excess: u64 = 9;
        let total: u64 = MAX_KEPT_PULSES + excess;
        for i in 1..=total {
            Pulses::<Test>::insert(i, pulse.clone());
        }

        let weight_large = migrate_prune_old_pulses::<Test>();

        let expected_oldest = excess + 1;
        assert_eq!(OldestStoredRound::<Test>::get(), expected_oldest);
        assert_eq!(LastStoredRound::<Test>::get(), total);

        for i in 1..=excess {
            assert!(!Pulses::<Test>::contains_key(i));
        }
        for i in expected_oldest..=total {
            assert!(Pulses::<Test>::contains_key(i));
        }

        let db_weight: RuntimeDbWeight = <Test as frame_system::Config>::DbWeight::get();
        let num_pulses = total;
        let num_to_delete = num_pulses - MAX_KEPT_PULSES;
        let expected_weight = db_weight.reads(1 + num_pulses) + db_weight.writes(num_to_delete + 3);
        assert_eq!(weight_large, expected_weight);
    });
}

#[test]
fn test_migrate_set_oldest_round() {
    new_test_ext().execute_with(|| {
        let migration_name = BoundedVec::truncate_from(b"migrate_set_oldest_round".to_vec());
        let db_weight: RuntimeDbWeight = <Test as frame_system::Config>::DbWeight::get();
        let pulse = Pulse::default();

        assert_eq!(Pulses::<Test>::iter().count(), 0);
        assert!(!HasMigrationRun::<Test>::get(&migration_name));
        assert_eq!(OldestStoredRound::<Test>::get(), 0);
        assert_eq!(LastStoredRound::<Test>::get(), 0);

        // Insert out-of-order rounds: oldest should be 5
        for r in [10u64, 7, 5].into_iter() {
            Pulses::<Test>::insert(r, pulse.clone());
        }
        let num_rounds = 3u64;

        // Run migration
        let weight = migrate_set_oldest_round::<Test>();

        assert_eq!(OldestStoredRound::<Test>::get(), 5);
        // Migration does NOT touch LastStoredRound
        assert_eq!(LastStoredRound::<Test>::get(), 0);
        // Pulses untouched
        assert!(Pulses::<Test>::contains_key(5));
        assert!(Pulses::<Test>::contains_key(7));
        assert!(Pulses::<Test>::contains_key(10));
        // Flag set
        assert!(HasMigrationRun::<Test>::get(&migration_name));

        // Weight: reads(1 + num_rounds) + writes(2) [Oldest + HasMigrationRun]
        let expected = db_weight.reads(1 + num_rounds) + db_weight.writes(2);
        assert_eq!(weight, expected);
    });
}
