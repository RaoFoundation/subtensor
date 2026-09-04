use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{storage_alias, traits::Get, weights::Weight};
use scale_info::TypeInfo;
use scale_info::prelude::string::String;
use sp_io::{hashing::twox_128, storage};
use sp_std::vec::Vec;

// Fresh marker reruns the bounded GC with the AlphaV2 zero-row target added below. The original
// v2 pass may already be marked complete on-chain.
const MIGRATION_NAME: &[u8] = b"migrate_storage_bloat_v3";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CleanupMode {
    Clear,
    ClearIfZero,
}

#[derive(Clone, Copy)]
struct CleanupTarget {
    pallet: &'static str,
    storage: &'static str,
    mode: CleanupMode,
}

// The undeclared prefixes are dead pre-dTAO state. The remaining targets use ValueQuery (or an
// OptionQuery whose consumer treats a missing value as zero), so deleting a zero/default row does
// not change the value observed by runtime callers.
const TARGETS: &[CleanupTarget] = &[
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "TotalHotkeyStake",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "PendingdHotkeyEmission",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "PendingdHotkeyEmissionUntouchable",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "LastHotkeyEmissionDrain",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "StakeDeltaSinceLastEmissionDrain",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "TotalColdkeyStake",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "LastAddStakeIncrease",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "ColdkeyArbitrationBlock",
        mode: CleanupMode::Clear,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "Alpha",
        mode: CleanupMode::ClearIfZero,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "TotalHotkeyShares",
        mode: CleanupMode::ClearIfZero,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "TotalHotkeyAlpha",
        mode: CleanupMode::ClearIfZero,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "TotalHotkeyAlphaLastEpoch",
        mode: CleanupMode::ClearIfZero,
    },
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "StakingHotkeys",
        mode: CleanupMode::ClearIfZero,
    },
    // Appended so an interrupted v2 cursor keeps the same target indices after upgrade.
    CleanupTarget {
        pallet: "SubtensorModule",
        storage: "AlphaV2",
        mode: CleanupMode::ClearIfZero,
    },
];

/// Persistent progress for the bounded storage cleanup.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct StorageBloatCleanupProgress {
    /// Index into [`TARGETS`].
    pub target: u16,
    /// Last raw key visited in the current prefix. An empty cursor starts at the prefix.
    pub cursor: Vec<u8>,
    /// Rows inspected across all passes.
    pub scanned: u64,
    /// Rows removed across all passes.
    pub removed: u64,
}

#[storage_alias]
pub type StorageBloatCleanupMigration<T: Config> =
    StorageValue<Pallet<T>, StorageBloatCleanupProgress, OptionQuery>;

/// Returns true only after the current storage-bloat sweep has completed successfully.
///
/// Dependants must use this marker instead of cursor absence: a missing cursor can also mean
/// that the migration was never scheduled or that its progress state was removed unexpectedly.
pub fn storage_bloat_cleanup_complete<T: Config>() -> bool {
    HasMigrationRun::<T>::get(MIGRATION_NAME)
}

fn storage_prefix(pallet: &str, item: &str) -> Vec<u8> {
    [twox_128(pallet.as_bytes()), twox_128(item.as_bytes())].concat()
}

fn is_zero_value(value: &[u8]) -> bool {
    !value.is_empty() && value.iter().all(|byte| *byte == 0)
}

fn scan_item_weight<T: Config>(mode: CleanupMode) -> Weight {
    match mode {
        // next_key + clear
        CleanupMode::Clear => T::DbWeight::get().reads_writes(1, 1),
        // next_key + get + a conservatively charged clear
        CleanupMode::ClearIfZero => T::DbWeight::get().reads_writes(2, 1),
    }
}

/// Starts the cleanup without doing any unbounded work in the runtime-upgrade block.
pub fn kickoff_storage_bloat_cleanup<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(2);
    if HasMigrationRun::<T>::get(MIGRATION_NAME) || StorageBloatCleanupMigration::<T>::exists() {
        return weight;
    }

    StorageBloatCleanupMigration::<T>::put(StorageBloatCleanupProgress {
        target: 0,
        cursor: Vec::new(),
        scanned: 0,
        removed: 0,
    });
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    log::info!(
        "Migration '{}' scheduled for bounded on_idle execution",
        String::from_utf8_lossy(MIGRATION_NAME)
    );
    weight
}
/// Continues the cleanup using no more than the supplied remaining block weight.
pub fn continue_storage_bloat_cleanup<T: Config>(limit: Weight) -> Weight {
    // Cursor read plus either a cursor write, or the completion marker and cursor removal.
    let pass_overhead = T::DbWeight::get().reads_writes(1, 2);
    if !pass_overhead.all_lte(limit) {
        return Weight::zero();
    }

    let Some(mut progress) = StorageBloatCleanupMigration::<T>::get() else {
        return T::DbWeight::get().reads(1);
    };
    let work_limit = limit.saturating_sub(pass_overhead);
    let mut work_weight = Weight::zero();

    while let Some(target) = TARGETS.get(usize::from(progress.target)).copied() {
        let item_weight = scan_item_weight::<T>(target.mode);
        if !work_weight.saturating_add(item_weight).all_lte(work_limit) {
            break;
        }

        let prefix = storage_prefix(target.pallet, target.storage);
        let start = if progress.cursor.is_empty() {
            &prefix
        } else {
            &progress.cursor
        };
        let Some(next_key) = storage::next_key(start) else {
            work_weight.saturating_accrue(T::DbWeight::get().reads(1));
            progress.target = progress.target.saturating_add(1);
            progress.cursor.clear();
            continue;
        };

        if !next_key.starts_with(&prefix) {
            work_weight.saturating_accrue(T::DbWeight::get().reads(1));
            log::info!(
                "Migration '{}' finished {}::{} (scanned {}, removed {} total)",
                String::from_utf8_lossy(MIGRATION_NAME),
                target.pallet,
                target.storage,
                progress.scanned,
                progress.removed,
            );
            progress.target = progress.target.saturating_add(1);
            progress.cursor.clear();
            continue;
        }

        let should_clear = match target.mode {
            CleanupMode::Clear => true,
            CleanupMode::ClearIfZero => storage::get(&next_key)
                .as_deref()
                .is_some_and(is_zero_value),
        };
        if should_clear {
            storage::clear(&next_key);
            progress.removed = progress.removed.saturating_add(1);
        }
        progress.cursor = next_key;
        progress.scanned = progress.scanned.saturating_add(1);
        work_weight.saturating_accrue(item_weight);
    }

    if usize::from(progress.target) == TARGETS.len() {
        HasMigrationRun::<T>::insert(MIGRATION_NAME, true);
        StorageBloatCleanupMigration::<T>::kill();
        log::info!(
            "Migration '{}' completed: scanned {}, removed {} rows",
            String::from_utf8_lossy(MIGRATION_NAME),
            progress.scanned,
            progress.removed,
        );
    } else {
        StorageBloatCleanupMigration::<T>::put(progress);
    }

    pass_overhead.saturating_add(work_weight)
}
