use crate::{Config, HasMigrationRun, NetworksAdded, SubnetTAO, TotalStake};
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::{TaoBalance, Token};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_resync_total_stake";

/// Set `TotalStake` to the sum of `SubnetTAO` over live subnets.
///
/// `TotalStake` used to grow by the gross TAO of every buy while `SubnetTAO` grew by the
/// TAO that entered the reserve (gross minus the swap fee), so it drifted above the sum
/// of the reserves it summarises. The two now move together; this one-shot resync removes
/// the accumulated difference. `TotalIssuance` is not touched.
pub fn migrate_resync_total_stake<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut reads = 1u64;

    if HasMigrationRun::<T>::get(&migration_name) {
        return T::DbWeight::get().reads(reads);
    }

    let mut total = TaoBalance::ZERO;
    for (netuid, added) in NetworksAdded::<T>::iter() {
        reads = reads.saturating_add(1);
        if added {
            reads = reads.saturating_add(1);
            total = total.saturating_add(SubnetTAO::<T>::get(netuid));
        }
    }

    let previous = TotalStake::<T>::get();
    reads = reads.saturating_add(1);
    TotalStake::<T>::put(total);
    log::info!(
        "migrate_resync_total_stake: TotalStake {previous:?} -> {total:?} (sum of live SubnetTAO)"
    );

    HasMigrationRun::<T>::insert(&migration_name, true);
    T::DbWeight::get().reads_writes(reads, 2)
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper with try-runtime
/// validation, registered in the runtime `Migrations` tuple: after the upgrade `TotalStake`
/// equals the sum of `SubnetTAO` over live subnets, that sum is unchanged by the upgrade,
/// `TotalIssuance` is untouched, and the marker is set.
pub mod resync_total_stake {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use crate::{NetworksAdded, SubnetTAO, TotalIssuance};
    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;
    #[cfg(feature = "try-runtime")]
    use sp_std::vec::Vec;

    #[cfg(feature = "try-runtime")]
    fn live_subnet_tao<T: Config>() -> u64 {
        NetworksAdded::<T>::iter()
            .filter(|(_, added)| *added)
            .map(|(netuid, _)| SubnetTAO::<T>::get(netuid).to_u64())
            .fold(0u64, u64::saturating_add)
    }

    #[cfg(feature = "try-runtime")]
    #[derive(Encode, Decode)]
    struct PreUpgradeState {
        live_subnet_tao: u64,
        total_issuance: u64,
    }

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_resync_total_stake::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            Ok(PreUpgradeState {
                live_subnet_tao: live_subnet_tao::<T>(),
                total_issuance: TotalIssuance::<T>::get().to_u64(),
            }
            .encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "TotalStake resync marker must be set"
            );
            let live = live_subnet_tao::<T>();
            ensure!(
                live == before.live_subnet_tao,
                "resync must not change the subnet reserves it sums"
            );
            ensure!(
                TotalStake::<T>::get().to_u64() == live,
                "TotalStake must equal the sum of live SubnetTAO"
            );
            ensure!(
                TotalIssuance::<T>::get().to_u64() == before.total_issuance,
                "resync must not touch TotalIssuance"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::mock::*;
    use subtensor_runtime_common::NetUid;

    #[test]
    fn migration_resyncs_total_stake_to_live_subnet_tao_once() {
        new_test_ext(1).execute_with(|| {
            let live_a = NetUid::from(1);
            let live_b = NetUid::from(2);
            let dissolving = NetUid::from(3);
            NetworksAdded::<Test>::insert(live_a, true);
            NetworksAdded::<Test>::insert(live_b, true);
            NetworksAdded::<Test>::insert(dissolving, false);
            SubnetTAO::<Test>::insert(live_a, TaoBalance::from(1_000_u64));
            SubnetTAO::<Test>::insert(live_b, TaoBalance::from(250_u64));
            SubnetTAO::<Test>::insert(dissolving, TaoBalance::from(99_u64));
            TotalStake::<Test>::put(TaoBalance::from(5_000_u64));

            let weight = migrate_resync_total_stake::<Test>();
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(1_250_u64));
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME.to_vec()));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads_writes(7, 2)
            );

            TotalStake::<Test>::put(TaoBalance::from(7_u64));
            let weight = migrate_resync_total_stake::<Test>();
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(7_u64));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads(1)
            );
        });
    }
}
