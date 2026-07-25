//! Create, top up, reduce, and aggregate conviction locks for a coldkey.
use super::*;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Locks stake for a coldkey on a subnet to a specific hotkey.
    /// If no lock exists, creates one. If one exists, the hotkey must match.
    /// Top-up adds to locked_mass after rolling forward.
    pub fn do_lock_stake(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        amount: AlphaBalance,
    ) -> dispatch::DispatchResult {
        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);
        ensure!(!amount.is_zero(), Error::<T>::AmountTooLow);
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        let total = Self::total_coldkey_alpha_on_subnet(coldkey, netuid);
        let now = Self::get_current_block_as_u64();

        let mut model = match Self::read_conviction_model(coldkey, netuid, now) {
            Some((existing_hotkey, model)) => {
                ensure!(*hotkey == existing_hotkey, Error::<T>::LockHotkeyMismatch);
                model
            }
            None => Self::read_conviction_model_for_hotkey(coldkey, netuid, hotkey, now),
        };
        model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());

        if model.individual_lock().locked_mass.is_zero()
            && model.individual_lock().conviction == U64F64::saturating_from_num(0)
        {
            ensure!(total >= amount, Error::<T>::InsufficientStakeForLock);

            model.set_rolled_individual_lock(
                LockState {
                    locked_mass: amount,
                    conviction: U64F64::saturating_from_num(0),
                    last_update: now,
                },
                now,
                UnlockRate::<T>::get(),
                MaturityRate::<T>::get(),
            );
        } else {
            let mut lock = model.individual_lock().clone();
            lock.locked_mass = lock.locked_mass.saturating_add(amount);
            ensure!(
                total >= lock.locked_mass,
                Error::<T>::InsufficientStakeForLock
            );
            model.set_rolled_individual_lock(
                lock,
                now,
                UnlockRate::<T>::get(),
                MaturityRate::<T>::get(),
            );
        }

        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        model.add_to_aggregate(&LockState {
            locked_mass: amount,
            conviction: U64F64::saturating_from_num(0),
            last_update: now,
        });
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        Self::save_conviction_model(coldkey, netuid, hotkey, model);

        Self::deposit_event(Event::StakeLocked {
            coldkey: coldkey.clone(),
            hotkey: hotkey.clone(),
            netuid,
            amount,
        });

        Ok(())
    }

    /// Reduces the coldkey lock by a specified alpha amount and the coldkey conviction
    /// proportionally.
    pub fn force_reduce_lock(coldkey: &T::AccountId, netuid: NetUid, amount: AlphaBalance) {
        let now = Self::get_current_block_as_u64();
        if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            model.force_reduce_individual(amount, now);
            Self::save_conviction_model(coldkey, netuid, &hotkey, model);
        }
    }

    /// Rolls the lock forward to now and persists it if the locked mass is zero. This is used when we want to
    /// update the lock when a user stakes or unstakes.
    pub fn cleanup_lock_if_zero(coldkey: &T::AccountId, netuid: NetUid) {
        let now = Self::get_current_block_as_u64();

        // Cleanup locks for the specific coldkey and hotkey
        if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
            model.roll_forward(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
            Self::save_conviction_model(coldkey, netuid, &hotkey, model);
        }
    }

    /// Update the total lock for a hotkey on a subnet or create one if
    /// it doesn't exist.
    ///
    /// Roll the existing hotkey lock forward to now, then add the
    /// latest conviction and locked mass.
    pub fn upsert_aggregate_lock(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) {
        let now = Self::get_current_block_as_u64();
        Self::add_aggregate_lock(
            coldkey,
            hotkey,
            netuid,
            LockState {
                locked_mass: amount,
                conviction: U64F64::saturating_from_num(0),
                last_update: now,
            },
        );
    }

    /// Merges an already-existing lock state into the aggregate lock bucket.
    ///
    /// This is used when lock state moves between keys, such as lock moves, stake
    /// transfers, or coldkey swaps. Unlike `upsert_aggregate_lock`, this preserves
    /// both locked mass and conviction from the moved lock because that conviction
    /// was already earned before the aggregate bucket changed.
    ///
    /// Locks to the subnet owner hotkey are merged into `OwnerLock`; all other
    /// locks are merged into the destination hotkey's perpetual or decaying bucket.
    pub(crate) fn add_aggregate_lock(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        added: LockState,
    ) {
        let now = Self::get_current_block_as_u64();
        let mut model = Self::read_conviction_model_for_hotkey(coldkey, netuid, hotkey, now);
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        model.add_to_aggregate(&added);
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        Self::save_conviction_model(coldkey, netuid, hotkey, model);
    }

    /// Reduces locked mass and conviction from exactly one aggregate bucket.
    pub(crate) fn reduce_aggregate_lock(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
        conviction: U64F64,
    ) {
        let now = Self::get_current_block_as_u64();
        let mut model = Self::read_conviction_model_for_hotkey(coldkey, netuid, hotkey, now);
        model.roll_forward_aggregate(now, UnlockRate::<T>::get(), MaturityRate::<T>::get());
        model.reduce_aggregate(amount, conviction);
        Self::save_conviction_model(coldkey, netuid, hotkey, model);
    }
}
