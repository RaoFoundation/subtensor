use super::*;

impl<T: Config> Pallet<T> {
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
        let r = T::DbWeight::get().reads(1);
        let w = T::DbWeight::get().writes(writes_per_match);
        let mut read_all = true;

        let mut to_rm: sp_std::vec::Vec<K> = sp_std::vec::Vec::new();
        let mut last_item = None;
        for item in iter {
            if !weight_meter.can_consume(r) {
                read_all = false;
                break;
            }
            weight_meter.consume(r);
            if matches_netuid(&item) {
                if !weight_meter.can_consume(w) {
                    read_all = false;
                    break;
                }
                weight_meter.consume(w);

                to_rm.push(key_from_item(item.clone()));
            }
            last_item = Some(item);
        }

        for hot in to_rm {
            ops_based_on_key(&hot);
        }

        (read_all, last_item)
    }
}
