use super::*;
use crate::epoch::run_epoch::EpochTerms;
use alloc::collections::BTreeMap;
use subtensor_runtime_common::{AlphaBalance, NetUid};

/// 14 days in blocks (assuming ~12 second blocks)
/// 14 * 24 * 60 * 60 / 12 = 100800 blocks
pub const VOTING_POWER_DISABLE_GRACE_PERIOD_BLOCKS: u64 = 100800;

/// Maximum alpha value (1.0 represented as u64 with 18 decimals)
pub const MAX_VOTING_POWER_EMA_ALPHA: u64 = 1_000_000_000_000_000_000;

impl<T: Config> Pallet<T> {
    // ========================
    // === Getters ===
    // ========================

    /// Get voting power for a hotkey on a subnet.
    /// Returns 0 if not found or tracking disabled.
    pub fn get_voting_power(netuid: NetUid, hotkey: &T::AccountId) -> u64 {
        VotingPower::<T>::get(netuid, hotkey)
    }

    /// Check if voting power tracking is enabled for a subnet.
    pub fn get_voting_power_tracking_enabled(netuid: NetUid) -> bool {
        VotingPowerTrackingEnabled::<T>::get(netuid)
    }

    /// Get the block at which voting power tracking will be disabled.
    /// Returns 0 if not scheduled for disabling.
    pub fn get_voting_power_disable_at_block(netuid: NetUid) -> u64 {
        VotingPowerDisableAtBlock::<T>::get(netuid)
    }

    /// Get the EMA alpha value for voting power calculation on a subnet.
    pub fn get_voting_power_ema_alpha(netuid: NetUid) -> u64 {
        VotingPowerEmaAlpha::<T>::get(netuid)
    }

    // ========================
    // === Extrinsic Handlers ===
    // ========================

    /// Enable voting power tracking for a subnet.
    pub fn do_enable_voting_power_tracking(netuid: NetUid) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Enable tracking
        VotingPowerTrackingEnabled::<T>::insert(netuid, true);

        // Clear any scheduled disable
        VotingPowerDisableAtBlock::<T>::remove(netuid);

        // Emit event
        Self::deposit_event(Event::VotingPowerTrackingEnabled { netuid });

        log::info!("VotingPower tracking enabled for netuid {netuid:?}");

        Ok(())
    }

    /// Schedule disabling of voting power tracking for a subnet.
    /// Tracking will continue for 14 days, then automatically disable.
    pub fn do_disable_voting_power_tracking(netuid: NetUid) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Check if tracking is enabled
        ensure!(
            Self::get_voting_power_tracking_enabled(netuid),
            Error::<T>::VotingPowerTrackingNotEnabled
        );

        // Calculate the block at which tracking will be disabled
        let current_block = Self::get_current_block_as_u64();
        let disable_at_block =
            current_block.saturating_add(VOTING_POWER_DISABLE_GRACE_PERIOD_BLOCKS);

        // Schedule disable
        VotingPowerDisableAtBlock::<T>::insert(netuid, disable_at_block);

        // Emit event
        Self::deposit_event(Event::VotingPowerTrackingDisableScheduled {
            netuid,
            disable_at_block,
        });

        log::info!(
            "VotingPower tracking scheduled to disable at block {disable_at_block:?} for netuid {netuid:?}"
        );

        Ok(())
    }

    /// Set the EMA alpha value for voting power calculation on a subnet.
    pub fn do_set_voting_power_ema_alpha(netuid: NetUid, alpha: u64) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Validate alpha (must be <= 1.0, represented as 10^18)
        ensure!(
            alpha <= MAX_VOTING_POWER_EMA_ALPHA,
            Error::<T>::InvalidVotingPowerEmaAlpha
        );

        // Set the alpha
        VotingPowerEmaAlpha::<T>::insert(netuid, alpha);

        // Emit event
        Self::deposit_event(Event::VotingPowerEmaAlphaSet { netuid, alpha });

        log::info!("VotingPower EMA alpha set to {alpha:?} for netuid {netuid:?}");

        Ok(())
    }

    // ========================
    // === Epoch Processing ===
    // ========================

    /// Update voting power for all validators on a subnet using pre-calculated epoch terms.
    pub fn update_voting_power_for_subnet(
        netuid: NetUid,
        epoch_output: &BTreeMap<T::AccountId, EpochTerms>,
    ) {
        // Early exit if tracking not enabled
        if !Self::get_voting_power_tracking_enabled(netuid) {
            return;
        }

        // Check if past grace period and should finalize disable
        let disable_at = Self::get_voting_power_disable_at_block(netuid);
        if disable_at > 0 {
            let current_block = Self::get_current_block_as_u64();
            if current_block >= disable_at {
                Self::finalize_voting_power_disable(netuid);
                return;
            }
            // Still in grace period - continue updating
        }

        // Get the EMA alpha value for this subnet
        let alpha = Self::get_voting_power_ema_alpha(netuid);

        // Get minimum stake threshold for validator permit
        let min_stake = Self::get_stake_threshold();

        // Iterate over epoch output using pre-calculated values
        for (hotkey, terms) in epoch_output.iter() {
            // Only validators (with vpermit) get voting power, not miners
            if terms.new_validator_permit {
                // Use the subnet-specific stake from epoch calculation
                Self::update_voting_power_for_hotkey(netuid, hotkey, terms.stake, alpha, min_stake);
            } else {
                // Miner without vpermit - remove any existing voting power
                VotingPower::<T>::remove(netuid, hotkey);
            }
        }

        // Remove voting power for any hotkeys that are no longer registered on this subnet
        Self::clear_voting_power_for_deregistered_hotkeys(netuid);

        log::trace!("VotingPower updated for validators on netuid {netuid:?}");
    }

    /// Clear voting power for hotkeys that are no longer registered on the subnet.
    fn clear_voting_power_for_deregistered_hotkeys(netuid: NetUid) {
        // Collect hotkeys to remove (can't mutate while iterating)
        let hotkeys_to_remove: Vec<T::AccountId> = VotingPower::<T>::iter_prefix(netuid)
            .filter_map(|(hotkey, _)| {
                // If the hotkey is not a network member, it's deregistered
                if !IsNetworkMember::<T>::get(&hotkey, netuid) {
                    Some(hotkey)
                } else {
                    None
                }
            })
            .collect();

        // Remove voting power for deregistered hotkeys
        for hotkey in hotkeys_to_remove {
            VotingPower::<T>::remove(netuid, &hotkey);
            log::trace!(
                "VotingPower removed for deregistered hotkey {hotkey:?} on netuid {netuid:?}"
            );
        }
    }

    /// Update voting power EMA for a single hotkey using subnet-specific stake.
    fn update_voting_power_for_hotkey(
        netuid: NetUid,
        hotkey: &T::AccountId,
        current_stake: AlphaBalance,
        alpha: u64,
        min_stake: u64,
    ) {
        // Get previous EMA value
        let previous_ema = VotingPower::<T>::get(netuid, hotkey);

        // Calculate new EMA value
        // new_ema = alpha * current_stake + (1 - alpha) * previous_ema
        // All values use 18 decimal precision for alpha (alpha is in range [0, 10^18])
        let new_ema = Self::calculate_voting_power_ema(current_stake.to_u64(), previous_ema, alpha);

        // Only remove if they previously had voting power ABOVE threshold and decayed below.
        // This allows new validators to build up voting power from 0 without being removed.
        if new_ema < min_stake && previous_ema >= min_stake {
            // Was above threshold, now decayed below - remove
            VotingPower::<T>::remove(netuid, hotkey);
            log::trace!(
                "VotingPower removed for hotkey {hotkey:?} on netuid {netuid:?} (decayed below removal threshold: {new_ema:?} < {min_stake:?})"
            );
        } else if new_ema > 0 {
            // Update voting power (building up or maintaining)
            VotingPower::<T>::insert(netuid, hotkey, new_ema);
            log::trace!(
                "VotingPower updated for hotkey {hotkey:?} on netuid {netuid:?}: {previous_ema:?} -> {new_ema:?}"
            );
        }
        // If new_ema == 0 do nothing
    }

    /// Calculate EMA for voting power.
    /// new_ema = alpha * current_stake + (1 - alpha) * previous_ema
    /// Alpha is in 18 decimal precision (10^18 = 1.0)
    fn calculate_voting_power_ema(current_stake: u64, previous_ema: u64, alpha: u64) -> u64 {
        // Use u128 for intermediate calculations to avoid overflow
        let alpha_128 = alpha as u128;
        let one_minus_alpha = (MAX_VOTING_POWER_EMA_ALPHA as u128).saturating_sub(alpha_128);
        let current_128 = current_stake as u128;
        let previous_128 = previous_ema as u128;

        // new_ema = (alpha * current_stake + (1 - alpha) * previous_ema) / 10^18
        let numerator = alpha_128
            .saturating_mul(current_128)
            .saturating_add(one_minus_alpha.saturating_mul(previous_128));

        let result = numerator
            .checked_div(MAX_VOTING_POWER_EMA_ALPHA as u128)
            .unwrap_or(0);

        // Safely convert back to u64, saturating at u64::MAX
        result.min(u64::MAX as u128) as u64
    }

    /// Finalize the disabling of voting power tracking.
    /// Clears all VotingPower entries for the subnet.
    fn finalize_voting_power_disable(netuid: NetUid) {
        // Clear all VotingPower entries for this subnet
        let _ = VotingPower::<T>::clear_prefix(netuid, u32::MAX, None);

        // Disable tracking
        VotingPowerTrackingEnabled::<T>::insert(netuid, false);

        // Clear disable schedule
        VotingPowerDisableAtBlock::<T>::remove(netuid);

        // Emit event
        Self::deposit_event(Event::VotingPowerTrackingDisabled { netuid });

        log::info!("VotingPower tracking disabled and entries cleared for netuid {netuid:?}");
    }

    // ========================
    // === Hotkey Swap ===
    // ========================

    /// Transfer voting power from old hotkey to new hotkey during swap.
    pub fn swap_voting_power_for_hotkey(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
    ) {
        // Get voting power from old hotkey
        let voting_power = VotingPower::<T>::take(netuid, old_hotkey);

        // Transfer to new hotkey if non-zero
        if voting_power > 0 {
            // Add to any existing voting power on new hotkey (in case new hotkey already has some)
            let existing = VotingPower::<T>::get(netuid, new_hotkey);
            VotingPower::<T>::insert(netuid, new_hotkey, voting_power.saturating_add(existing));

            log::trace!(
                "VotingPower transferred from {old_hotkey:?} to {new_hotkey:?} on netuid {netuid:?}: {voting_power:?}"
            );
        }
    }
}
