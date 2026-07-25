//! Hotkey alpha share-pool storage adapter ([`HotkeyAlphaSharePoolDataOperations`]).
//!
//! Backs `SharePool` so each coldkey's alpha share on a `(hotkey, netuid)` is
//! stored in the `Alpha` map with a shared denominator.
use super::*;
use share_pool::{SafeFloat, SharePool, SharePoolDataOperations};
use subtensor_runtime_common::NetUid;

///////////////////////////////////////////
// Alpha share pool chain data layer

#[derive(Debug)]
pub struct HotkeyAlphaSharePoolDataOperations<T: frame_system::Config> {
    netuid: NetUid,
    hotkey: <T as frame_system::Config>::AccountId,
    _marker: sp_std::marker::PhantomData<T>,
}

impl<T: Config> HotkeyAlphaSharePoolDataOperations<T> {
    pub(crate) fn new(hotkey: <T as frame_system::Config>::AccountId, netuid: NetUid) -> Self {
        HotkeyAlphaSharePoolDataOperations {
            netuid,
            hotkey,
            _marker: sp_std::marker::PhantomData,
        }
    }
}

// Alpha share key is coldkey because the HotkeyAlphaSharePoolDataOperations struct already has hotkey and netuid
pub(crate) type AlphaShareKey<T> = <T as frame_system::Config>::AccountId;

impl<T: Config> SharePoolDataOperations<AlphaShareKey<T>>
    for HotkeyAlphaSharePoolDataOperations<T>
{
    fn get_shared_value(&self) -> u64 {
        u64::from(TotalHotkeyAlpha::<T>::get(&self.hotkey, self.netuid))
    }

    fn get_share(&self, key: &AlphaShareKey<T>) -> SafeFloat {
        // Read the deprecated Alpha map first and, if value is not available, try new AlphaV2
        let maybe_share_v1 = Alpha::<T>::try_get((&(self.hotkey), key, self.netuid));
        if let Ok(share_v1) = maybe_share_v1 {
            return SafeFloat::from(share_v1);
        }

        AlphaV2::<T>::get((&(self.hotkey), key, self.netuid))
    }

    fn try_get_share(&self, key: &AlphaShareKey<T>) -> Result<SafeFloat, ()> {
        // Read the deprecated Alpha map first and, if value is not available, try new AlphaV2
        let maybe_share_v1 = Alpha::<T>::try_get((&(self.hotkey), key, self.netuid));
        if let Ok(share_v1) = maybe_share_v1 {
            return Ok(SafeFloat::from(share_v1));
        }

        let maybe_share = AlphaV2::<T>::try_get((&(self.hotkey), key, self.netuid));
        if let Ok(share) = maybe_share {
            Ok(share)
        } else {
            Err(())
        }
    }

    fn get_denominator(&self) -> SafeFloat {
        // Read the deprecated TotalHotkeyShares map first and, if value is not available, try new TotalHotkeySharesV2
        let maybe_denomnator_v1 = TotalHotkeyShares::<T>::try_get(&(self.hotkey), self.netuid);
        if let Ok(denomnator_v1) = maybe_denomnator_v1 {
            return SafeFloat::from(denomnator_v1);
        }

        TotalHotkeySharesV2::<T>::get(&(self.hotkey), self.netuid)
    }

    fn set_shared_value(&mut self, value: u64) {
        if value != 0 {
            TotalHotkeyAlpha::<T>::insert(&(self.hotkey), self.netuid, AlphaBalance::from(value));
        } else {
            TotalHotkeyAlpha::<T>::remove(&(self.hotkey), self.netuid);
        }
    }

    fn set_share(&mut self, key: &AlphaShareKey<T>, share: SafeFloat) {
        // Lazy Alpha -> AlphaV2 migration happens right here
        // Delete the Alpha entry, insert into AlphaV2
        let maybe_share_v1 = Alpha::<T>::try_get((&(self.hotkey), key, self.netuid));
        if maybe_share_v1.is_ok() {
            Alpha::<T>::remove((&self.hotkey, key, self.netuid));
        }

        if !share.is_zero() {
            AlphaV2::<T>::insert((&self.hotkey, key, self.netuid), share);
        } else {
            AlphaV2::<T>::remove((&self.hotkey, key, self.netuid));
        }
    }

    fn set_denominator(&mut self, update: SafeFloat) {
        // Lazy TotalHotkeyShares -> TotalHotkeySharesV2 migration happens right here
        // Delete the TotalHotkeyShares entry, insert into TotalHotkeySharesV2
        let maybe_denominator_v1 = TotalHotkeyShares::<T>::try_get(&(self.hotkey), self.netuid);
        if maybe_denominator_v1.is_ok() {
            TotalHotkeyShares::<T>::remove(&self.hotkey, self.netuid);
        }

        if !update.is_zero() {
            TotalHotkeySharesV2::<T>::insert(&self.hotkey, self.netuid, update);
        } else {
            TotalHotkeySharesV2::<T>::remove(&self.hotkey, self.netuid);
        }
    }
}

impl<T: Config> Pallet<T> {
    pub fn get_alpha_share_pool(
        hotkey: <T as frame_system::Config>::AccountId,
        netuid: NetUid,
    ) -> SharePool<AlphaShareKey<T>, HotkeyAlphaSharePoolDataOperations<T>> {
        let ops = HotkeyAlphaSharePoolDataOperations::new(hotkey, netuid);
        SharePool::<AlphaShareKey<T>, HotkeyAlphaSharePoolDataOperations<T>>::new(ops)
    }
}
