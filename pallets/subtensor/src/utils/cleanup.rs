//! Weight-metered deletion of storage entries keyed by `netuid`.
//!
//! Used by subnet dissolution and stake-cleanup paths that must stop when the
//! remaining weight budget cannot cover another read or write.

use super::*;

impl<T: Config> Pallet<T> {
    /// Scan `iter`, collect keys for items matching `netuid`, then delete them.
    ///
    /// Removals are deferred until after the scan so mutating storage while
    /// iterating the same prefix is safe. Returns `(read_all, last_item)` where
    /// `read_all` is `false` if the weight meter ran out mid-scan (caller should
    /// resume later from `last_item`).
    pub fn remove_storage_entries_for_netuid<I, K>(
        weight_meter: &mut WeightMeter,
        iter: I,
        matches_netuid: impl Fn(&I::Item) -> bool,
        key_from_item: impl Fn(I::Item) -> K,
        ops_based_on_key: impl Fn(&K),
        writes_per_match: u64,
    ) -> (bool, Option<I::Item>)
    where
        I: Iterator,
        I::Item: Clone,
    {
        let read_weight = T::DbWeight::get().reads(1);
        let write_weight = T::DbWeight::get().writes(writes_per_match);
        let mut read_all = true;

        let mut keys_to_remove: sp_std::vec::Vec<K> = sp_std::vec::Vec::new();
        let mut last_item = None;
        for item in iter {
            if !weight_meter.can_consume(read_weight) {
                read_all = false;
                break;
            }
            weight_meter.consume(read_weight);
            if matches_netuid(&item) {
                if !weight_meter.can_consume(write_weight) {
                    read_all = false;
                    break;
                }
                weight_meter.consume(write_weight);

                keys_to_remove.push(key_from_item(item.clone()));
            }
            last_item = Some(item);
        }

        for key in keys_to_remove {
            ops_based_on_key(&key);
        }

        (read_all, last_item)
    }
}
