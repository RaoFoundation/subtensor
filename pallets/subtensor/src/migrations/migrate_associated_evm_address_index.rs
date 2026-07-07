use super::*;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_std::vec::Vec;

/// Upper bound on the number of `AssociatedEvmAddress` entries this migration will process in a
/// single `on_runtime_upgrade`.
///
/// `AssociatedEvmAddress` is grown only through `do_associate_evm_key` — an opt-in, signature-gated,
/// rate-limited extrinsic — so in practice it holds at most a handful of entries per subnet. This
/// cap gives the one-shot migration a *verified* bound on the reads/writes it can perform, so a
/// pathologically large map can never turn the upgrade block into unbounded Wasm work. The value is
/// orders of magnitude above any realistic association count while keeping the upgrade block's work
/// safely bounded. If the map ever exceeds it, the migration refuses to mark itself complete and
/// logs an error, rather than silently indexing only a prefix of the map.
const MAX_MIGRATION_ENTRIES: u64 = 50_000;

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
    let mut processed = 0_u64;
    let mut capped = false;

    // Forward-map entries whose address bucket is already full and therefore cannot be represented
    // in the bounded reverse index. Collected here and pruned from the forward map after the scan
    // so both maps agree on the pallet's cap (see the reconciliation loop below).
    let mut overflow = Vec::new();

    for (netuid, uid, (evm_key, block_associated)) in AssociatedEvmAddress::<T>::iter() {
        if processed >= MAX_MIGRATION_ENTRIES {
            capped = true;
            break;
        }
        processed = processed.saturating_add(1);
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
    // address is far below the cap); it exists so the migration can never silently produce an
    // inconsistent index.
    for (netuid, uid) in &overflow {
        AssociatedEvmAddress::<T>::remove(*netuid, *uid);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
        log::warn!(
            "migrate_associated_evm_address_index: dropped over-cap association (netuid={netuid:?}, uid={uid}) to keep the forward map and reverse index consistent"
        );
    }

    if capped {
        log::error!(
            "Migration 'migrate_associated_evm_address_index' hit the {MAX_MIGRATION_ENTRIES}-entry \
             processing cap and was left incomplete. This indicates an unexpectedly large \
             AssociatedEvmAddress map and requires manual attention."
        );
        return weight;
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
