use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_std::vec::Vec;

/// Backfill the reverse index `AssociatedUidsByEvmAddress` from the existing
/// `AssociatedEvmAddress` forward map. One-time, idempotent, guarded by `HasMigrationRun`.
///
/// This scans the whole forward map in a single block. That is safe because the map is tiny: it
/// grows only through `do_associate_evm_key`, an opt-in, signature-gated, rate-limited extrinsic.
/// Measured on 2026-07-07, the entire map holds **100 entries on Finney and 20 on testnet**, with a
/// largest single-`(netuid, evm_key)` bucket of **3** (against the cap of 32). There is no realistic
/// chain state in which this scan is expensive, so no chunked / multi-block migration is warranted.
pub fn migrate_associated_evm_address_index<T: Config>() -> Weight {
    let migration_name = b"migrate_associated_evm_address_index".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let mut migrated = 0_u64;

    // Forward-map entries whose address bucket is already full and therefore cannot be represented
    // in the bounded reverse index. Collected here and pruned from the forward map after the scan
    // so both maps agree on the pallet's cap (see the reconciliation loop below).
    let mut overflow = Vec::new();

    for (netuid, uid, (evm_key, block_associated)) in AssociatedEvmAddress::<T>::iter() {
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        let mut overflowed = false;
        AssociatedUidsByEvmAddress::<T>::mutate(netuid, evm_key, |uids| {
            if let Some((_, stored_block)) =
                uids.iter_mut().find(|(stored_uid, _)| *stored_uid == uid)
            {
                *stored_block = block_associated;
                return;
            }

            if uids.try_push((uid, block_associated)).is_err() {
                overflowed = true;
            }
        });
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        if overflowed {
            overflow.push((netuid, uid));
        } else {
            migrated = migrated.saturating_add(1);
        }
    }

    // Reconcile over-cap buckets. An address that already holds
    // `MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS` UIDs cannot index any further ones. Leaving those extra
    // UIDs in the forward map would make the two maps disagree: `uid_lookup` would silently miss
    // them, and the capacity check would see a full bucket and refuse to let them refresh — so they
    // could never recover. Instead we drop the excess from the forward map too, so both maps agree
    // on the cap the pallet now enforces; a dropped UID can re-associate later, reusing a freed
    // slot. This branch is unreachable for any real chain state (observed peak reuse of a single
    // address is 3, far below the cap); it exists so the migration can never silently produce an
    // inconsistent index.
    for (netuid, uid) in &overflow {
        AssociatedEvmAddress::<T>::remove(*netuid, *uid);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
        log::warn!(
            "migrate_associated_evm_address_index: dropped over-cap association (netuid={netuid:?}, uid={uid}) to keep the forward map and reverse index consistent"
        );
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. {} associations indexed, {} over-cap associations dropped.",
        String::from_utf8_lossy(&migration_name),
        migrated,
        overflow.len(),
    );

    weight
}
