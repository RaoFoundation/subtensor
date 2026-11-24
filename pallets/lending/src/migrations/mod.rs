//! One-shot storage migrations for the lending pallet.
use core::marker::PhantomData;

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;
#[cfg(feature = "try-runtime")]
use codec::{Decode, Encode};
use frame_support::{
    BoundedVec,
    storage::{storage_prefix, unhashed},
    traits::{OnRuntimeUpgrade, PalletInfoAccess},
    weights::Weight,
};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

use crate::*;

/// Storage items of the lift-based lending pallet. Every key under these
/// prefixes is dropped; items that kept their name but changed their value
/// type (`Positions`, `Due`, `Dissolutions`, …) are listed too.
pub const RETIRED_ITEMS: &[&str] = &[
    "LiftParamsStore",
    "Lifts",
    "InvariantBaseline",
    "LongInterestPending",
    "LongInterestSince",
    "Positions",
    "OpenByNetuid",
    "PositionCount",
    "TotalPositions",
    "EscrowOwner",
    "References",
    "Dissolutions",
    "DissolutionCursor",
    "DissolutionPhase",
    "Due",
    "DueIndex",
    "DueNext",
    "DuePrev",
    "NextDue",
    "DueTail",
    "DueTailRebuildCursor",
    "DueBucketCursor",
    "Expiry",
    "ExpiryIndex",
    "ExpiryNext",
    "ExpiryPrev",
    "NextExpiry",
    "ExpiryTail",
    "ExpiryBucketCursor",
    "InterestRetry",
    "NextInterestRetry",
    "InterestRetryPending",
    "InterestRetryCursor",
    "ExpiryRetry",
    "NextExpiryRetry",
    "ExpiryRetryPending",
    "ExpiryRetryCursor",
    "ExpiryAttemptsLeft",
    "LastDeferral",
    "BandDeferralSince",
    "ProtocolForcedImpact",
    "BenchCombinedResidualSale",
    "ConversionCursor",
    "ActiveNets",
    "ActiveNetCount",
    "TaoEma",
    "StatsByNetuid",
    "StatsGlobal",
    "CurrentTermsVersion",
    "DeregMaBlocks",
];

pub fn synth_shorts_v1_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_to_synth_shorts_v1".to_vec())
}

#[cfg(any(test, feature = "try-runtime"))]
pub(crate) fn retired_keys_left<T: Config>() -> u32 {
    let pallet = <Pallet<T> as PalletInfoAccess>::name().as_bytes();
    RETIRED_ITEMS
        .iter()
        .map(|item| {
            let prefix = storage_prefix(pallet, item.as_bytes());
            let mut count = 0u32;
            let mut key = prefix.to_vec();
            while let Some(next) = sp_io::storage::next_key(&key) {
                if !next.starts_with(&prefix) {
                    break;
                }
                count = count.saturating_add(1);
                key = next;
            }
            count
        })
        .fold(0, u32::saturating_add)
}

/// Drop every lift-era storage item, then install [`SynthParams`] defaults
/// with [`Enabled`] off until root turns synthetic shorts on. Keeps
/// [`PalletHotkey`] and [`HasMigrationRun`]. Idempotent.
///
/// Lift-era positions are not unwound: their custody balances stay on the
/// custody accounts. Run only where no lift-era position is open.
pub fn migrate_to_synth_shorts_v1<T: Config>() -> Weight {
    let key = synth_shorts_v1_key();
    let db = T::DbWeight::get();
    let mut weight = db.reads(1);
    if HasMigrationRun::<T>::get(&key) {
        return weight;
    }
    let pallet = <Pallet<T> as PalletInfoAccess>::name().as_bytes();
    for item in RETIRED_ITEMS {
        let prefix = storage_prefix(pallet, item.as_bytes());
        let removed = unhashed::clear_prefix(&prefix, None, None);
        let n = u64::from(removed.loops.max(1));
        weight = weight.saturating_add(db.reads_writes(n, n));
    }
    SynthParamsStore::<T>::put(SynthParams::default());
    Enabled::<T>::put(false);
    HasMigrationRun::<T>::insert(&key, true);
    weight.saturating_add(db.writes(3))
}

pub struct MigrateToSynthShortsV1<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for MigrateToSynthShortsV1<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_to_synth_shorts_v1::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        Ok(HasMigrationRun::<T>::get(synth_shorts_v1_key()).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let already_run: bool =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(synth_shorts_v1_key()),
            "synth-shorts migration must be stamped"
        );
        if !already_run {
            ensure!(
                retired_keys_left::<T>() == 0,
                "lift-era storage must be empty"
            );
            ensure!(!Enabled::<T>::get(), "synthetic shorts must start disabled");
            ensure!(
                SynthParamsStore::<T>::get() == SynthParams::default(),
                "SynthParams must hold defaults"
            );
        }
        Pallet::<T>::do_try_state()
    }
}
