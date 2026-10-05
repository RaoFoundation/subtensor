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

#[cfg(feature = "try-runtime")]
#[derive(codec::Encode, codec::Decode)]
struct PoolSnapshot {
    netuid: subtensor_runtime_common::NetUid,
    tao: u64,
    alpha: u64,
    extracted_alpha: u64,
    extracted_tao: u64,
    vault: Option<pallet_lending::Vault>,
    curve: [u8; 32],
    extractable: bool,
}

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
                let (extracted_alpha, extracted_tao, curve, extractable) =
                    T::SwapInterface::reserve_funding_state(n)?;
                let tao = crate::SubnetTAO::<T>::get(n).to_u64();
                let alpha = crate::SubnetAlphaIn::<T>::get(n).to_u64();
                Ok(PoolSnapshot {
                    netuid: n,
                    tao,
                    alpha,
                    extracted_alpha: extracted_alpha.to_u64(),
                    extracted_tao: extracted_tao.to_u64(),
                    vault: pallet_lending::Vaults::<T>::get(n),
                    curve,
                    extractable,
                })
            })
            .collect::<Result<_, sp_runtime::TryRuntimeError>>()?;
        Ok((HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()), pools).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        use codec::Decode;
        use frame_support::ensure;
        use pallet_lending::LendingInterface;
        use subtensor_runtime_common::Token;
        use subtensor_swap_interface::SwapHandler;
        let (already_ran, pools): (bool, sp_std::vec::Vec<PoolSnapshot>) =
            Decode::decode(&mut &state[..]).map_err(|_| "reserve funding snapshot must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
            "every admitted reserve funding candidate must succeed"
        );
        let count = pallet_lending::VaultCount::<T>::get();
        ensure!(
            count <= <T as pallet_lending::Config>::MaxFundedSubnets::get(),
            "funded vault count must stay within the processing bound"
        );
        ensure!(
            usize::try_from(count).ok() == Some(pallet_lending::Vaults::<T>::iter_keys().count()),
            "funded vault count must match stored vaults"
        );
        if already_ran {
            return Ok(());
        }
        ensure!(
            pallet_lending::Enabled::<T>::get(),
            "successful bounded initialization must enable lending"
        );
        let full = count == <T as pallet_lending::Config>::MaxFundedSubnets::get();
        for before in pools {
            let netuid = before.netuid;
            let (extracted_alpha, extracted_tao, curve, _) =
                T::SwapInterface::reserve_funding_state(netuid)?;
            let tao_delta = extracted_tao
                .to_u64()
                .checked_sub(before.extracted_tao)
                .ok_or("extraction counter must increase")?;
            let alpha_delta = extracted_alpha
                .to_u64()
                .checked_sub(before.extracted_alpha)
                .ok_or("extraction counter must increase")?;
            ensure!(
                crate::SubnetTAO::<T>::get(netuid)
                    .to_u64()
                    .checked_add(tao_delta)
                    == Some(before.tao),
                "active plus extracted TAO must be conserved"
            );
            ensure!(
                crate::SubnetAlphaIn::<T>::get(netuid)
                    .to_u64()
                    .checked_add(alpha_delta)
                    == Some(before.alpha),
                "active plus extracted alpha must be conserved"
            );
            let vault = pallet_lending::Vaults::<T>::get(netuid);
            if let Some(previous) = before.vault {
                // A retry may encounter vaults funded before an unrelated pool failed.
                // Existing inventory and debt are preserved; only new extraction counts.
                ensure!(
                    vault == Some(previous),
                    "existing vault must remain unchanged"
                );
                ensure!(
                    tao_delta == 0 && alpha_delta == 0 && curve == before.curve,
                    "existing vault must not be extracted again"
                );
            } else if let Some(ref funded) = vault {
                ensure!(
                    funded.outstanding_alpha == 0
                        && funded.outstanding_tao == 0
                        && funded.lost_alpha == 0
                        && funded.lost_tao == 0,
                    "migration must not issue loans or create losses"
                );
                ensure!(
                    funded.available_alpha == alpha_delta && funded.available_tao == tao_delta,
                    "new vault inventory must equal real reserve extraction"
                );
            } else {
                ensure!(
                    tao_delta == 0 && alpha_delta == 0 && curve == before.curve,
                    "unfunded pool balances and geometry must remain unchanged"
                );
                ensure!(
                    !before.extractable || full,
                    "only full capacity may defer nonzero reserve funding"
                );
            }
            if let Some(funded) = vault {
                let (account, hotkey) = T::LendingInterface::custody_accounts(netuid)
                    .ok_or("funded custody must resolve")?;
                ensure!(
                    Pallet::<T>::get_coldkey_balance(&account).to_u64() >= funded.available_tao,
                    "TAO vault must be physically funded"
                );
                ensure!(
                    Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey, &account, netuid
                    )
                    .to_u64()
                        >= funded.available_alpha,
                    "alpha vault must be physically funded"
                );
            }
        }
        Ok(())
    }
}
