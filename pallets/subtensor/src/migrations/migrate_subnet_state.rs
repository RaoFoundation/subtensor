use super::*;
use frame_support::{traits::Get, weights::Weight};
use sp_std::collections::btree_map::BTreeMap;

const MIGRATION_NAME: &[u8] = b"backfill_subnet_lifecycle_state_v1";

/// Backfills the reporting-only subnet lifecycle map from the existing operational state.
pub fn migrate_subnet_state<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    let mut expected = BTreeMap::<NetUid, SubnetLifecycleState>::new();
    for (netuid, added) in NetworksAdded::<T>::iter() {
        weight.saturating_accrue(T::DbWeight::get().reads(1));
        if !added {
            continue;
        }

        let state = if netuid == NetUid::ROOT {
            SubnetLifecycleState::Started
        } else {
            let started = FirstEmissionBlockNumber::<T>::contains_key(netuid)
                || SubtokenEnabled::<T>::get(netuid);
            weight.saturating_accrue(T::DbWeight::get().reads(2));
            if started {
                SubnetLifecycleState::Started
            } else {
                SubnetLifecycleState::Registered
            }
        };

        expected.insert(netuid, state);
    }

    let queued = DissolveCleanupQueue::<T>::get();
    weight.saturating_accrue(T::DbWeight::get().reads(1));
    for netuid in queued {
        expected.insert(netuid, SubnetLifecycleState::PendingDissolution);
    }

    if let Some(status) = CurrentDissolveCleanupStatus::<T>::get() {
        expected.insert(status.netuid, SubnetLifecycleState::Dissolving);
    }
    weight.saturating_accrue(T::DbWeight::get().reads(1));

    for (netuid, state) in &expected {
        SubnetState::<T>::insert(netuid, state);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
    }

    // Validate the complete expected set before making the migration idempotency marker durable.
    // `OptionQuery` guarantees a netuid has at most one state; equality here proves every active,
    // queued, or in-progress subnet has exactly the state selected above.
    let valid = expected
        .iter()
        .all(|(netuid, state)| SubnetState::<T>::get(netuid).as_ref() == Some(state));
    weight.saturating_accrue(T::DbWeight::get().reads(expected.len() as u64));
    if !valid {
        log::error!("subnet lifecycle migration validation failed");
        return weight;
    }

    HasMigrationRun::<T>::insert(migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    weight
}
