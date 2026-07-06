use alloc::collections::BTreeMap;
use safe_math::*;
use share_pool::SafeFloat;
use substrate_fixed::types::{U64F64, U96F32};
use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::{Order, SwapHandler};

use super::*;

impl<T: Config> Pallet<T> {
    // Returns true if the passed hotkey allow delegative staking.
    //
    pub fn hotkey_is_delegate(hotkey: &T::AccountId) -> bool {
        Delegates::<T>::contains_key(hotkey)
    }

    // Sets the hotkey as a delegate with take.
    //
    pub fn delegate_hotkey(hotkey: &T::AccountId, take: u16) {
        Delegates::<T>::insert(hotkey, take);
    }

    // Returns the total amount of stake in the staking table.
    //
    pub fn get_total_stake() -> TaoBalance {
        TotalStake::<T>::get()
    }

    // Increases the total amount of stake by the passed amount.
    //
    pub fn increase_total_stake(increment: TaoBalance) {
        TotalStake::<T>::put(Self::get_total_stake().saturating_add(increment));
    }

    // Decreases the total amount of stake by the passed amount.
    //
    pub fn decrease_total_stake(decrement: TaoBalance) {
        TotalStake::<T>::put(Self::get_total_stake().saturating_sub(decrement));
    }

    /// Returns the total amount of stake (in TAO) under a hotkey (delegative or otherwise)
    pub fn get_total_stake_for_hotkey(hotkey: &T::AccountId) -> TaoBalance {
        Self::get_all_subnet_netuids()
            .into_iter()
            .map(|netuid| {
                let alpha = U64F64::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(
                    hotkey, netuid,
                ));
                let alpha_price = T::SwapInterface::current_alpha_price(netuid.into());
                alpha.saturating_mul(alpha_price)
            })
            .sum::<U64F64>()
            .saturating_to_num::<u64>()
            .into()
    }

    // Returns the total amount of stake under a coldkey
    //
    pub fn get_total_stake_for_coldkey(coldkey: &T::AccountId) -> TaoBalance {
        let hotkeys = StakingHotkeys::<T>::get(coldkey);
        hotkeys
            .iter()
            .map(|hotkey| {
                Self::alpha_iter_prefix((hotkey, coldkey))
                    .map(|(netuid, _)| {
                        let alpha_stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                            hotkey, coldkey, netuid,
                        );
                        let order = GetTaoForAlpha::<T>::with_amount(alpha_stake);
                        T::SwapInterface::sim_swap(netuid.into(), order)
                            .map(|r| {
                                let fee: u64 = U64F64::saturating_from_num(r.fee_paid)
                                    .saturating_mul(T::SwapInterface::current_alpha_price(
                                        netuid.into(),
                                    ))
                                    .saturating_to_num();
                                r.amount_paid_out.to_u64().saturating_add(fee)
                            })
                            .unwrap_or_default()
                    })
                    .sum::<u64>()
            })
            .sum::<u64>()
            .into()
    }

    // Returns the total amount of stake under a coldkey on a subnet
    //
    pub fn get_total_stake_for_coldkey_on_subnet(
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> TaoBalance {
        let hotkeys = StakingHotkeys::<T>::get(coldkey);
        hotkeys
            .iter()
            .map(|hotkey| {
                Self::alpha_iter_prefix((hotkey, coldkey))
                    .map(|(netuid_on_storage, _)| {
                        if netuid_on_storage == netuid {
                            let alpha_stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                                hotkey, coldkey, netuid,
                            );
                            let order = GetTaoForAlpha::<T>::with_amount(alpha_stake);
                            T::SwapInterface::sim_swap(netuid.into(), order)
                                .map(|r| {
                                    let fee: u64 = U64F64::saturating_from_num(r.fee_paid)
                                        .saturating_mul(T::SwapInterface::current_alpha_price(
                                            netuid.into(),
                                        ))
                                        .saturating_to_num();
                                    r.amount_paid_out.to_u64().saturating_add(fee)
                                })
                                .unwrap_or_default()
                        } else {
                            0
                        }
                    })
                    .sum::<u64>()
            })
            .sum::<u64>()
            .into()
    }

    // Creates a cold - hot pairing account if the hotkey is not already an active account.
    //
    pub fn create_account_if_non_existent(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> DispatchResult {
        // Only allow to register non-system hotkeys
        ensure!(
            Self::is_subnet_account_id(hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );

        if !Self::hotkey_account_exists(hotkey) {
            Owner::<T>::insert(hotkey, coldkey);

            // Update OwnedHotkeys map
            let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
            if !hotkeys.contains(hotkey) {
                hotkeys.push(hotkey.clone());
                OwnedHotkeys::<T>::insert(coldkey, hotkeys);
            }

            // Update StakingHotkeys map
            let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
            if !staking_hotkeys.contains(hotkey) {
                staking_hotkeys.push(hotkey.clone());
                StakingHotkeys::<T>::insert(coldkey, staking_hotkeys);
            }
        }
        Ok(())
    }

    pub fn set_hotkey_owner(coldkey: &T::AccountId, hotkey: &T::AccountId) -> DispatchResult {
        // Only allow to register non-system hotkeys
        ensure!(
            Self::is_subnet_account_id(hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );
        Owner::<T>::insert(hotkey, coldkey);
        Ok(())
    }

    //// If the hotkey is not a delegate, make it a delegate.
    pub fn maybe_become_delegate(hotkey: &T::AccountId) {
        if !Self::hotkey_is_delegate(hotkey) {
            Self::delegate_hotkey(hotkey, Self::get_hotkey_take(hotkey));
        }
    }

    /// Returns the coldkey owning this hotkey. This function should only be called for active accounts.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// The coldkey account ID that owns the hotkey.
    pub fn get_owning_coldkey_for_hotkey(hotkey: &T::AccountId) -> T::AccountId {
        Owner::<T>::get(hotkey)
    }

    /// Returns the hotkey take.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// The take value of the hotkey.
    pub fn get_hotkey_take(hotkey: &T::AccountId) -> u16 {
        Delegates::<T>::get(hotkey)
    }
    pub fn get_hotkey_take_float(hotkey: &T::AccountId) -> U96F32 {
        U96F32::saturating_from_num(Self::get_hotkey_take(hotkey))
            .safe_div(U96F32::saturating_from_num(u16::MAX))
    }

    /// Returns true if the hotkey account has been created.
    ///
    /// # Arguments
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// True if the hotkey account exists, false otherwise.
    pub fn hotkey_account_exists(hotkey: &T::AccountId) -> bool {
        Owner::<T>::contains_key(hotkey)
    }

    /// Returns true if the passed coldkey owns the hotkey.
    ///
    /// # Arguments
    /// * `coldkey` - The coldkey account ID.
    /// * `hotkey` - The hotkey account ID.
    ///
    /// # Returns
    /// True if the coldkey owns the hotkey, false otherwise.
    pub fn coldkey_owns_hotkey(coldkey: &T::AccountId, hotkey: &T::AccountId) -> bool {
        if Self::hotkey_account_exists(hotkey) {
            Owner::<T>::get(hotkey) == *coldkey
        } else {
            false
        }
    }

    /// Clears the nomination for an account, if it is a nominator account and the stake is below the minimum required threshold.
    pub fn clear_small_nomination_if_required(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) {
        // Verify if the account is a nominator account by checking ownership of the hotkey by the coldkey.
        if !Self::coldkey_owns_hotkey(coldkey, hotkey) {
            // If the stake is non-zero and below the minimum required, it's considered a small nomination and needs to be cleared.
            // Log if the stake is below the minimum required
            let alpha_stake =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
            let min_alpha_stake =
                U64F64::saturating_from_num(Self::get_nominator_min_required_stake())
                    .safe_div(T::SwapInterface::current_alpha_price(netuid))
                    .saturating_to_num::<u64>();
            if alpha_stake > 0.into() && alpha_stake < min_alpha_stake.into() {
                // Log the clearing of a small nomination
                // Remove the stake from the nominator account. (this is a more forceful unstake operation which )
                // Actually deletes the staking account.
                // Do not apply any fees
                if Self::unstake_from_subnet(
                    hotkey,
                    coldkey,
                    coldkey,
                    netuid,
                    alpha_stake,
                    T::SwapInterface::min_price(),
                    false,
                )
                .is_err()
                {
                    // Ignore errors if unstaking fails
                    // Just clear small alpha
                    let alpha =
                        Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
                    Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey, coldkey, netuid, alpha,
                    );
                }

                // Reduce lock (if exists) by the cleaned stake amount
                Self::force_reduce_lock(coldkey, netuid, alpha_stake);
            }
        }
    }

    /// Clears small nominations for all accounts.
    ///
    /// WARN: This is an O(N) operation, where N is the number of staking accounts. It should be
    /// used with caution.
    pub fn clear_small_nominations() {
        // Loop through all staking accounts to identify and clear nominations below the minimum stake.
        for ((hotkey, coldkey, netuid), _) in Self::alpha_iter() {
            Self::clear_small_nomination_if_required(&hotkey, &coldkey, netuid);
        }
    }

    /// The function clears Alpha map in batches. Each run will check ALPHA_MAP_BATCH_SIZE
    /// alphas. It keeps the alpha value stored when it's >= than MIN_ALPHA.
    /// The function uses AlphaMapLastKey as a storage for key iterator between runs.
    pub fn populate_root_coldkey_staking_maps() {
        // Get starting key for the batch. Get the first key if we restart the process.
        let mut new_starting_raw_key = AlphaMapLastKey::<T>::get();
        let mut starting_key = None;
        if new_starting_raw_key.is_none() {
            starting_key = Alpha::<T>::iter_keys().next();
            new_starting_raw_key = starting_key.as_ref().map(Alpha::<T>::hashed_key_for);
        }

        if let Some(starting_raw_key) = new_starting_raw_key {
            // Get the key batch
            let mut keys = Alpha::<T>::iter_keys_from(starting_raw_key)
                .take(ALPHA_MAP_BATCH_SIZE)
                .collect::<Vec<_>>();

            // New iteration: insert the starting key in the batch if it's a new iteration
            // iter_keys_from() skips the starting key
            if let Some(starting_key) = starting_key {
                if keys.len() == ALPHA_MAP_BATCH_SIZE {
                    keys.remove(keys.len().saturating_sub(1));
                }
                keys.insert(0, starting_key);
            }

            let mut new_starting_key = None;
            let new_iteration = keys.len() < ALPHA_MAP_BATCH_SIZE;

            // Check and remove alphas if necessary
            for key in keys {
                let (_, coldkey, netuid) = key.clone();

                if netuid == NetUid::ROOT {
                    Self::maybe_add_coldkey_index(&coldkey);
                }

                new_starting_key = Some(Alpha::<T>::hashed_key_for(key));
            }

            // Restart the process if it's the last batch
            if new_iteration {
                new_starting_key = None;
            }

            AlphaMapLastKey::<T>::put(new_starting_key);
        }
    }

    // Same thing as populate_root_coldkey_staking_maps, but for AlphaV2
    // TODO: Remove this function and AlphaV2MapLastKey when slow migration is finished
    pub fn populate_root_coldkey_staking_maps_v2() {
        // Get starting key for the batch. Get the first key if we restart the process.
        let mut new_starting_raw_key = AlphaV2MapLastKey::<T>::get();
        let mut starting_key = None;
        if new_starting_raw_key.is_none() {
            starting_key = AlphaV2::<T>::iter_keys().next();
            new_starting_raw_key = starting_key.as_ref().map(AlphaV2::<T>::hashed_key_for);
        }

        if let Some(starting_raw_key) = new_starting_raw_key {
            // Get the key batch
            let mut keys = AlphaV2::<T>::iter_keys_from(starting_raw_key)
                .take(ALPHA_MAP_BATCH_SIZE)
                .collect::<Vec<_>>();

            // New iteration: insert the starting key in the batch if it's a new iteration
            // iter_keys_from() skips the starting key
            if let Some(starting_key) = starting_key {
                if keys.len() == ALPHA_MAP_BATCH_SIZE {
                    keys.remove(keys.len().saturating_sub(1));
                }
                keys.insert(0, starting_key);
            }

            let mut new_starting_key = None;
            let new_iteration = keys.len() < ALPHA_MAP_BATCH_SIZE;

            // Check and remove alphas if necessary
            for key in keys {
                let (_, coldkey, netuid) = key.clone();

                if netuid == NetUid::ROOT {
                    Self::maybe_add_coldkey_index(&coldkey);
                }

                new_starting_key = Some(AlphaV2::<T>::hashed_key_for(key));
            }

            // Restart the process if it's the last batch
            if new_iteration {
                new_starting_key = None;
            }

            AlphaV2MapLastKey::<T>::put(new_starting_key);
        }
    }

    /// Several alpha iteration helpers that merge key space from Alpha and AlphaV2 maps
    pub fn alpha_iter() -> impl Iterator<Item = ((T::AccountId, T::AccountId, NetUid), SafeFloat)> {
        // Old Alpha shares format: U64F64 -> SafeFloat
        let legacy = Alpha::<T>::iter().map(|(key, val_u64f64)| {
            let sf: SafeFloat = val_u64f64.into();
            (key, sf)
        });

        // New Alpha shares format
        let v2 = AlphaV2::<T>::iter();

        // Merge and prefer v2 on duplicates
        let merged: BTreeMap<_, SafeFloat> =
            legacy
                .chain(v2)
                .fold(BTreeMap::new(), |mut acc, (key, val)| {
                    acc.entry(key)
                        .and_modify(|existing| {
                            *existing = val.clone();
                        })
                        .or_insert(val);
                    acc
                });

        merged.into_iter()
    }

    pub fn alpha_iter_prefix(
        prefix: (&T::AccountId, &T::AccountId),
    ) -> impl Iterator<Item = (NetUid, SafeFloat)>
    where
        T::AccountId: Clone,
    {
        // Old Alpha shares format: U64F64 -> SafeFloat
        let legacy = Alpha::<T>::iter_prefix(prefix).map(|(netuid, val_u64f64)| {
            let sf: SafeFloat = val_u64f64.into();
            (netuid, sf)
        });

        // New Alpha shares format
        let v2 = AlphaV2::<T>::iter_prefix(prefix);

        // Merge by netuid and sum SafeFloat values
        let merged: BTreeMap<NetUid, SafeFloat> =
            legacy
                .chain(v2)
                .fold(BTreeMap::new(), |mut acc, (netuid, sf)| {
                    acc.entry(netuid)
                        .and_modify(|existing| {
                            *existing = sf.clone();
                        })
                        .or_insert(sf);
                    acc
                });

        merged
            .into_iter()
            .filter(|(_, alpha_share)| !alpha_share.is_zero())
    }

    pub fn alpha_iter_single_prefix(
        prefix: &T::AccountId,
    ) -> impl Iterator<Item = (T::AccountId, NetUid, SafeFloat)>
    where
        T::AccountId: Clone,
    {
        // Old Alpha shares format: U64F64 -> SafeFloat
        let legacy =
            Alpha::<T>::iter_prefix((prefix.clone(),)).map(|((coldkey, netuid), val_u64f64)| {
                let sf: SafeFloat = val_u64f64.into();
                ((coldkey, netuid), sf)
            });

        // New Alpha shares format
        let v2 = AlphaV2::<T>::iter_prefix((prefix,));

        let merged: BTreeMap<(T::AccountId, NetUid), SafeFloat> =
            legacy
                .chain(v2)
                .fold(BTreeMap::new(), |mut acc, (key, sf)| {
                    acc.entry(key)
                        .and_modify(|existing| {
                            *existing = sf.clone();
                        })
                        .or_insert(sf);
                    acc
                });

        merged
            .into_iter()
            .map(|((coldkey, netuid), sf)| (coldkey, netuid, sf))
    }
}
