use super::*;
use frame_support::{traits::Get, weights::Weight};
use sp_std::collections::btree_map::BTreeMap;

pub(crate) const MIGRATION_NAME: &[u8] = b"backfill_subnet_lifecycle_state_v1";

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

    HasMigrationRun::<T>::insert(migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    weight
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper for
/// [`migrate_subnet_state`]. Its try-runtime hooks snapshot the lifecycle implied by the
/// legacy operational storage before the upgrade, then require the complete reporting map
/// and migration marker afterward.
pub mod subnet_state {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;
    #[cfg(feature = "try-runtime")]
    use sp_std::vec::Vec;

    #[cfg(feature = "try-runtime")]
    #[derive(Encode, Decode)]
    struct PreUpgradeState {
        expected: Vec<(NetUid, SubnetLifecycleState)>,
    }

    #[cfg(feature = "try-runtime")]
    fn expected_lifecycle_state<T: Config>() -> BTreeMap<NetUid, SubnetLifecycleState> {
        let mut expected = BTreeMap::new();
        for (netuid, added) in NetworksAdded::<T>::iter() {
            if !added {
                continue;
            }
            let state = if netuid == NetUid::ROOT
                || FirstEmissionBlockNumber::<T>::contains_key(netuid)
                || SubtokenEnabled::<T>::get(netuid)
            {
                SubnetLifecycleState::Started
            } else {
                SubnetLifecycleState::Registered
            };
            expected.insert(netuid, state);
        }
        for netuid in DissolveCleanupQueue::<T>::get() {
            expected.insert(netuid, SubnetLifecycleState::PendingDissolution);
        }
        if let Some(status) = CurrentDissolveCleanupStatus::<T>::get() {
            expected.insert(status.netuid, SubnetLifecycleState::Dissolving);
        }
        expected
    }

    #[cfg(feature = "try-runtime")]
    fn validate_lifecycle_state<T: Config>(
        expected: BTreeMap<NetUid, SubnetLifecycleState>,
    ) -> Result<(), TryRuntimeError> {
        let actual: BTreeMap<NetUid, SubnetLifecycleState> = SubnetState::<T>::iter().collect();
        ensure!(
            actual == expected,
            "SubnetState must exactly match the expected lifecycle state"
        );
        Ok(())
    }

    /// Validate the live lifecycle invariant after every migration in the upgrade has run.
    #[cfg(feature = "try-runtime")]
    pub(crate) fn try_state<T: Config>() -> Result<(), TryRuntimeError> {
        validate_lifecycle_state::<T>(expected_lifecycle_state::<T>())
    }

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_subnet_state::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            // Deliberately derive this independently of the migration's `expected` map: the
            // check must compare the old and new representations rather than reuse migration
            // output and merely read it back.
            Ok(PreUpgradeState {
                expected: expected_lifecycle_state::<T>().into_iter().collect(),
            }
            .encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "subnet lifecycle migration marker must be set"
            );
            let expected: BTreeMap<NetUid, SubnetLifecycleState> =
                before.expected.into_iter().collect();
            validate_lifecycle_state::<T>(expected)
        }
    }
}

#[cfg(all(test, feature = "try-runtime"))]
mod try_runtime_tests {
    use super::*;
    use crate::{
        subnets::dissolution::DissolveCleanupStatus,
        tests::mock::{Test, new_test_ext},
    };
    use frame_support::traits::OnRuntimeUpgrade;

    type Migration = subnet_state::Migration<Test>;

    #[test]
    fn post_upgrade_rejects_missing_incorrect_and_extra_lifecycle_entries()
    -> Result<(), sp_runtime::TryRuntimeError> {
        new_test_ext(1).execute_with(|| {
            let registered = NetUid::from(100);
            let queued = NetUid::from(101);
            let dissolving = NetUid::from(102);
            let extra = NetUid::from(103);
            let started = NetUid::from(104);

            NetworksAdded::<Test>::insert(registered, true);
            NetworksAdded::<Test>::insert(started, true);
            FirstEmissionBlockNumber::<Test>::insert(started, 10);
            DissolveCleanupQueue::<Test>::set(vec![queued, dissolving]);
            CurrentDissolveCleanupStatus::<Test>::set(Some(DissolveCleanupStatus::new(dissolving)));

            // Simulate the old runtime, which did not populate the reporting map.
            let existing: Vec<_> = SubnetState::<Test>::iter_keys().collect();
            for netuid in existing {
                SubnetState::<Test>::remove(netuid);
            }
            HasMigrationRun::<Test>::remove(MIGRATION_NAME.to_vec());

            let pre_upgrade = Migration::pre_upgrade()?;
            Migration::on_runtime_upgrade();
            assert!(Migration::post_upgrade(pre_upgrade.clone()).is_ok());
            assert!(subnet_state::try_state::<Test>().is_ok());

            SubnetState::<Test>::remove(registered);
            assert!(
                Migration::post_upgrade(pre_upgrade.clone()).is_err(),
                "a missing lifecycle entry must fail validation"
            );
            assert!(subnet_state::try_state::<Test>().is_err());

            SubnetState::<Test>::insert(registered, SubnetLifecycleState::PendingDissolution);
            assert!(
                Migration::post_upgrade(pre_upgrade.clone()).is_err(),
                "an incorrectly classified lifecycle entry must fail validation"
            );
            assert!(subnet_state::try_state::<Test>().is_err());

            SubnetState::<Test>::insert(registered, SubnetLifecycleState::Registered);
            SubnetState::<Test>::insert(extra, SubnetLifecycleState::Registered);
            assert!(
                Migration::post_upgrade(pre_upgrade.clone()).is_err(),
                "an extra lifecycle entry must fail validation"
            );
            assert!(subnet_state::try_state::<Test>().is_err());

            SubnetState::<Test>::remove(extra);
            assert!(subnet_state::try_state::<Test>().is_ok());
            HasMigrationRun::<Test>::remove(MIGRATION_NAME.to_vec());
            assert!(
                Migration::post_upgrade(pre_upgrade).is_err(),
                "a missing completion marker must fail validation"
            );
            Ok(())
        })
    }
}
