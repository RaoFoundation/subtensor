use super::*;
use crate::staking::lock::LockState;
use frame_support::weights::Weight;
use scale_info::prelude::string::String;
use sp_core::crypto::Ss58Codec;
use sp_runtime::AccountId32;
use substrate_fixed::types::U64F64;

struct HotkeySwapLockFix {
    coldkey: Option<&'static str>,
    netuid: u16,
    old_hotkey: &'static str,
    new_hotkey: &'static str,
}

const HOTKEY_SWAP_LOCK_FIXES: &[HotkeySwapLockFix] = &[
    HotkeySwapLockFix {
        coldkey: None,
        netuid: 28,
        old_hotkey: "5Ca8L8PkbqXUtzohKtSM3i1naGQxANGLx51kJsEPNB14Admz",
        new_hotkey: "5Evgh9QTXJLxYLusVy3tcY5S6Z3GgRSNDb9AzXUchX5dco3P",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 97,
        old_hotkey: "5EU83xGi9piVeTEQsjAod1Jrog7bFKuHRVQekM4LURwXqNdJ",
        new_hotkey: "5DSEX7ww3K5i2rpCuv6cyvQ2nVn1qi7b5Ur86Vqop3muxXcC",
    },
    HotkeySwapLockFix {
        coldkey: Some("5C53wCYowihKwAxwTKd7ZA8hyzBkZJ1Qqa3Ry7v75Ed6eRNP"),
        netuid: 97,
        old_hotkey: "5F1dKAbbJNtf4Yce8ostaU5e1iPfrL6q8cjqH1KUGbBzmees",
        new_hotkey: "5CtNXpjaK79SX9QC1GqRbS3C4KNETT7jh6GgZDrmCxvYrAdJ",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 97,
        old_hotkey: "5DAGsDentUAs6Uh9SYJ2uLEQvYRWu1Euqai97AHF3A7RiGoT",
        new_hotkey: "5GsqcuatjJtgSwJuaZWpPV8QWcQ4aHcPkF8DG7oqMNJLoN93",
    },
    HotkeySwapLockFix {
        coldkey: Some("5C53wCYowihKwAxwTKd7ZA8hyzBkZJ1Qqa3Ry7v75Ed6eRNP"),
        netuid: 97,
        old_hotkey: "5CtNXpjaK79SX9QC1GqRbS3C4KNETT7jh6GgZDrmCxvYrAdJ",
        new_hotkey: "5DMN2AnnbUqbnSvbHkXHF7A8JUKBT8vxJJhKkDKG1GCwGDwf",
    },
    HotkeySwapLockFix {
        coldkey: Some("5D2n9CKP4KQ1FMf1ybm2psFsQSMKkCiCFMRchcvo3EUECFro"),
        netuid: 120,
        old_hotkey: "5GCN5Bo2djDGQ6aqVjgdfMzWbLuqhcN5pyNmrbkkJ4n7jZpQ",
        new_hotkey: "5GRaijFsfTR723LeofVrjrq8kNAdyucmT9TPPGdiyrxckwGg",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 120,
        old_hotkey: "5EU83xGi9piVeTEQsjAod1Jrog7bFKuHRVQekM4LURwXqNdJ",
        new_hotkey: "5DAGsDentUAs6Uh9SYJ2uLEQvYRWu1Euqai97AHF3A7RiGoT",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 97,
        old_hotkey: "5H3Kuy7L7DBSy7BS2c9EBayJYGkHV1pzWtnJm3iXvThT4VUJ",
        new_hotkey: "5CSiRF3sMKt1c3MT4KsRLBWENGkymVE7wA2zUDPsYy6JtpGE",
    },
    HotkeySwapLockFix {
        coldkey: Some("5C53wCYowihKwAxwTKd7ZA8hyzBkZJ1Qqa3Ry7v75Ed6eRNP"),
        netuid: 97,
        old_hotkey: "5Cm7DPowNeA2b8b2ET4EkyqgviZUnsTUqQuqAnGp1SfXuPSw",
        new_hotkey: "5GbMdbCdt4TJ94JUbf22uWGRvf17u99DdKpHuJGyMiexuCKx",
    },
    HotkeySwapLockFix {
        coldkey: Some("5D2n9CKP4KQ1FMf1ybm2psFsQSMKkCiCFMRchcvo3EUECFro"),
        netuid: 120,
        old_hotkey: "5GRaijFsfTR723LeofVrjrq8kNAdyucmT9TPPGdiyrxckwGg",
        new_hotkey: "5CAkU49aHNYcDVLKKYSHnBuuymWr1A7aoAiAhK8FZpNYF6YH",
    },
    HotkeySwapLockFix {
        coldkey: Some("5DywxdtESjskgPZrDXL86qV44SpPgJuqs9X6noyJJwX9PaSD"),
        netuid: 128,
        old_hotkey: "5GRViDgqddpH3qB9A6nqPgMepgum51ZUZ199ksXQuCFsn128",
        new_hotkey: "5Gq2gs4ft5dhhjbHabvVbAhjMCV2RgKmVJKAFCUWiirbRT21",
    },
    HotkeySwapLockFix {
        coldkey: Some("5D2n9CKP4KQ1FMf1ybm2psFsQSMKkCiCFMRchcvo3EUECFro"),
        netuid: 120,
        old_hotkey: "5CAkU49aHNYcDVLKKYSHnBuuymWr1A7aoAiAhK8FZpNYF6YH",
        new_hotkey: "5HbgNXyw4mCMQWLL6Hb7inA2qQ81A8pqw1GFxpcshpKu11Aj",
    },
    HotkeySwapLockFix {
        coldkey: Some("5C53wCYowihKwAxwTKd7ZA8hyzBkZJ1Qqa3Ry7v75Ed6eRNP"),
        netuid: 97,
        old_hotkey: "5CfXcxCex4Up1S2SjP4MhBPM55qioPd8dCt2SEMC94m4M5Md",
        new_hotkey: "5EWk5uun4rdLHfst1DXiU6e4QqTXSNpdkCtGVBGjEkDoorfN",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 97,
        old_hotkey: "5HVyG7q3AiMLvG4GvkXTCfarerA3GnJ6a3r8pSVVeUiSLTng",
        new_hotkey: "5GWJ5cdmEAiCL8V9sopDvntQjKtw5ciHy8urPh9AMLkpmtEw",
    },
    HotkeySwapLockFix {
        coldkey: Some("5C8SMSqb1i3tFao2vwdAnFWM6KA38y5UFCwBCLVr5a48tXtz"),
        netuid: 97,
        old_hotkey: "5GCFXhD1E7aY1Eq9hDWe24fXwRe4gqJ4nxw7XcV19SwTgtoq",
        new_hotkey: "5FCXQcqNd8W5CJuTgNvPjR2R82N5TMJ66sPmWPhEDs3GkgZQ",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EZWeiJunm2PCdsyUCv6UvckY5daLKGrxzRu1K9QHBAYiVhm"),
        netuid: 120,
        old_hotkey: "5ECiTKuujAHqf29cUDvsiEPwtAC6Yg3cT8aHJ4riAp41p1bS",
        new_hotkey: "5CMFnjWR72kCMi9rChg3DZAH4MidSLHNfRaKCwyqaTyyRsev",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 120,
        old_hotkey: "5DAGsDentUAs6Uh9SYJ2uLEQvYRWu1Euqai97AHF3A7RiGoT",
        new_hotkey: "5HVyG7q3AiMLvG4GvkXTCfarerA3GnJ6a3r8pSVVeUiSLTng",
    },
    HotkeySwapLockFix {
        coldkey: Some("5C53wCYowihKwAxwTKd7ZA8hyzBkZJ1Qqa3Ry7v75Ed6eRNP"),
        netuid: 97,
        old_hotkey: "5GbMdbCdt4TJ94JUbf22uWGRvf17u99DdKpHuJGyMiexuCKx",
        new_hotkey: "5CURjyKkCiSnaSPwMBUJXLC7mkadbPPkKQamyFhdsfb5DnSp",
    },
    HotkeySwapLockFix {
        coldkey: Some("5EWUPMenvyvHdEGUHfUhSTeTDJDLzLkKZq74LFLRWtzcqZiS"),
        netuid: 97,
        old_hotkey: "5CSiRF3sMKt1c3MT4KsRLBWENGkymVE7wA2zUDPsYy6JtpGE",
        new_hotkey: "5EsnHJK89FgF55EYwXtqhUwLu3c14xakyQ8PWoomcFwpxk5e",
    },
];

fn decode_account_id32<T: Config>(ss58_string: &str) -> Option<T::AccountId> {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).ok()?;
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    T::AccountId::decode(&mut account_id32_slice).ok()
}

fn is_non_zero_lock(lock: &LockState) -> bool {
    !lock.locked_mass.is_zero() || lock.conviction > U64F64::saturating_from_num(0)
}

fn add_lock_state(mut lhs: LockState, rhs: &LockState) -> LockState {
    lhs.locked_mass = lhs.locked_mass.saturating_add(rhs.locked_mass);
    lhs.conviction = lhs.conviction.saturating_add(rhs.conviction);
    lhs.last_update = lhs.last_update.max(rhs.last_update);
    lhs
}

fn subtract_lock_state(mut lhs: LockState, rhs: &LockState) -> LockState {
    lhs.locked_mass = lhs.locked_mass.saturating_sub(rhs.locked_mass);
    lhs.conviction = lhs.conviction.saturating_sub(rhs.conviction);
    lhs
}

fn mutate_aggregate<T: Config, F>(
    coldkey: &T::AccountId,
    netuid: NetUid,
    hotkey: &T::AccountId,
    mutate: F,
) where
    F: FnOnce(LockState) -> LockState + Clone,
{
    let perpetual = DecayingLock::<T>::get(coldkey, netuid) == Some(false);
    let owner = SubnetOwnerHotkey::<T>::get(netuid) == *hotkey;

    match (owner, perpetual) {
        (true, true) => OwnerLock::<T>::mutate(netuid, |maybe_lock| {
            if let Some(lock) = maybe_lock.take() {
                let updated = mutate(lock);
                if is_non_zero_lock(&updated) {
                    *maybe_lock = Some(updated);
                }
            }
        }),
        (true, false) => DecayingOwnerLock::<T>::mutate(netuid, |maybe_lock| {
            if let Some(lock) = maybe_lock.take() {
                let updated = mutate(lock);
                if is_non_zero_lock(&updated) {
                    *maybe_lock = Some(updated);
                }
            }
        }),
        (false, true) => HotkeyLock::<T>::mutate(netuid, hotkey, |maybe_lock| {
            if let Some(lock) = maybe_lock.take() {
                let updated = mutate(lock);
                if is_non_zero_lock(&updated) {
                    *maybe_lock = Some(updated);
                }
            }
        }),
        (false, false) => DecayingHotkeyLock::<T>::mutate(netuid, hotkey, |maybe_lock| {
            if let Some(lock) = maybe_lock.take() {
                let updated = mutate(lock);
                if is_non_zero_lock(&updated) {
                    *maybe_lock = Some(updated);
                }
            }
        }),
    }
}

fn add_to_aggregate<T: Config>(
    coldkey: &T::AccountId,
    netuid: NetUid,
    hotkey: &T::AccountId,
    added: &LockState,
) {
    let perpetual = DecayingLock::<T>::get(coldkey, netuid) == Some(false);
    let owner = SubnetOwnerHotkey::<T>::get(netuid) == *hotkey;

    match (owner, perpetual) {
        (true, true) => OwnerLock::<T>::mutate(netuid, |maybe_lock| {
            *maybe_lock = Some(match maybe_lock.take() {
                Some(lock) => add_lock_state(lock, added),
                None => added.clone(),
            });
        }),
        (true, false) => DecayingOwnerLock::<T>::mutate(netuid, |maybe_lock| {
            *maybe_lock = Some(match maybe_lock.take() {
                Some(lock) => add_lock_state(lock, added),
                None => added.clone(),
            });
        }),
        (false, true) => HotkeyLock::<T>::mutate(netuid, hotkey, |maybe_lock| {
            *maybe_lock = Some(match maybe_lock.take() {
                Some(lock) => add_lock_state(lock, added),
                None => added.clone(),
            });
        }),
        (false, false) => DecayingHotkeyLock::<T>::mutate(netuid, hotkey, |maybe_lock| {
            *maybe_lock = Some(match maybe_lock.take() {
                Some(lock) => add_lock_state(lock, added),
                None => added.clone(),
            });
        }),
    }
}

/// Fixes lock state left behind by subnet-scoped hotkey swaps.
///
/// If a destination lock already exists for the same coldkey, the old lock is
/// discarded instead of merged.
pub fn migrate_fix_subnet_hotkey_lock_swaps<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_subnet_hotkey_lock_swaps".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let mut moved_locks = 0u64;
    let mut discarded_locks = 0u64;
    let mut missing_locks = 0u64;

    for fix in HOTKEY_SWAP_LOCK_FIXES {
        let Some(old_hotkey) = decode_account_id32::<T>(fix.old_hotkey) else {
            log::error!("Failed to decode old hotkey: {}", fix.old_hotkey);
            continue;
        };
        let Some(new_hotkey) = decode_account_id32::<T>(fix.new_hotkey) else {
            log::error!("Failed to decode new hotkey: {}", fix.new_hotkey);
            continue;
        };
        let netuid = NetUid::from(fix.netuid);

        let locks_to_fix: Vec<(T::AccountId, LockState)> = if let Some(coldkey) = fix.coldkey {
            let Some(coldkey) = decode_account_id32::<T>(coldkey) else {
                log::error!("Failed to decode coldkey: {}", coldkey);
                continue;
            };
            Lock::<T>::take((coldkey.clone(), netuid, old_hotkey.clone()))
                .map(|lock| vec![(coldkey, lock)])
                .unwrap_or_default()
        } else {
            let locks: Vec<(T::AccountId, LockState)> = Lock::<T>::iter()
                .filter_map(|((coldkey, lock_netuid, hotkey), lock)| {
                    (lock_netuid == netuid && hotkey == old_hotkey).then_some((coldkey, lock))
                })
                .collect();
            for (coldkey, _) in &locks {
                Lock::<T>::remove((coldkey.clone(), netuid, old_hotkey.clone()));
            }
            locks
        };
        let locks_to_fix_count = locks_to_fix.len() as u64;
        weight = weight.saturating_add(
            T::DbWeight::get()
                .reads_writes(locks_to_fix_count.saturating_add(1), locks_to_fix_count),
        );

        if locks_to_fix.is_empty() {
            missing_locks = missing_locks.saturating_add(1);
            continue;
        }

        for (coldkey, lock) in locks_to_fix {
            let destination_conflict =
                Lock::<T>::contains_key((coldkey.clone(), netuid, new_hotkey.clone()));
            weight = weight.saturating_add(T::DbWeight::get().reads(1));

            let new_hotkey_is_owner = SubnetOwnerHotkey::<T>::get(netuid) == new_hotkey;
            if !new_hotkey_is_owner || destination_conflict {
                let removed = lock.clone();
                mutate_aggregate::<T, _>(&coldkey, netuid, &old_hotkey, |aggregate| {
                    subtract_lock_state(aggregate, &removed)
                });
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 1));
            }

            if destination_conflict {
                discarded_locks = discarded_locks.saturating_add(1);
                continue;
            }

            Lock::<T>::insert((coldkey.clone(), netuid, new_hotkey.clone()), lock.clone());
            weight = weight.saturating_add(T::DbWeight::get().writes(1));

            if !new_hotkey_is_owner {
                add_to_aggregate::<T>(&coldkey, netuid, &new_hotkey, &lock);
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 1));
            }

            moved_locks = moved_locks.saturating_add(1);
        }
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully. Moved locks: {:?}, discarded locks: {:?}, missing locks: {:?}.",
        String::from_utf8_lossy(&migration_name),
        moved_locks,
        discarded_locks,
        missing_locks,
    );

    weight
}
