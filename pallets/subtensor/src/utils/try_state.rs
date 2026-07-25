//! Try-runtime invariants for stake accounting (`try-runtime` feature only).

use super::*;

impl<T: Config> Pallet<T> {
    /// Checks that Σ `SubnetTAO` (minus non-root network min-lock) equals [`TotalStake`].
    #[allow(dead_code)]
    pub(crate) fn check_total_stake() -> Result<(), sp_runtime::TryRuntimeError> {
        // Calculate the total staked amount
        let total_staked = SubnetTAO::<T>::iter().fold(TaoBalance::ZERO, |acc, (netuid, stake)| {
            let acc = acc.saturating_add(stake);

            if netuid.is_root() {
                // root network doesn't have initial pool TAO
                acc
            } else {
                acc.saturating_sub(Self::get_network_min_lock())
            }
        });

        log::warn!(
            "total_staked: {}, TotalStake: {}",
            total_staked,
            TotalStake::<T>::get()
        );

        // Verify that the calculated total stake matches the stored TotalStake
        ensure!(
            total_staked == TotalStake::<T>::get(),
            "TotalStake does not match total staked",
        );

        Ok(())
    }
}
