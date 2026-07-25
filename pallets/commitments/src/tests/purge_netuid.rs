//! Tests for commitments pallet: purge netuid.

use super::*;

#[test]
fn purge_netuid_clears_only_that_netuid() {
    new_test_ext().execute_with(|| {
        // Setup
        System::<Test>::set_block_number(1);

        let net_a = NetUid::from(42);
        let net_b = NetUid::from(43);
        let who_a1: u64 = 1001;
        let who_a2: u64 = 1002;
        let who_b: u64 = 2001;

        // Minimal commitment payload
        let empty_fields: BoundedVec<Data, <Test as Config>::MaxFields> = BoundedVec::default();
        let info_empty: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: empty_fields,
        };
        let bn = System::<Test>::block_number();

        // Seed NET A with two accounts across all tracked storages
        let reg_a1 = Registration {
            deposit: Default::default(),
            block: bn,
            info: info_empty.clone(),
        };
        let reg_a2 = Registration {
            deposit: Default::default(),
            block: bn,
            info: info_empty.clone(),
        };
        CommitmentOf::<Test>::insert(net_a, who_a1, reg_a1);
        CommitmentOf::<Test>::insert(net_a, who_a2, reg_a2);
        LastCommitment::<Test>::insert(net_a, who_a1, bn);
        LastCommitment::<Test>::insert(net_a, who_a2, bn);
        LastBondsReset::<Test>::insert(net_a, who_a1, bn);
        RevealedCommitments::<Test>::insert(net_a, who_a1, vec![(b"a".to_vec(), 7u64)]);
        UsedSpaceOf::<Test>::insert(
            net_a,
            who_a1,
            UsageTracker {
                last_epoch: 1,
                used_space: 123,
            },
        );

        // Seed NET B with one account that must remain intact
        let reg_b = Registration {
            deposit: Default::default(),
            block: bn,
            info: info_empty,
        };
        CommitmentOf::<Test>::insert(net_b, who_b, reg_b);
        LastCommitment::<Test>::insert(net_b, who_b, bn);
        LastBondsReset::<Test>::insert(net_b, who_b, bn);
        RevealedCommitments::<Test>::insert(net_b, who_b, vec![(b"b".to_vec(), 8u64)]);
        UsedSpaceOf::<Test>::insert(
            net_b,
            who_b,
            UsageTracker {
                last_epoch: 9,
                used_space: 999,
            },
        );

        // Timelocked index contains both nets
        TimelockedIndex::<Test>::mutate(|idx| {
            idx.insert((net_a, who_a1));
            idx.insert((net_a, who_a2));
            idx.insert((net_b, who_b));
        });

        // Sanity pre-checks
        assert!(CommitmentOf::<Test>::get(net_a, who_a1).is_some());
        assert!(CommitmentOf::<Test>::get(net_b, who_b).is_some());
        assert!(TimelockedIndex::<Test>::get().contains(&(net_a, who_a1)));

        // Act
        purge_netuid_with_meter(net_a, Weight::from_parts(u64::MAX, u64::MAX));

        // NET A: everything cleared
        assert_eq!(CommitmentOf::<Test>::iter_prefix(net_a).count(), 0);
        assert!(CommitmentOf::<Test>::get(net_a, who_a1).is_none());
        assert!(CommitmentOf::<Test>::get(net_a, who_a2).is_none());

        assert_eq!(LastCommitment::<Test>::iter_prefix(net_a).count(), 0);
        assert!(LastCommitment::<Test>::get(net_a, who_a1).is_none());
        assert!(LastCommitment::<Test>::get(net_a, who_a2).is_none());

        assert_eq!(LastBondsReset::<Test>::iter_prefix(net_a).count(), 0);
        assert!(LastBondsReset::<Test>::get(net_a, who_a1).is_none());

        assert_eq!(RevealedCommitments::<Test>::iter_prefix(net_a).count(), 0);
        assert!(RevealedCommitments::<Test>::get(net_a, who_a1).is_none());

        assert_eq!(UsedSpaceOf::<Test>::iter_prefix(net_a).count(), 0);
        assert!(UsedSpaceOf::<Test>::get(net_a, who_a1).is_none());

        let idx_after = TimelockedIndex::<Test>::get();
        assert!(!idx_after.contains(&(net_a, who_a1)));
        assert!(!idx_after.contains(&(net_a, who_a2)));

        // NET B: untouched
        assert!(CommitmentOf::<Test>::get(net_b, who_b).is_some());
        assert!(LastCommitment::<Test>::get(net_b, who_b).is_some());
        assert!(LastBondsReset::<Test>::get(net_b, who_b).is_some());
        assert!(RevealedCommitments::<Test>::get(net_b, who_b).is_some());
        assert!(UsedSpaceOf::<Test>::get(net_b, who_b).is_some());
        assert!(idx_after.contains(&(net_b, who_b)));

        // Idempotency
        purge_netuid_with_meter(net_a, Weight::from_parts(u64::MAX, u64::MAX));
        assert_eq!(CommitmentOf::<Test>::iter_prefix(net_a).count(), 0);
        assert!(!TimelockedIndex::<Test>::get().contains(&(net_a, who_a1)));
    });
}

/// `purge_netuid` runs weighted prefix clears **before** the timelock-index update. The macro batch
/// sizing uses the meter's **limit** (not accumulated consumption), so maps may already be empty
/// when the weight budget runs out; `done == false` must still mean the timelock index
/// row for this netuid survives until a later call with enough budget.
#[test]
fn purge_netuid_under_budget_may_skip_timelock_update_while_clearing_maps() {
    new_test_ext().execute_with(|| {
        System::<Test>::set_block_number(1);
        let net_a = NetUid::from(77);
        let who_a: u64 = 4001;

        let empty_fields: BoundedVec<Data, <Test as Config>::MaxFields> = BoundedVec::default();
        let info_empty: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: empty_fields,
        };
        let bn = System::<Test>::block_number();
        let reg = Registration {
            deposit: Default::default(),
            block: bn,
            info: info_empty,
        };
        CommitmentOf::<Test>::insert(net_a, who_a, reg);
        LastCommitment::<Test>::insert(net_a, who_a, bn);
        LastBondsReset::<Test>::insert(net_a, who_a, bn);
        RevealedCommitments::<Test>::insert(net_a, who_a, vec![(b"x".to_vec(), 1u64)]);
        UsedSpaceOf::<Test>::insert(
            net_a,
            who_a,
            UsageTracker {
                last_epoch: 1,
                used_space: 1,
            },
        );
        TimelockedIndex::<Test>::mutate(|idx| {
            idx.insert((net_a, who_a));
        });

        let write1 = <Test as frame_system::Config>::DbWeight::get().writes(1);
        // Budget is strictly below one DB write, so the weighted prefix clears inside
        // `purge_netuid` reliably run out of budget and report `done == false`.
        let budget = write1.saturating_sub(Weight::from_parts(1, 1));

        let done = purge_netuid_with_meter(net_a, budget);
        assert!(
            !done,
            "purge_netuid must report not-done when under-budget"
        );
        assert!(
            TimelockedIndex::<Test>::get().contains(&(net_a, who_a)),
            "timelock index is only trimmed after a successful final pass; stale index entries are expected if that write is skipped"
        );

        // Full budget finishes (including timelock index), even if prior pass already cleared maps.
        let done = purge_netuid_with_meter(net_a, Weight::from_parts(u64::MAX, u64::MAX));
        assert!(done);
        assert!(CommitmentOf::<Test>::get(net_a, who_a).is_none());
        assert!(!TimelockedIndex::<Test>::get().contains(&(net_a, who_a)));
    });
}
