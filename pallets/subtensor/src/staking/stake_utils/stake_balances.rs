//! Read and mutate alpha stake shares for hotkey/coldkey pairs on a subnet.
use super::*;
use sp_std::ops::Neg;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

impl<T: Config> Pallet<T> {
    /// Checks if a specific hotkey-coldkey pair has enough stake on a subnet to fulfill a given decrement.
    ///
    /// This function performs the following steps:
    /// 1. Retrieves the current stake for the hotkey-coldkey pair on the specified subnet.
    /// 2. Compares this stake with the requested decrement amount.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey.
    /// * `coldkey`: The account ID of the coldkey.
    /// * `netuid`: The unique identifier of the subnet.
    /// * `decrement`: The amount of stake to be potentially decremented.
    ///
    /// # Returns
    /// * `bool`: True if the account has enough stake to fulfill the decrement, false otherwise.
    ///
    /// # Note
    /// This function only checks the stake for the specific hotkey-coldkey pair, not the total stake of the hotkey or coldkey individually.
    pub fn calculate_reduced_stake_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        decrement: AlphaBalance,
    ) -> Result<AlphaBalance, Error<T>> {
        // Retrieve the current stake for this hotkey-coldkey pair on the subnet
        let current_stake =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);

        // Compare the current stake with the requested decrement
        // Return true if the current stake is greater than or equal to the decrement
        if current_stake >= decrement {
            Ok(current_stake.saturating_sub(decrement))
        } else {
            Err(Error::<T>::NotEnoughStakeToWithdraw)
        }
    }

    /// Retrieves the alpha (stake) value for a given hotkey and coldkey pair on a specific subnet.
    ///
    /// This function performs the following steps:
    /// 1. Takes the hotkey, coldkey, and subnet ID as input parameters.
    /// 2. Accesses the Alpha storage map to retrieve the stake value.
    /// 3. Returns the retrieved stake value as a u64.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey (neuron).
    /// * `coldkey`: The account ID of the coldkey (owner).
    /// * `netuid`: The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `u64`: The alpha (stake) value for the specified hotkey-coldkey pair on the given subnet.
    ///
    /// # Note
    /// This function retrieves the stake specific to the hotkey-coldkey pair, not the total stake of the hotkey or coldkey individually.
    pub fn get_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> AlphaBalance {
        let alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.try_get_value(coldkey).unwrap_or(0).into()
    }

    /// Retrieves the total stake (alpha) for a given hotkey on a specific subnet.
    ///
    /// This function performs the following step:
    /// 1. Retrieves and returns the total alpha value associated with the hotkey on the specified subnet.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey.
    /// * `netuid`: The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `u64`: The total alpha value for the hotkey on the specified subnet.
    ///
    /// # Note
    /// This function returns the cumulative stake across all coldkeys associated with this hotkey on the subnet.
    pub fn get_stake_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        // Retrieve and return the total alpha this hotkey owns on this subnet.
        // This value represents the sum of stakes from all coldkeys associated with this hotkey.
        TotalHotkeyAlpha::<T>::get(hotkey, netuid)
    }

    /// Increase hotkey stake on a subnet.
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey.
    /// * `netuid`: The unique identifier of the subnet.
    /// * `amount`: The amount of alpha to be added.
    ///
    pub fn increase_stake_for_hotkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.update_value_for_all(amount.to_u64() as i64);
    }

    /// Decrease hotkey stake on a subnet.
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey.
    /// * `netuid`: The unique identifier of the subnet.
    /// * `amount`: The amount of alpha to be added.
    ///
    pub fn decrease_stake_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: NetUid, amount: u64) {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.update_value_for_all((amount as i64).neg());
    }

    /// Buys shares in the hotkey on a given subnet
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey.
    /// * `coldkey`: The account ID of the coldkey (owner).
    /// * `netuid`: The unique identifier of the subnet.
    /// * `amount`: The amount of alpha to be added.
    ///
    pub fn increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) {
        if !amount.is_zero() {
            let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
            if !staking_hotkeys.contains(hotkey) {
                staking_hotkeys.push(hotkey.clone());
                StakingHotkeys::<T>::insert(coldkey, staking_hotkeys.clone());
            }
        }

        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        // We expect to add a positive amount here.
        let amount = amount.to_u64() as i64;
        alpha_share_pool.update_value_for_one(coldkey, amount);
    }

    pub fn try_increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> bool {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        let amount = amount.to_u64() as i64;
        alpha_share_pool.sim_update_value_for_one(amount)
    }

    /// Sell shares in the hotkey on a given subnet
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey`: The account ID of the hotkey.
    /// * `coldkey`: The account ID of the coldkey (owner).
    /// * `netuid`: The unique identifier of the subnet.
    /// * `amount`: The amount of alpha to be added.
    ///
    pub fn decrease_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        let amount = amount.to_u64();

        // We expect a negative value here
        if let Ok(value) = alpha_share_pool.try_get_value(coldkey)
            && value >= amount
        {
            alpha_share_pool.update_value_for_one(coldkey, (amount as i64).neg());
        }
    }
}
