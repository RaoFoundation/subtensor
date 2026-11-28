//! Storage migrations. `main` has no earlier lending storage (DESIGN-v2 section 10 item 2), so
//! the only one is the v1 initialisation (DESIGN-v2 section 8).

use core::marker::PhantomData;

use frame_support::{
    BoundedVec,
    traits::{ConstU32, Get, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};

use crate::{
    Config, DeregParamsStore, HasMigrationRun, LendingParamsStore, LendingPoolInterface,
    LongParamsStore, Pallet, ShortParamsStore, WeightInfo,
    params::{DeregParams, LendingParams, LongParams, ShortParams},
};

/// Key of [`migrate_to_v1`] in `HasMigrationRun`.
const V1_KEY: &[u8] = b"lending_v1";

fn v1_key() -> BoundedVec<u8, ConstU32<32>> {
    BoundedVec::truncate_from(V1_KEY.to_vec())
}

/// Whether [`migrate_to_v1`] or a genesis build has initialised the pallet.
#[must_use]
pub fn has_run<T: Config>() -> bool {
    HasMigrationRun::<T>::get(v1_key())
}

fn mark_run<T: Config>() {
    HasMigrationRun::<T>::insert(v1_key(), true);
}

/// v0 -> v1: writes the launch parameters and prefills every live subnet's price rings,
/// whether lending is enabled or not (SPEC section 9 item 1, I25). The rank ring is seeded
/// from `SubnetMovingPrice` (the read permitted by SPEC 8.7) and the TWAP ring from spot.
/// `Enabled` and `LongsEnabled` are left false (SPEC P1, L1). It also registers the pallet hotkey
/// ([`Pallet::initialize_custody`], itself idempotent); a failure there leaves the rest of the
/// initialisation in place. Runs once: guarded by its `HasMigrationRun` entry, which a genesis
/// build sets too.
pub fn migrate_to_v1<T: Config>() -> Weight {
    if has_run::<T>() {
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
    // Best effort here: the hotkey is unregistered only if another account already owns that
    // address, and `initialize_custody` can be called again and checked by the runtime upgrade.
    let _ = Pallet::<T>::initialize_custody();
    StorageVersion::new(1).put::<Pallet<T>>();
    mark_run::<T>();
    // The measured run registers a pallet hotkey that the chain already knows; the first
    // registration also writes the hotkey's owner.
    T::DbWeight::get()
        .writes(1)
        .saturating_add(T::WeightInfo::migrate_to_v1(
            u32::try_from(subnets.len()).unwrap_or(u32::MAX),
        ))
}

/// [`migrate_to_v1`] as a runtime migration: the runtime lists it in its `Migrations` tuple.
/// Guarded by its `HasMigrationRun` entry, so a second run reads one value and writes nothing.
pub struct MigrateToV1<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for MigrateToV1<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_to_v1::<T>()
    }

    /// Records whether this run is the one that initialises the pallet.
    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
        use codec::Encode;
        Ok((!has_run::<T>()).encode())
    }

    /// After any run: the storage version is 1, the pallet hotkey is registered and every live
    /// subnet has full price rings (SPEC I25, with the other invariants of `do_try_state`).
    /// After the initialising run also: the four parameter groups are the launch defaults, no
    /// lending parameter change is pending and both sides are disabled (SPEC P1, L1). Root may
    /// have changed either since, so a later run does not check them.
    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        use codec::Decode;
        use frame_support::{ensure, traits::GetStorageVersion};

        let initialising = bool::decode(&mut state.as_slice())
            .map_err(|_| "lending v1: pre_upgrade state does not decode")?;
        ensure!(
            Pallet::<T>::on_chain_storage_version() == 1,
            "lending v1: storage version is not 1"
        );
        ensure!(
            has_run::<T>(),
            "lending v1: the migration is not marked as run"
        );
        ensure!(
            Pallet::<T>::initialize_custody().is_ok(),
            "lending v1: the pallet hotkey is owned by another account"
        );
        for netuid in T::Pool::live_subnets() {
            ensure!(
                Pallet::<T>::price_refs(netuid).is_ok(),
                "lending v1: a live subnet has no price rings"
            );
        }
        Pallet::<T>::do_try_state()?;
        if initialising {
            ensure!(
                LendingParamsStore::<T>::get() == LendingParams::default()
                    && ShortParamsStore::<T>::get() == ShortParams::default()
                    && LongParamsStore::<T>::get() == LongParams::default()
                    && DeregParamsStore::<T>::get() == DeregParams::default(),
                "lending v1: parameters are not the launch defaults"
            );
            ensure!(
                crate::PendingLendingParams::<T>::get().is_none(),
                "lending v1: a lending parameter change is pending after the upgrade"
            );
            ensure!(
                !crate::Enabled::<T>::get(),
                "lending v1: lending is enabled after the upgrade"
            );
            ensure!(
                !crate::LongsEnabled::<T>::get(),
                "lending v1: longs are enabled after the upgrade"
            );
        }
        Ok(())
    }
}

/// The initialisation of a chain that starts with this pallet: a genesis build sets the
/// storage version itself, so [`migrate_to_v1`] never runs there. Seeds the price rings of
/// the genesis subnets and registers the pallet hotkey; the parameters read as their defaults
/// and both sides stay disabled.
pub fn initialize_at_genesis<T: Config>() {
    for netuid in T::Pool::live_subnets() {
        Pallet::<T>::seed_subnet(netuid, T::Pool::moving_alpha_price(netuid));
    }
    let _ = Pallet::<T>::initialize_custody();
    mark_run::<T>();
}
