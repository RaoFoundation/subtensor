//! Rebuild associations lost before the alpha-key retention fix in spec 473.
//!
//! Scan authoritative share rows, rather than the incomplete StakingHotkeys index or
//! a snapshot list. Zero and retired share rows also keep an association alive under
//! staking_hotkey_must_remain. Nonzero basket watermarks need the index independently
//! of alpha. Run after conversion and the existing destructive cleanup have finished.

use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::traits::fungible::Inspect;
use frame_support::{
    storage_alias,
    traits::{Get, OnRuntimeUpgrade},
    weights::Weight,
};
use scale_info::TypeInfo;
use sp_std::marker::PhantomData;

pub const MIGRATION_NAME: &[u8] = b"migrate_restore_staking_hotkeys_v1";
const MAX_ROWS_PER_BLOCK: usize = 64;

#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, Default)]
pub struct RestoreProgress {
    /// 0: AlphaV2; 1: BasketClaimed; 2: complete.
    pub phase: u8,
    pub after: Option<Vec<u8>>,
    pub scanned: u64,
    pub restored: u64,
}

#[storage_alias]
pub type StakingHotkeysRestoreMigration<T: Config> =
    StorageValue<Pallet<T>, RestoreProgress, OptionQuery>;

pub fn kickoff<T: Config>() -> Weight {
    let mut used = T::DbWeight::get().reads(2);
    if !HasMigrationRun::<T>::get(MIGRATION_NAME) && !StakingHotkeysRestoreMigration::<T>::exists()
    {
        StakingHotkeysRestoreMigration::<T>::put(RestoreProgress::default());
        used.saturating_accrue(T::DbWeight::get().writes(1));
    }
    used
}

pub struct Migration<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        kickoff::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
        Ok(<T as Config>::Currency::total_issuance().encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        let issuance = TaoBalance::decode(&mut &state[..])
            .map_err(|_| "invalid staking-hotkeys restore pre-upgrade state")?;
        ensure!(
            HasMigrationRun::<T>::get(MIGRATION_NAME)
                || StakingHotkeysRestoreMigration::<T>::exists(),
            "staking-hotkeys restoration was not scheduled"
        );
        ensure!(
            <T as Config>::Currency::total_issuance() == issuance,
            "staking-hotkeys restoration changed issuance"
        );
        Ok(())
    }
}

pub fn continue_restore<T: Config>(limit: Weight) -> Weight {
    // Reserve the status read and progress/completion writes before doing any work.
    let reserve = T::DbWeight::get().reads_writes(1, 2);
    if !reserve.all_lte(limit) {
        return Weight::zero();
    }
    let mut used = T::DbWeight::get().reads(1);
    let Some(mut progress) = StakingHotkeysRestoreMigration::<T>::get() else {
        return used;
    };
    if progress.phase == 2 {
        return used;
    }
    let available = limit.saturating_sub(T::DbWeight::get().writes(2));

    for _ in 0..MAX_ROWS_PER_BLOCK {
        // next_key and value load, plus decode_len for the destination vector.
        let probe = T::DbWeight::get().reads(3);
        if !used.saturating_add(probe).all_lte(available) {
            break;
        }
        // Drop the iterator before changing any storage. The cursor advances only
        // when the relationship has been handled, so a small budget retries the row.
        let next = if progress.phase == 0 {
            let mut iter = match &progress.after {
                Some(after) => AlphaV2::<T>::iter_from(after.clone()),
                None => AlphaV2::<T>::iter(),
            };
            iter.next().map(|((hotkey, coldkey, netuid), _)| {
                let raw = AlphaV2::<T>::hashed_key_for((&hotkey, &coldkey, netuid));
                (hotkey, coldkey, raw, true)
            })
        } else {
            let mut iter = match &progress.after {
                Some(after) => BasketClaimed::<T>::iter_from(after.clone()),
                None => BasketClaimed::<T>::iter(),
            };
            iter.next().map(|(hotkey, coldkey, claimed)| {
                let raw = BasketClaimed::<T>::hashed_key_for(&hotkey, &coldkey);
                (hotkey, coldkey, raw, claimed != 0)
            })
        };
        used.saturating_accrue(T::DbWeight::get().reads(2));
        let Some((hotkey, coldkey, raw, must_remain)) = next else {
            progress.phase = progress.phase.saturating_add(1);
            progress.after = None;
            if progress.phase == 2 {
                HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
                used.saturating_accrue(T::DbWeight::get().writes(1));
                break;
            }
            continue;
        };
        if must_remain {
            // Reuse the existing conservative, length-dependent vector-walk
            // allowance, including oversized historical lists. Repair must not
            // silently omit existing positions at the admission cap.
            let rewrite = Pallet::<T>::staking_hotkeys_walk_actual(&coldkey)
                .saturating_add(T::DbWeight::get().writes(1));
            used.saturating_accrue(T::DbWeight::get().reads(1));
            if !used.saturating_add(rewrite).all_lte(available) {
                break;
            }
            let mut hotkeys = StakingHotkeys::<T>::get(&coldkey);
            if !hotkeys.contains(&hotkey) {
                hotkeys.push(hotkey);
                StakingHotkeys::<T>::insert(&coldkey, hotkeys);
                progress.restored = progress.restored.saturating_add(1);
            }
            used.saturating_accrue(rewrite);
        }
        progress.after = Some(raw);
        progress.scanned = progress.scanned.saturating_add(1);
    }
    // Keep final counters for chain audits and make upgrades idempotent.
    StakingHotkeysRestoreMigration::<T>::put(progress);
    used.saturating_add(T::DbWeight::get().writes(1))
}
