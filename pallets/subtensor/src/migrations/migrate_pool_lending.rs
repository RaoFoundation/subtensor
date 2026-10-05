//! One-shot real-custody funding after the price-preserving pool migration.
use crate::weights::WeightInfo;
use crate::{Config, HasMigrationRun, NetworksAdded, Pallet};
use frame_support::{
    traits::{Get, OnRuntimeUpgrade},
    weights::Weight,
};
use sp_std::marker::PhantomData;

pub const MIGRATION_NAME: &[u8] = b"migrate_unreachable_pool_reserve_lending_v1";
pub struct Migration<T>(PhantomData<T>);

impl<T: Config + pallet_lending::Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        let mut weight = T::DbWeight::get().reads(1);
        if HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()) {
            return weight;
        }
        let mut complete = true;
        for (netuid, live) in NetworksAdded::<T>::iter() {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
            if !live || netuid.is_root() {
                continue;
            }
            weight.saturating_accrue(<T as Config>::WeightInfo::fund_lending_reserves());
            if let Err(error) = Pallet::<T>::fund_unreachable_reserves(netuid, true) {
                complete = false;
                log::error!("Reserve lending migration cannot fund {netuid:?}: {error:?}");
            }
        }
        if complete {
            HasMigrationRun::<T>::insert(MIGRATION_NAME.to_vec(), true);
            pallet_lending::Enabled::<T>::put(true);
            weight.saturating_accrue(T::DbWeight::get().writes(2));
        }
        weight
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
        use codec::Encode;
        use subtensor_runtime_common::Token;
        use subtensor_swap_interface::SwapHandler;
        let pools: sp_std::vec::Vec<_> = NetworksAdded::<T>::iter()
            .filter(|(n, live)| *live && !n.is_root())
            .map(|(n, _)| {
                (
                    n,
                    crate::SubnetTAO::<T>::get(n).to_u64(),
                    crate::SubnetAlphaIn::<T>::get(n).to_u64(),
                    T::SwapInterface::extracted_tao(n).to_u64(),
                )
            })
            .collect();
        Ok((HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()), pools).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        use codec::Decode;
        use frame_support::ensure;
        use pallet_lending::LendingInterface;
        use subtensor_runtime_common::{NetUid, Token};
        use subtensor_swap_interface::SwapHandler;
        let (already_ran, pools): (bool, sp_std::vec::Vec<(NetUid, u64, u64, u64)>) =
            Decode::decode(&mut &state[..]).map_err(|_| "reserve funding snapshot must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
            "every reserve funding candidate must succeed"
        );
        if already_ran {
            return Ok(());
        }
        for (netuid, before_tao, before_alpha, before_extracted_tao) in pools {
            let extracted = T::SwapInterface::extracted_tao(netuid)
                .to_u64()
                .checked_sub(before_extracted_tao)
                .ok_or("extraction counter must increase")?;
            ensure!(
                crate::SubnetTAO::<T>::get(netuid)
                    .to_u64()
                    .checked_add(extracted)
                    == Some(before_tao),
                "active plus extracted TAO must be conserved"
            );
            if let Some(vault) = pallet_lending::Vaults::<T>::get(netuid) {
                ensure!(
                    vault.outstanding_alpha == 0
                        && vault.outstanding_tao == 0
                        && vault.lost_alpha == 0
                        && vault.lost_tao == 0,
                    "migration must not issue loans or create losses"
                );
                ensure!(
                    crate::SubnetAlphaIn::<T>::get(netuid)
                        .to_u64()
                        .checked_add(vault.available_alpha)
                        == Some(before_alpha),
                    "active plus funded alpha must be conserved"
                );
                let (account, hotkey) = T::LendingInterface::custody_accounts(netuid)
                    .ok_or("funded custody must resolve")?;
                ensure!(
                    Pallet::<T>::get_coldkey_balance(&account).to_u64() >= vault.available_tao,
                    "TAO vault must be physically funded"
                );
                ensure!(
                    Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey, &account, netuid
                    )
                    .to_u64()
                        >= vault.available_alpha,
                    "alpha vault must be physically funded"
                );
            }
        }
        Ok(())
    }
}
