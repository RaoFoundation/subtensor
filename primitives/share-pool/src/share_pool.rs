//! [`SharePool`]: proportional rao ownership keyed by coldkey/hotkey (or any `Eq` key).

use sp_std::marker;
use sp_std::ops::Neg;

use crate::SafeFloat;

/// Persistence backend for a [`SharePool`]: total rao, per-key shares, denominator.
pub trait SharePoolDataOperations<Key> {
    /// Total shared value in integer rao (not a fractional share).
    fn get_shared_value(&self) -> u64;
    /// Share units held by `key` (zero if unset).
    fn get_share(&self, key: &Key) -> SafeFloat;
    /// Share units for `key`, or `Err(())` if the key has no entry.
    fn try_get_share(&self, key: &Key) -> Result<SafeFloat, ()>;
    /// Sum of all share units in the pool (denominator for ownership ratios).
    fn get_denominator(&self) -> SafeFloat;
    /// Replace the total shared rao value.
    fn set_shared_value(&mut self, value: u64);
    /// Replace the share units stored for `key`.
    fn set_share(&mut self, key: &Key, share: SafeFloat);
    /// Replace the pool denominator.
    fn set_denominator(&mut self, update: SafeFloat);
}

/// Proportional ownership pool: each key owns `share / denominator` of `shared_value` rao.
#[derive(Debug)]
pub struct SharePool<K, Ops>
where
    K: Eq,
    Ops: SharePoolDataOperations<K>,
{
    /// Storage backend; `pub(crate)` so unit tests can seed pool state directly.
    pub(crate) state_ops: Ops,
    phantom_key: marker::PhantomData<K>,
}

impl<K, Ops> SharePool<K, Ops>
where
    K: Eq,
    Ops: SharePoolDataOperations<K>,
{
    /// Wrap a storage backend; no pool state is created until the first update.
    pub fn new(ops: Ops) -> Self {
        SharePool {
            state_ops: ops,
            phantom_key: marker::PhantomData,
        }
    }

    /// Absolute rao owned by `key`: `shared_value * share(key) / denominator`.
    pub fn get_value(&self, key: &K) -> u64 {
        let shared_value: SafeFloat =
            SafeFloat::new(self.state_ops.get_shared_value() as u128, 0).unwrap_or_default();
        let current_share: SafeFloat = self.state_ops.get_share(key);
        let denominator: SafeFloat = self.state_ops.get_denominator();
        shared_value
            .mul_div(&current_share, &denominator)
            .unwrap_or_default()
            .into()
    }

    /// Absolute rao for an arbitrary share amount (without looking up a key).
    pub fn get_value_from_shares(&self, current_share: SafeFloat) -> u64 {
        let shared_value: SafeFloat =
            SafeFloat::new(self.state_ops.get_shared_value() as u128, 0).unwrap_or_default();
        let denominator: SafeFloat = self.state_ops.get_denominator();
        shared_value
            .mul_div(&current_share, &denominator)
            .unwrap_or_default()
            .into()
    }

    /// Like [`Self::get_value`], but `Err` if `key` has no share entry.
    pub fn try_get_value(&self, key: &K) -> Result<u64, ()> {
        match self.state_ops.try_get_share(key) {
            Ok(_) => Ok(self.get_value(key)),
            Err(i) => Err(i),
        }
    }

    /// Apply a signed rao delta to the shared total; every key's absolute value scales with it.
    pub fn update_value_for_all(&mut self, update: i64) {
        let shared_value: u64 = self.state_ops.get_shared_value();
        self.state_ops.set_shared_value(if update >= 0 {
            shared_value.saturating_add(update as u64)
        } else {
            shared_value.saturating_sub(update.neg() as u64)
        });
    }

    /// Dry-run whether a non-zero share delta would result from `update` for some key.
    ///
    /// Does not mutate share state; used by staking to reject dust updates.
    pub fn sim_update_value_for_one(&mut self, update: i64) -> bool {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let denominator: SafeFloat = self.state_ops.get_denominator();

        // Then, update this key's share
        if denominator.is_zero() {
            true
        } else {
            // There are already keys in the pool, set or update this key
            let shares_per_update =
                self.shares_for_value_update(update, shared_value, &denominator);

            !shares_per_update.is_zero()
        }
    }

    /// Share units corresponding to an absolute rao `update` at current pool scale.
    pub(crate) fn shares_for_value_update(
        &self,
        update: i64,
        shared_value: u64,
        denominator: &SafeFloat,
    ) -> SafeFloat {
        let shared_value: SafeFloat = SafeFloat::new(shared_value as u128, 0).unwrap_or_default();
        let update_sf: SafeFloat =
            SafeFloat::new(update.unsigned_abs() as u128, 0).unwrap_or_default();
        update_sf
            .mul_div(denominator, &shared_value)
            .unwrap_or_default()
    }

    /// Apply a signed rao delta to one key's ownership and to the shared total.
    ///
    /// Initializes the pool on the first non-empty update (that key gets all shares).
    /// SafeFloat overflows are logged and skipped rather than panicking.
    pub fn update_value_for_one(&mut self, key: &K, update: i64) {
        let shared_value: u64 = self.state_ops.get_shared_value();
        let current_share: SafeFloat = self.state_ops.get_share(key);
        let denominator: SafeFloat = self.state_ops.get_denominator();

        // Then, update this key's share
        if denominator.is_zero() {
            // Initialize the pool. The first key gets all.
            let update_float: SafeFloat =
                SafeFloat::new(update.unsigned_abs() as u128, 0).unwrap_or_default();
            self.state_ops.set_denominator(update_float.clone());
            self.state_ops.set_share(key, update_float);
        } else {
            let new_denominator;
            let new_current_share;

            let shares_per_update: SafeFloat =
                self.shares_for_value_update(update, shared_value, &denominator);

            // Handle SafeFloat overflows quietly here because this overflow of i64 exponent
            // is extremely hypothetical and should never happen in practice.
            if update > 0 {
                new_denominator = match denominator.add(&shares_per_update) {
                    Some(new_denominator) => new_denominator,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old denominator",
                            shares_per_update,
                            denominator,
                        );
                        // Return the value as it was before the failed addition
                        denominator
                    }
                };

                new_current_share = match current_share.add(&shares_per_update) {
                    Some(new_current_share) => new_current_share,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old current_share",
                            shares_per_update,
                            current_share,
                        );
                        // Return the value as it was before the failed addition
                        current_share
                    }
                };
            } else {
                new_denominator = match denominator.sub(&shares_per_update) {
                    Some(new_denominator) => new_denominator,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old denominator",
                            shares_per_update,
                            denominator,
                        );
                        // Return the value as it was before the failed addition
                        denominator
                    }
                };

                new_current_share = match current_share.sub(&shares_per_update) {
                    Some(new_current_share) => new_current_share,
                    None => {
                        log::error!(
                            "SafeFloat::add overflow when adding {:?} to {:?}; keeping old current_share",
                            shares_per_update,
                            current_share,
                        );
                        // Return the value as it was before the failed addition
                        current_share
                    }
                };
            }

            self.state_ops.set_denominator(new_denominator);
            self.state_ops.set_share(key, new_current_share);
        }

        // Update shared value
        self.update_value_for_all(update);
    }
}
