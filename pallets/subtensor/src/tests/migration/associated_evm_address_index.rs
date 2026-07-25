#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! AssociatedEvmAddress index + orphan subnet identity cleanup.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_associated_evm_address_index() {
    new_test_ext(1).execute_with(|| {
        let migration_name = b"migrate_associated_evm_address_index".to_vec();
        let netuid = NetUid::from(1);
        let other_netuid = NetUid::from(2);
        let evm_key = H160::repeat_byte(1);
        let other_evm_key = H160::repeat_byte(2);

        HasMigrationRun::<Test>::remove(&migration_name);
        AssociatedUidsByEvmAddress::<Test>::remove(netuid, evm_key);
        AssociatedUidsByEvmAddress::<Test>::remove(other_netuid, other_evm_key);

        AssociatedEvmAddress::<Test>::insert(netuid, 0, (evm_key, 10));
        AssociatedEvmAddress::<Test>::insert(netuid, 1, (evm_key, 11));
        AssociatedEvmAddress::<Test>::insert(other_netuid, 0, (other_evm_key, 12));

        crate::migrations::migrate_associated_evm_address_index::migrate_associated_evm_address_index::<Test>();

        assert_eq!(
            AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_key).into_inner(),
            vec![(0, 10), (1, 11)]
        );
        assert_eq!(
            AssociatedUidsByEvmAddress::<Test>::get(other_netuid, other_evm_key).into_inner(),
            vec![(0, 12)]
        );
        assert!(HasMigrationRun::<Test>::get(&migration_name));
    });
}

#[test]
fn test_migrate_clear_orphan_subnet_identities_v3() {
    new_test_ext(1).execute_with(|| {
        let migration_name = b"migrate_clear_orphan_subnet_identities_v3".to_vec();
        HasMigrationRun::<Test>::remove(&migration_name);

        let orphan_netuid = NetUid::from(1);
        let live_netuid = NetUid::from(2);

        // live_netuid is a registered network; orphan_netuid is not.
        NetworksAdded::<Test>::insert(live_netuid, true);

        let orphan_identity = SubnetIdentityV3 {
            subnet_name: b"orphan".to_vec(),
            ..Default::default()
        };
        let live_identity = SubnetIdentityV3 {
            subnet_name: b"live".to_vec(),
            ..Default::default()
        };

        SubnetIdentitiesV3::<Test>::insert(orphan_netuid, orphan_identity);
        SubnetIdentitiesV3::<Test>::insert(live_netuid, live_identity.clone());

        crate::migrations::migrate_clear_orphan_subnet_identities_v3::migrate_clear_orphan_subnet_identities_v3::<Test>();

        // The orphan identity is removed; the live subnet identity is preserved.
        assert!(!SubnetIdentitiesV3::<Test>::contains_key(orphan_netuid));
        assert_eq!(
            SubnetIdentitiesV3::<Test>::get(live_netuid),
            Some(live_identity.clone())
        );

        // Migration is marked as run.
        assert!(HasMigrationRun::<Test>::get(&migration_name));

        // Idempotent: re-running is a no-op (live identity still present).
        crate::migrations::migrate_clear_orphan_subnet_identities_v3::migrate_clear_orphan_subnet_identities_v3::<Test>();
        assert_eq!(
            SubnetIdentitiesV3::<Test>::get(live_netuid),
            Some(live_identity)
        );
    });
}

#[test]
fn test_migrate_associated_evm_address_index_reconciles_over_cap_buckets() {
    new_test_ext(1).execute_with(|| {
        let migration_name = b"migrate_associated_evm_address_index".to_vec();
        let netuid = NetUid::from(1);
        let evm_key = H160::repeat_byte(1);

        HasMigrationRun::<Test>::remove(&migration_name);
        AssociatedUidsByEvmAddress::<Test>::remove(netuid, evm_key);

        // Seed more forward-map associations for a single address than the reverse index can hold.
        let cap = MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS;
        let total = cap + 8;
        for uid in 0..total {
            AssociatedEvmAddress::<Test>::insert(netuid, uid as u16, (evm_key, 100 + uid as u64));
        }

        crate::migrations::migrate_associated_evm_address_index::migrate_associated_evm_address_index::<Test>();

        // The reverse index is bounded by the cap.
        let bucket = AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_key);
        assert_eq!(bucket.len() as u32, cap);

        // The forward map was pruned to match, so the two maps agree on the cap: every remaining
        // forward entry is present in the reverse index, and there are no extras on either side.
        let forward: Vec<u16> = AssociatedEvmAddress::<Test>::iter_prefix(netuid)
            .map(|(uid, _)| uid)
            .collect();
        assert_eq!(forward.len() as u32, cap);
        for uid in &forward {
            assert!(
                bucket.iter().any(|(stored_uid, _)| stored_uid == uid),
                "forward uid {uid} missing from reverse index"
            );
        }
        for (uid, _) in bucket.iter() {
            assert!(
                forward.contains(uid),
                "reverse uid {uid} missing from forward map"
            );
        }

        assert!(HasMigrationRun::<Test>::get(&migration_name));
    });
}
