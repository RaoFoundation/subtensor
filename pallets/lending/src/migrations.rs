//! Storage migrations. `main` has no earlier lending storage (DESIGN-v2 section 10 item 2), so
//! the only one is the v1 initialisation (DESIGN-v2 section 8).

use frame_support::{
    traits::{Get, GetStorageVersion, StorageVersion},
    weights::Weight,
};

use crate::{
    Config, DeregParamsStore, LendingParamsStore, LendingPoolInterface, LongParamsStore, Pallet,
    ShortParamsStore, WeightInfo,
    params::{DeregParams, LendingParams, LongParams, ShortParams},
};

/// v0 -> v1: writes the launch parameters and prefills every live subnet's price rings,
/// whether lending is enabled or not (SPEC section 9 item 1, I25). The rank ring is seeded
/// from `SubnetMovingPrice` (the read permitted by SPEC 8.7) and the TWAP ring from spot.
/// `Enabled` and `LongsEnabled` are left false (SPEC P1, L1). Runs once.
pub fn migrate_to_v1<T: Config>() -> Weight {
    if Pallet::<T>::on_chain_storage_version() >= 1 {
        return T::DbWeight::get().reads(1);
    }
    LendingParamsStore::<T>::put(LendingParams::default());
    ShortParamsStore::<T>::put(ShortParams::default());
    LongParamsStore::<T>::put(LongParams::default());
    DeregParamsStore::<T>::put(DeregParams::default());

    let subnets = T::Pool::live_subnets();
    for netuid in subnets.iter().copied() {
        Pallet::<T>::seed_subnet(netuid, T::Pool::moving_alpha_price(netuid));
    }
    StorageVersion::new(1).put::<Pallet<T>>();
    T::WeightInfo::migrate_to_v1(u32::try_from(subnets.len()).unwrap_or(u32::MAX))
}
