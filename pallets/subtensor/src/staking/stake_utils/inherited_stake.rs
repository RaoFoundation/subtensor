//! Stake weight vectors and parent/child inherited alpha for a hotkey on a subnet.
use super::*;
use safe_math::*;
use substrate_fixed::types::{I64F64, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

impl<T: Config> Pallet<T> {
    /// Calculates the weighted combination of alpha and TAO stake for a single hotkey on a subnet.
    ///
    pub fn get_stake_weights_for_hotkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> (I64F64, I64F64, I64F64) {
        // Retrieve the TAO weight.
        let tao_weight = I64F64::saturating_from_num(Self::get_tao_weight());
        log::debug!("tao_weight: {tao_weight:?}");

        // Step 1: Get stake of hotkey (neuron)
        let alpha_stake =
            I64F64::saturating_from_num(Self::get_inherited_for_hotkey_on_subnet(hotkey, netuid));
        log::debug!("alpha_stake: {alpha_stake:?}");

        // Step 2: Get the TAO stake for the hotkey
        let tao_stake = I64F64::saturating_from_num(Self::get_tao_inherited_for_hotkey_on_subnet(
            hotkey, netuid,
        ));
        log::debug!("tao_stake: {tao_stake:?}");

        // Step 3: Combine alpha and tao stakes
        let total_stake = alpha_stake.saturating_add(tao_stake.saturating_mul(tao_weight));
        log::debug!("total_stake: {total_stake:?}");

        (total_stake, alpha_stake, tao_stake)
    }

    /// Calculates the weighted combination of alpha and TAO stake for hotkeys on a subnet.
    ///
    pub fn get_stake_weights_for_network(
        netuid: NetUid,
    ) -> (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) {
        // Retrieve the TAO weight.
        let tao_weight: I64F64 = I64F64::saturating_from_num(Self::get_tao_weight());
        log::debug!("tao_weight: {tao_weight:?}");

        // Step 1: Get subnetwork size
        let n: u16 = Self::get_subnetwork_n(netuid);

        // Step 2: Get stake of all hotkeys (neurons) ordered by uid
        let alpha_stake: Vec<I64F64> = (0..n)
            .map(|uid| {
                if Keys::<T>::contains_key(netuid, uid) {
                    let hotkey: T::AccountId = Keys::<T>::get(netuid, uid);
                    I64F64::saturating_from_num(Self::get_inherited_for_hotkey_on_subnet(
                        &hotkey, netuid,
                    ))
                } else {
                    I64F64::saturating_from_num(0)
                }
            })
            .collect();
        log::debug!("alpha_stake: {alpha_stake:?}");

        // Step 3: Calculate the TAO stake vector.
        // Initialize a vector to store TAO stakes for each neuron.
        let tao_stake: Vec<I64F64> = (0..n)
            .map(|uid| {
                if Keys::<T>::contains_key(netuid, uid) {
                    let hotkey: T::AccountId = Keys::<T>::get(netuid, uid);
                    I64F64::saturating_from_num(Self::get_tao_inherited_for_hotkey_on_subnet(
                        &hotkey, netuid,
                    ))
                } else {
                    I64F64::saturating_from_num(0)
                }
            })
            .collect();
        log::trace!("tao_stake: {tao_stake:?}");

        // Step 4: Combine alpha and TAO stakes.
        // Calculate the weighted average of alpha and TAO stakes for each neuron.
        let total_stake: Vec<I64F64> = alpha_stake
            .iter()
            .zip(tao_stake.iter())
            .map(|(alpha_i, tao_i)| alpha_i.saturating_add(tao_i.saturating_mul(tao_weight)))
            .collect();
        log::trace!("total_stake: {total_stake:?}");

        (total_stake, alpha_stake, tao_stake)
    }

    /// Calculates the total inherited stake (alpha) held by a hotkey on a network, considering child/parent relationships.
    ///
    /// This function performs the following steps:
    /// 1. Retrieves the initial alpha (stake) for the hotkey on the specified subnet.
    /// 2. Retrieves the list of children and parents for the hotkey on the subnet.
    /// 3. Calculates the alpha allocated to children:
    ///    a. For each child, computes the proportion of alpha to be allocated.
    ///    b. Accumulates the total alpha allocated to all children.
    /// 4. Calculates the alpha received from parents:
    ///    a. For each parent, retrieves the parent's stake on the subnet.
    ///    b. Computes the proportion of the parent's stake to be inherited.
    ///    c. Accumulates the total alpha inherited from all parents.
    /// 5. Computes the final inherited alpha by adjusting the initial alpha:
    ///    a. Subtracts the alpha allocated to children.
    ///    b. Adds the alpha inherited from parents.
    /// 6. Returns the final inherited alpha value.
    ///
    /// # Arguments
    /// * `hotkey`: AccountId of the hotkey whose total inherited stake is to be calculated.
    /// * `netuid`: Network unique identifier specifying the subnet context.
    ///
    /// # Returns
    /// * `u64`: The total inherited alpha for the hotkey on the subnet after considering the
    ///   stakes allocated to children and inherited from parents.
    ///
    /// # Note
    /// This function uses saturating arithmetic to prevent overflows.
    pub fn get_tao_inherited_for_hotkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> TaoBalance {
        let initial_tao: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT));

        // Initialize variables to track alpha allocated to children and inherited from parents.
        let mut tao_to_children: U96F32 = U96F32::saturating_from_num(0);
        let mut tao_from_parents: U96F32 = U96F32::saturating_from_num(0);

        // Step 2: Retrieve the lists of parents and children for the hotkey on the subnet.
        let parents: Vec<(u64, T::AccountId)> = Self::get_parents(hotkey, netuid);
        let children: Vec<(u64, T::AccountId)> = Self::get_children(hotkey, netuid);
        log::trace!("Parents for hotkey {hotkey:?} on subnet {netuid}: {parents:?}");
        log::trace!("Children for hotkey {hotkey:?} on subnet {netuid}: {children:?}");

        // Step 3: Calculate the total tao allocated to children.
        for (proportion, _) in children {
            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!("Normalized proportion for child: {normalized_proportion:?}");

            // Calculate the amount of tao to be allocated to this child.
            let tao_proportion_to_child: U96F32 =
                U96F32::saturating_from_num(initial_tao).saturating_mul(normalized_proportion);
            log::trace!("Tao proportion to child: {tao_proportion_to_child:?}");

            // Add this child's allocation to the total tao allocated to children.
            tao_to_children = tao_to_children.saturating_add(tao_proportion_to_child);
        }
        log::trace!("Total tao allocated to children: {tao_to_children:?}");

        // Step 4: Calculate the total tao inherited from parents.
        for (proportion, parent) in parents {
            // Retrieve the parent's total stake on this subnet.
            let parent_tao = U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(
                &parent,
                NetUid::ROOT,
            ));
            log::trace!("Parent tao for parent {parent:?} on subnet {netuid}: {parent_tao:?}");

            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!("Normalized proportion from parent: {normalized_proportion:?}");

            // Calculate the amount of tao to be inherited from this parent.
            let tao_proportion_from_parent: U96F32 =
                U96F32::saturating_from_num(parent_tao).saturating_mul(normalized_proportion);
            log::trace!("Tao proportion from parent: {tao_proportion_from_parent:?}");

            // Add this parent's contribution to the total tao inherited from parents.
            tao_from_parents = tao_from_parents.saturating_add(tao_proportion_from_parent);
        }
        log::trace!("Total tao inherited from parents: {tao_from_parents:?}");

        // Step 5: Calculate the final inherited tao for the hotkey.
        let finalized_tao: U96F32 = initial_tao
            .saturating_sub(tao_to_children) // Subtract tao allocated to children
            .saturating_add(tao_from_parents); // Add tao inherited from parents
        log::trace!("Finalized tao for hotkey {hotkey:?} on subnet {netuid}: {finalized_tao:?}");

        // Step 6: Return the final inherited tao value.
        finalized_tao.saturating_to_num::<u64>().into()
    }

    pub fn get_inherited_for_hotkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> AlphaBalance {
        // Step 1: Retrieve the initial total stake (alpha) for the hotkey on the specified subnet.
        let initial_alpha: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, netuid));
        log::debug!("Initial alpha for hotkey {hotkey:?} on subnet {netuid}: {initial_alpha:?}");
        if netuid.is_root() {
            return initial_alpha.saturating_to_num::<u64>().into();
        }

        // Initialize variables to track alpha allocated to children and inherited from parents.
        let mut alpha_to_children: U96F32 = U96F32::saturating_from_num(0);
        let mut alpha_from_parents: U96F32 = U96F32::saturating_from_num(0);

        // Step 2: Retrieve the lists of parents and children for the hotkey on the subnet.
        let parents: Vec<(u64, T::AccountId)> = Self::get_parents(hotkey, netuid);
        let children: Vec<(u64, T::AccountId)> = Self::get_children(hotkey, netuid);
        log::debug!("Parents for hotkey {hotkey:?} on subnet {netuid}: {parents:?}");
        log::debug!("Children for hotkey {hotkey:?} on subnet {netuid}: {children:?}");

        // Step 3: Calculate the total alpha allocated to children.
        for (proportion, _) in children {
            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!("Normalized proportion for child: {normalized_proportion:?}");

            // Calculate the amount of alpha to be allocated to this child.
            let alpha_proportion_to_child: U96F32 =
                U96F32::saturating_from_num(initial_alpha).saturating_mul(normalized_proportion);
            log::trace!("Alpha proportion to child: {alpha_proportion_to_child:?}");

            // Add this child's allocation to the total alpha allocated to children.
            alpha_to_children = alpha_to_children.saturating_add(alpha_proportion_to_child);
        }
        log::debug!("Total alpha allocated to children: {alpha_to_children:?}");

        // Step 4: Calculate the total alpha inherited from parents.
        for (proportion, parent) in parents {
            // Retrieve the parent's total stake on this subnet.
            let parent_alpha: U96F32 =
                U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(&parent, netuid));
            log::trace!("Parent alpha for parent {parent:?} on subnet {netuid}: {parent_alpha:?}");

            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!("Normalized proportion from parent: {normalized_proportion:?}");

            // Calculate the amount of alpha to be inherited from this parent.
            let alpha_proportion_from_parent: U96F32 =
                U96F32::saturating_from_num(parent_alpha).saturating_mul(normalized_proportion);
            log::trace!("Alpha proportion from parent: {alpha_proportion_from_parent:?}");

            // Add this parent's contribution to the total alpha inherited from parents.
            alpha_from_parents = alpha_from_parents.saturating_add(alpha_proportion_from_parent);
        }
        log::debug!("Total alpha inherited from parents: {alpha_from_parents:?}");

        // Step 5: Calculate the final inherited alpha for the hotkey.
        let finalized_alpha: U96F32 = initial_alpha
            .saturating_sub(alpha_to_children) // Subtract alpha allocated to children
            .saturating_add(alpha_from_parents); // Add alpha inherited from parents
        log::trace!(
            "Finalized alpha for hotkey {hotkey:?} on subnet {netuid}: {finalized_alpha:?}"
        );

        // Step 6: Return the final inherited alpha value.
        finalized_alpha.saturating_to_num::<u64>().into()
    }
}
