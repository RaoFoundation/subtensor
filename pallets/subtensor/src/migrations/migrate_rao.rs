use alloc::{format, string::String};

use frame_support::IterableStorageMap;
use frame_support::{traits::Get, weights::Weight};
use subtensor_runtime_common::{AlphaBalance, NetUid};

use super::*;

pub fn migrate_rao<T: Config>() -> Weight {
    let migration_name = b"migrate_rao".to_vec();

    // Initialize the weight with one read operation.
    let mut weight = T::DbWeight::get().reads(1);

    // Check if the migration has already run
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

    let netuids: Vec<NetUid> = <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();
    weight = weight.saturating_add(T::DbWeight::get().reads_writes(netuids.len() as u64, 0));

    // Migrate all TAO to root.
    // This migration has already run, leaving this only for reference for now, since this is a recent migration
    // Stake::<T>::iter().for_each(|(hotkey, coldkey, stake)| {
    //     // Increase SubnetTAO on root.
    //     SubnetTAO::<T>::mutate(0, |total| {
    //         *total = total.saturating_add(stake);
    //     });
    //     // Increase SubnetAlphaOut on root.
    //     SubnetAlphaOut::<T>::mutate(0, |total| {
    //         *total = total.saturating_add(stake);
    //     });
    //     // Set all the stake on root 0 subnet.
    //     Alpha::<T>::mutate((hotkey.clone(), coldkey.clone(), 0), |total| {
    //         *total = total.saturating_add(U64F64::saturating_from_num(stake))
    //     });
    //     TotalHotkeyShares::<T>::mutate(hotkey.clone(), 0, |total| {
    //         *total = total.saturating_add(U64F64::saturating_from_num(stake))
    //     });
    //     // Set the total stake on the hotkey
    //     TotalHotkeyAlpha::<T>::mutate(hotkey.clone(), 0, |total| {
    //         *total = total.saturating_add(stake)
    //     });
    //     // 6 reads and 6 writes.
    //     weight = weight.saturating_add(T::DbWeight::get().reads_writes(6, 6));
    // });

    // Convert subnets and give them lock.
    // Set global weight to 18% from the start
    // Set min lock
    NetworkMinLockCost::<T>::set(TaoBalance::from(1_000_000_000));
    // Set tao weight.
    TaoWeight::<T>::set(3_320_413_933_267_719_290);
    for netuid in netuids.iter() {
        if netuid.is_root() {
            // Give root a single RAO in pool to avoid any catestrophic division by zero.
            SubnetAlphaIn::<T>::insert(netuid, AlphaBalance::from(1_000_000_000));
            SubnetMechanism::<T>::insert(netuid, 0); // Set to zero mechanism.
            TokenSymbol::<T>::insert(netuid, Pallet::<T>::get_symbol_for_subnet(NetUid::ROOT));
            continue;
        }
        let owner = SubnetOwner::<T>::get(netuid);
        let lock = SubnetLocked::<T>::get(netuid);

        // Put initial TAO from lock into subnet TAO and produce numerically equal amount of Alpha
        // The initial TAO is the locked amount, with a minimum of 1 RAO and a cap of 100 TAO.
        let pool_initial_tao = Pallet::<T>::get_network_min_lock();
        if lock < pool_initial_tao {
            let difference = pool_initial_tao.saturating_sub(lock);
            TotalIssuance::<T>::mutate(|total| {
                *total = total.saturating_add(difference);
            });
        }

        let remaining_lock = lock.saturating_sub(pool_initial_tao);
        // Refund the owner for the remaining lock.
        // SubnetMovingPrice::<T>::insert(
        //     netuid,
        //     I96F32::from_num(EmissionValues::<T>::get(netuid))
        //         .checked_div(I96F32::from_num(1_000_000_000))
        //         .unwrap_or(I96F32::from_num(0.0)),
        // );

        // This code mimics what used to be here previously (add_balance_to_coldkey_account) as
        // close as reasonably possible.
        let credit = Pallet::<T>::mint_tao(remaining_lock.into());
        let _ = Pallet::<T>::spend_tao(&owner, credit, remaining_lock.into());

        SubnetLocked::<T>::insert(netuid, TaoBalance::ZERO); // Clear lock amount.
        SubnetTAO::<T>::insert(netuid, pool_initial_tao);
        TotalStake::<T>::mutate(|total| {
            *total = total.saturating_add(pool_initial_tao);
        }); // Increase total stake.
        SubnetAlphaIn::<T>::insert(netuid, AlphaBalance::from(pool_initial_tao.to_u64())); // Set initial alpha to pool initial tao.
        SubnetAlphaOut::<T>::insert(netuid, AlphaBalance::ZERO); // Set zero subnet alpha out.
        SubnetMechanism::<T>::insert(netuid, 1); // Convert to dynamic immediately with initialization.

        // Set the token symbol for this subnet using Self instead of Pallet::<T>
        TokenSymbol::<T>::insert(netuid, Pallet::<T>::get_symbol_for_subnet(*netuid));

        if let Ok(owner_coldkey) = SubnetOwner::<T>::try_get(netuid) {
            // Set Owner as the coldkey.
            SubnetOwnerHotkey::<T>::insert(netuid, owner_coldkey.clone());
            // Associate the coldkey to coldkey. The function only fails if hotkey is a system
            // account, which is never the case in this migration. Hence, the result can be ignored.
            let _ = Pallet::<T>::create_account_if_non_existent(&owner_coldkey, &owner_coldkey);

            // Only register the owner coldkey if it's not already a hotkey on the subnet.
            if !Uids::<T>::contains_key(*netuid, &owner_coldkey) {
                // Register the owner_coldkey as neuron to the network.
                //let _neuron_uid: u16 = Pallet::<T>::register_neuron(*netuid, &owner_coldkey);
            }
            // Register the neuron immediately.
            if !IdentitiesV2::<T>::contains_key(owner_coldkey.clone()) {
                // Set the identitiy for the Owner coldkey if non existent.
                let identity = ChainIdentityOfV2 {
                    name: format!("Owner{netuid}").as_bytes().to_vec(),
                    url: Vec::new(),
                    image: Vec::new(),
                    github_repo: Vec::new(),
                    discord: Vec::new(),
                    description: Vec::new(),
                    additional: Vec::new(),
                };
                // Validate the created identity and set it.
                if Pallet::<T>::is_valid_identity(&identity) {
                    IdentitiesV2::<T>::insert(owner_coldkey.clone(), identity.clone());
                }
            }
        }

        // HotkeyEmissionTempo::<T>::put(30); // same as subnet tempo // (DEPRECATED)
        // Set the target stakes per interval to 10.
        // TargetStakesPerInterval::<T>::put(10); (DEPRECATED)
    }

    // update `TotalIssuance`, because currency issuance (`T::Currency`) has changed due to lock
    // refunds above
    weight = weight.saturating_add(migrate_init_total_issuance::migrate_init_total_issuance::<T>());

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed. Storage version set to 7.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}
