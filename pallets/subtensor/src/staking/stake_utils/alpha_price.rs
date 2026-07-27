//! Alpha issuance, moving price EMA, median price, and TAO-weight / childkey-burn knobs.
use super::*;
use safe_math::*;
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::{I96F32, U64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
    /// Retrieves the total alpha issuance for a given subnet.
    ///
    /// This function calculates the total alpha issuance by summing the alpha
    /// values from `SubnetAlphaIn` and `SubnetAlphaOut` for the specified subnet.
    ///
    /// # Arguments
    /// * `netuid`: The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `u64`: The total alpha issuance for the specified subnet.
    pub fn get_alpha_issuance(netuid: NetUid) -> AlphaBalance {
        SubnetAlphaIn::<T>::get(netuid)
            .saturating_add(SubnetAlphaOut::<T>::get(netuid))
            .saturating_add(T::SwapInterface::protocol_alpha_reservoir(netuid))
    }

    pub fn get_moving_alpha_price(netuid: NetUid) -> U64F64 {
        let one = U64F64::saturating_from_num(1.0);
        if netuid.is_root() {
            // Root.
            one
        } else if SubnetMechanism::<T>::get(netuid) == 0 {
            // Stable
            one
        } else {
            U64F64::saturating_from_num(SubnetMovingPrice::<T>::get(netuid))
        }
    }

    pub fn update_moving_price(netuid: NetUid) {
        let blocks_since_start_call = U64F64::saturating_from_num({
            // We expect FirstEmissionBlockNumber to be set earlier, and we take the block when
            // `start_call` was called (first block before FirstEmissionBlockNumber).
            let start_call_block = FirstEmissionBlockNumber::<T>::get(netuid)
                .unwrap_or_default()
                .saturating_sub(1);

            Self::get_current_block_as_u64().saturating_sub(start_call_block)
        });

        // Use halving time hyperparameter. The meaning of this parameter can be best explained under
        // the assumption of a constant price and SubnetMovingAlpha == 0.5: It is how many blocks it
        // will take in order for the distance between current EMA of price and current price to shorten
        // by half.
        let halving_time = EMAPriceHalvingBlocks::<T>::get(netuid);
        let current_ma_unsigned = U64F64::saturating_from_num(SubnetMovingAlpha::<T>::get());
        let alpha: U64F64 = current_ma_unsigned.saturating_mul(blocks_since_start_call.safe_div(
            blocks_since_start_call.saturating_add(U64F64::saturating_from_num(halving_time)),
        ));
        // Because alpha = b / (b + h), where b and h > 0, alpha < 1, so 1 - alpha > 0.
        // We can use unsigned type here: U96F32
        let one_minus_alpha: U64F64 = U64F64::saturating_from_num(1.0).saturating_sub(alpha);
        let current_price: U64F64 = alpha.saturating_mul(U64F64::saturating_from_num(
            T::SwapInterface::current_alpha_price(netuid.into())
                .min(U64F64::saturating_from_num(1.0)),
        ));
        let current_moving: U64F64 = one_minus_alpha.saturating_mul(U64F64::saturating_from_num(
            Self::get_moving_alpha_price(netuid),
        ));
        // Convert batch to signed I96F32 to avoid migration of SubnetMovingPrice for now``
        let new_moving: I96F32 =
            I96F32::saturating_from_num(current_price.saturating_add(current_moving));
        SubnetMovingPrice::<T>::insert(netuid, new_moving);
    }

    /// Gets the Median Subnet Alpha Price
    pub fn get_median_subnet_alpha_price() -> U64F64 {
        let default_price = U64F64::saturating_from_num(1_u64);
        let zero_price = U64F64::saturating_from_num(0_u64);
        let two = U64F64::saturating_from_num(2_u64);

        let mut price_counts: BTreeMap<U64F64, usize> = BTreeMap::new();
        let mut total_prices: usize = 0;

        for (netuid, added) in NetworksAdded::<T>::iter() {
            if !added || netuid == NetUid::ROOT {
                continue;
            }

            let price = T::SwapInterface::current_alpha_price(netuid);
            if price <= zero_price {
                continue;
            }

            total_prices = total_prices.saturating_add(1);

            if let Some(count) = price_counts.get_mut(&price) {
                *count = count.saturating_add(1);
            } else {
                price_counts.insert(price, 1usize);
            }
        }

        if total_prices == 0 {
            return default_price;
        }

        let Some(last_index) = total_prices.checked_sub(1) else {
            return default_price;
        };
        let Some(lower_target) = last_index.checked_div(2) else {
            return default_price;
        };
        let Some(upper_target) = total_prices.checked_div(2) else {
            return default_price;
        };

        let mut cumulative: usize = 0;
        let mut lower_price: Option<U64F64> = None;
        let mut upper_price: Option<U64F64> = None;

        for (price, count) in price_counts.into_iter() {
            let next_cumulative = cumulative.saturating_add(count);

            if lower_price.is_none() && lower_target < next_cumulative {
                lower_price = Some(price);
            }

            if upper_price.is_none() && upper_target < next_cumulative {
                upper_price = Some(price);
            }

            if lower_price.is_some() && upper_price.is_some() {
                break;
            }

            cumulative = next_cumulative;
        }

        match (lower_price, upper_price) {
            (Some(_), Some(upper)) if lower_target == upper_target => upper,
            (Some(lower), Some(upper)) => lower.saturating_add(upper).safe_div(two),
            _ => default_price,
        }
    }

    /// Retrieves the TAO weight as a normalized value between 0 and 1.
    ///
    /// This function performs the following steps:
    /// 1. Fetches the TAO weight from storage using the TaoWeight storage item.
    /// 2. Converts the retrieved u64 value to a fixed-point number (U96F32).
    /// 3. Normalizes the weight by dividing it by the maximum possible u64 value.
    /// 4. Returns the normalized weight as an U96F32 fixed-point number.
    ///
    /// The normalization ensures that the returned value is always between 0 and 1,
    /// regardless of the actual stored weight value.
    ///
    /// # Returns
    /// * `U96F32`: The normalized TAO weight as a fixed-point number between 0 and 1.
    ///
    /// # Note
    /// This function uses saturating division to prevent potential overflow errors.
    pub fn get_tao_weight() -> U96F32 {
        // Step 1: Fetch the TAO weight from storage
        let stored_weight = TaoWeight::<T>::get();

        // Step 2: Convert the u64 weight to U96F32
        let weight_fixed = U96F32::saturating_from_num(stored_weight);

        // Step 3: Normalize the weight by dividing by u64::MAX
        // This ensures the result is always between 0 and 1
        weight_fixed.safe_div(U96F32::saturating_from_num(u64::MAX))
    }

    pub fn get_ck_burn() -> U96F32 {
        let stored_weight = CKBurn::<T>::get();
        let weight_fixed = U96F32::saturating_from_num(stored_weight);
        weight_fixed.safe_div(U96F32::saturating_from_num(u64::MAX))
    }

    /// Sets the TAO weight in storage.
    ///
    /// This function performs the following steps:
    /// 1. Takes the provided weight value as a u64.
    /// 2. Updates the TaoWeight storage item with the new value.
    ///
    /// # Arguments
    /// * `weight`: The new TAO weight value to be set, as a u64.
    ///
    /// # Effects
    /// This function modifies the following storage item:
    /// * `TaoWeight`: Updates it with the new weight value.
    ///
    /// # Note
    /// The weight is stored as a raw u64 value. To get the normalized weight between 0 and 1,
    /// use the `get_tao_weight()` function.
    pub fn set_tao_weight(weight: u64) {
        // Update the TaoWeight storage with the new weight value
        TaoWeight::<T>::set(weight);
    }

    // Set the amount burned on non owned CK
    pub fn set_ck_burn(weight: u64) {
        // Update the ck burn value.
        CKBurn::<T>::set(weight);
    }
}
