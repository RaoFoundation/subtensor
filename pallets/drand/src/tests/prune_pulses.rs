use super::*;

#[test]
fn test_pulses_are_correctly_pruned() {
    new_test_ext().execute_with(|| {
        let pulse = Pulse::default();
        let last_round: u64 = MAX_KEPT_PULSES + 2;
        let oldest_round: u64 = 1;
        let prune_count: u64 = 2;
        let new_oldest: u64 = oldest_round + prune_count;
        let middle_round: u64 = MAX_KEPT_PULSES / 2;

        // Set storage bounds
        OldestStoredRound::<Test>::put(oldest_round);
        LastStoredRound::<Test>::put(last_round);

        // Insert pulses at boundaries
        // These should be pruned
        Pulses::<Test>::insert(1, pulse.clone());
        Pulses::<Test>::insert(2, pulse.clone());

        // This should remain (new oldest)
        Pulses::<Test>::insert(new_oldest, pulse.clone());

        // Middle and last should remain
        Pulses::<Test>::insert(middle_round, pulse.clone());
        Pulses::<Test>::insert(last_round, pulse.clone());

        // Trigger prune
        Drand::prune_old_pulses(last_round);

        // Assert new oldest
        assert_eq!(OldestStoredRound::<Test>::get(), new_oldest);

        // Assert pruned correctly
        assert!(!Pulses::<Test>::contains_key(1), "Round 1 should be pruned");
        assert!(!Pulses::<Test>::contains_key(2), "Round 2 should be pruned");

        // Assert not pruned incorrectly
        assert!(
            Pulses::<Test>::contains_key(new_oldest),
            "New oldest round should remain"
        );
        assert!(
            Pulses::<Test>::contains_key(middle_round),
            "Middle round should remain"
        );
        assert!(
            Pulses::<Test>::contains_key(last_round),
            "Last round should remain"
        );
    });
}

#[test]
fn test_prune_maximum_of_100_pulses_per_call() {
    new_test_ext().execute_with(|| {
        // ------------------------------------------------------------
        // 1. Arrange – create a storage layout that exceeds MAX_KEPT_PULSES
        // ------------------------------------------------------------
        const EXTRA: u64 = 250;
        let oldest_round: u64 = 1;
        let last_round: u64 = oldest_round + MAX_KEPT_PULSES + EXTRA;

        OldestStoredRound::<Test>::put(oldest_round);
        LastStoredRound::<Test>::put(last_round);
        let pulse = Pulse::default();

        // Insert the first 150 rounds so we can check they disappear / stay
        for r in oldest_round..=oldest_round + 150 {
            Pulses::<Test>::insert(r, pulse.clone());
        }
        let mid_round = oldest_round + 150;
        Pulses::<Test>::insert(last_round, pulse.clone());

        // ------------------------------------------------------------
        // 2. Act – run the pruning function once
        // ------------------------------------------------------------
        Drand::prune_old_pulses(last_round);

        // ------------------------------------------------------------
        // 3. Assert – only the *first* 100 pulses were removed
        // ------------------------------------------------------------
        let expected_new_oldest = oldest_round + 100; // 101

        // ‣ Storage bound updated correctly
        assert_eq!(
            OldestStoredRound::<Test>::get(),
            expected_new_oldest,
            "OldestStoredRound should advance by exactly 100"
        );

        // ‣ Rounds 1‑100 are gone
        for r in oldest_round..expected_new_oldest {
            assert!(
                !Pulses::<Test>::contains_key(r),
                "Round {r} should have been pruned"
            );
        }

        // ‣ Round 101 (new oldest) and later rounds remain
        assert!(
            Pulses::<Test>::contains_key(expected_new_oldest),
            "Round {expected_new_oldest} should remain after pruning"
        );
        assert!(
            Pulses::<Test>::contains_key(mid_round),
            "Mid-range round should remain after pruning"
        );
        assert!(
            Pulses::<Test>::contains_key(last_round),
            "LastStoredRound should remain after pruning"
        );
    });
}
