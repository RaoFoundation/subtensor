use crate::{Config, HasMigrationRun, NetworksAdded, SubnetTAO, TotalStake};
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::{TaoBalance, Token};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_resync_total_stake";
/// Second one-shot resync (spec 468): a subnet dissolution on finney (block 9111229, subnet
/// 108) subtracted the fund holdings it converted from `TotalStake` twice, leaving it
/// 26.516982241 TAO below the sum of live `SubnetTAO`. The double subtraction is fixed in
/// `convert_basket_holding_to_root`; this pass removes the drift it already caused.
pub(crate) const MIGRATION_NAME_V2: &[u8] = b"migrate_resync_total_stake_v2";

/// Set `TotalStake` to the sum of `SubnetTAO` over live subnets.
///
/// `TotalStake` used to grow by the gross TAO of every buy while `SubnetTAO` grew by the
/// TAO that entered the reserve (gross minus the swap fee), so it drifted above the sum
/// of the reserves it summarises. The two now move together; this one-shot resync removes
/// the accumulated difference. `TotalIssuance` is not touched.
pub fn migrate_resync_total_stake<T: Config>() -> Weight {
    resync_total_stake_once::<T>(MIGRATION_NAME)
}

/// Spec 468 pass of the same resync, under its own marker (see [`MIGRATION_NAME_V2`]).
pub fn migrate_resync_total_stake_v2<T: Config>() -> Weight {
    resync_total_stake_once::<T>(MIGRATION_NAME_V2)
}

/// Sum of `SubnetTAO` over live subnets: the value `TotalStake` summarises.
pub fn live_subnet_tao<T: Config>() -> (TaoBalance, u64) {
    let mut total = TaoBalance::ZERO;
    let mut reads = 0u64;
    for (netuid, added) in NetworksAdded::<T>::iter() {
        reads = reads.saturating_add(1);
        if added {
            reads = reads.saturating_add(1);
            total = total.saturating_add(SubnetTAO::<T>::get(netuid));
        }
    }
    (total, reads)
}

fn resync_total_stake_once<T: Config>(name: &[u8]) -> Weight {
    let migration_name = name.to_vec();
    let mut reads = 1u64;

    if HasMigrationRun::<T>::get(&migration_name) {
        return T::DbWeight::get().reads(reads);
    }

    let (total, sum_reads) = live_subnet_tao::<T>();
    reads = reads.saturating_add(sum_reads);

    let previous = TotalStake::<T>::get();
    reads = reads.saturating_add(1);
    TotalStake::<T>::put(total);
    log::info!(
        "{}: TotalStake {previous:?} -> {total:?} (sum of live SubnetTAO)",
        core::str::from_utf8(name).unwrap_or("migrate_resync_total_stake")
    );

    HasMigrationRun::<T>::insert(&migration_name, true);
    T::DbWeight::get().reads_writes(reads, 2)
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrappers with try-runtime
/// validation, registered in the runtime `Migrations` tuple. `pre_upgrade` records whether
/// the pass still had to run; `post_upgrade` asserts `TotalStake == Σ live SubnetTAO` **only
/// when this pass ran during the upgrade** — a stamped, no-op pass must never fail
/// try-runtime because the chain drifted again since it ran (which is exactly what the 462
/// pass did on the 2026-09-21 mainnet snapshot). In every case the sum is unchanged by the
/// upgrade, `TotalIssuance` is untouched, and the marker is set.
mod resync_wrapper {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use crate::TotalIssuance;
    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;
    #[cfg(feature = "try-runtime")]
    use sp_std::vec::Vec;

    /// Which resync pass a wrapper drives.
    pub trait Pass {
        const NAME: &'static [u8];
        fn run<T: Config>() -> Weight;
    }

    #[cfg(feature = "try-runtime")]
    #[derive(Encode, Decode)]
    struct PreUpgradeState {
        live_subnet_tao: u64,
        total_issuance: u64,
        already_ran: bool,
    }

    pub struct Migration<T: Config, P: Pass>(PhantomData<(T, P)>);

    impl<T: Config, P: Pass> OnRuntimeUpgrade for Migration<T, P> {
        fn on_runtime_upgrade() -> Weight {
            P::run::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            Ok(PreUpgradeState {
                live_subnet_tao: live_subnet_tao::<T>().0.to_u64(),
                total_issuance: TotalIssuance::<T>::get().to_u64(),
                already_ran: HasMigrationRun::<T>::get(P::NAME.to_vec()),
            }
            .encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            ensure!(
                HasMigrationRun::<T>::get(P::NAME.to_vec()),
                "TotalStake resync marker must be set"
            );
            let live = live_subnet_tao::<T>().0.to_u64();
            ensure!(
                live == before.live_subnet_tao,
                "resync must not change the subnet reserves it sums"
            );
            if !before.already_ran {
                ensure!(
                    TotalStake::<T>::get().to_u64() == live,
                    "TotalStake must equal the sum of live SubnetTAO"
                );
            }
            ensure!(
                TotalIssuance::<T>::get().to_u64() == before.total_issuance,
                "resync must not touch TotalIssuance"
            );
            Ok(())
        }
    }
}

/// The spec 462 pass.
pub mod resync_total_stake {
    use super::*;

    pub struct V1;
    impl resync_wrapper::Pass for V1 {
        const NAME: &'static [u8] = MIGRATION_NAME;
        fn run<T: Config>() -> Weight {
            migrate_resync_total_stake::<T>()
        }
    }
    pub type Migration<T> = resync_wrapper::Migration<T, V1>;
}

/// The spec 468 pass.
pub mod resync_total_stake_v2 {
    use super::*;

    pub struct V2;
    impl resync_wrapper::Pass for V2 {
        const NAME: &'static [u8] = MIGRATION_NAME_V2;
        fn run<T: Config>() -> Weight {
            migrate_resync_total_stake_v2::<T>()
        }
    }
    pub type Migration<T> = resync_wrapper::Migration<T, V2>;
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

    /// Spec 468: the second pass runs under its own marker, resyncs once, and no-ops after,
    /// independently of the 462 marker.
    #[test]
    fn v2_resyncs_once_under_its_own_marker() {
        new_test_ext(1).execute_with(|| {
            let live = NetUid::from(1);
            NetworksAdded::<Test>::insert(live, true);
            SubnetTAO::<Test>::insert(live, TaoBalance::from(1_000_u64));
            // The 462 pass already ran on chain.
            HasMigrationRun::<Test>::insert(MIGRATION_NAME.to_vec(), true);
            // Finney's drift: TotalStake below the live sum.
            TotalStake::<Test>::put(TaoBalance::from(973_u64));

            assert_eq!(
                migrate_resync_total_stake::<Test>(),
                <Test as frame_system::Config>::DbWeight::get().reads(1),
                "the stamped 462 pass is a no-op"
            );
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(973_u64));

            let weight = migrate_resync_total_stake_v2::<Test>();
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(1_000_u64));
            assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME_V2.to_vec()));
            assert_eq!(
                weight,
                <Test as frame_system::Config>::DbWeight::get().reads_writes(4, 2)
            );

            TotalStake::<Test>::put(TaoBalance::from(5_u64));
            assert_eq!(
                migrate_resync_total_stake_v2::<Test>(),
                <Test as frame_system::Config>::DbWeight::get().reads(1)
            );
            assert_eq!(TotalStake::<Test>::get(), TaoBalance::from(5_u64));
        });
    }
}
