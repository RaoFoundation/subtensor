//! Coldkey lock queries and unstake availability checks.
//!
//! Conviction locks are subnet-wide: they block unstaking from any hotkey on the
//! subnet, not a single hotkey position.
use super::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Returns the sum of raw alpha shares for a coldkey across all hotkeys on a given subnet.
    pub fn total_coldkey_alpha_on_subnet(coldkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        StakingHotkeys::<T>::get(coldkey)
            .into_iter()
            .map(|hotkey| {
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, coldkey, netuid)
            })
            .fold(AlphaBalance::ZERO, |acc, stake| acc.saturating_add(stake))
    }

    /// Returns the current locked amount for a coldkey on a subnet.
    pub fn get_current_locked(coldkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now)
            .map(|(_hotkey, mut model)| {
                model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
                model.individual_lock().locked_mass
            })
            .unwrap_or(AlphaBalance::ZERO)
    }

    /// Returns the current conviction for a coldkey on a subnet (rolled forward to now).
    pub fn get_conviction(coldkey: &T::AccountId, netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now)
            .map(|(_hotkey, mut model)| {
                model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
                model.individual_lock().conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0))
    }

    /// Returns the current lock for a coldkey on a subnet, rolled forward to now.
    pub fn get_coldkey_lock(coldkey: &T::AccountId, netuid: NetUid) -> Option<LockState> {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now).map(|(_hotkey, mut model)| {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            model.individual_lock().clone()
        })
    }

    /// (total_stake, locked_mass, available_to_unstake) for a coldkey on one subnet.
    ///
    /// The conviction lock is subnet-wide: it blocks unstaking from any hotkey on
    /// that subnet, not from a single hotkey position. Miner registration
    /// collateral is also subtracted here as a coldkey-wide residual; call sites
    /// that know the origin hotkey must additionally call
    /// `ensure_hotkey_covers_collateral` so the bond cannot be covered by free
    /// stake on a sibling hotkey.
    pub fn stake_availability(
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> (AlphaBalance, AlphaBalance, AlphaBalance) {
        let total = Self::total_coldkey_alpha_on_subnet(coldkey, netuid);
        let locked = Self::get_current_locked(coldkey, netuid);
        let collateral = Self::total_miner_collateral_for_coldkey(coldkey, netuid);
        let available = total.saturating_sub(locked).saturating_sub(collateral);
        (total, locked, available)
    }

    /// Alpha the coldkey can still unstake on this subnet right now.
    pub fn available_to_unstake(coldkey: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        let (_, _, available) = Self::stake_availability(coldkey, netuid);
        available
    }

    /// Ensures that the amount can be unstaked
    pub fn ensure_available_to_unstake(
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        let alpha_available = Self::available_to_unstake(coldkey, netuid);
        ensure!(alpha_available >= amount, Error::<T>::StakeUnavailable);
        Ok(())
    }
}
