use super::*;
use frame_support::pallet_prelude::Weight;
use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::prelude::string::String;
use sp_core::crypto::Ss58Codec;
use sp_runtime::AccountId32;
use subtensor_runtime_common::{AlphaBalance, NetUid};

pub fn decode_account_id32<T: Config>(ss58_string: &str) -> Option<T::AccountId> {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).ok()?;
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    T::AccountId::decode(&mut account_id32_slice).ok()
}

struct HotkeySwapFix {
    new_hotkey_ss58: &'static str,
}

/// Cleans up leftover `RootClaimable` state on new hotkeys produced by the buggy
/// `perform_hotkey_swap_on_one_subnet`, which unconditionally moved the entire
/// `RootClaimable` map from the old hotkey to the new hotkey during a
/// single-subnet swap.
///
/// These new hotkeys have no root stake (root swaps are and were guarded), so the
/// transferred claimable state produces no legitimate yield and only blocks future
/// flows. For each affected new hotkey we check that it truly holds no root-subnet
/// alpha and, if so, remove its `RootClaimable` entry. `RootClaimed` watermarks
/// are intentionally left in place — scanning that map does not fit in a single
/// block.
pub fn migrate_fix_root_claimed_overclaim<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_root_claimed_overclaim".to_vec();
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

    // Only run on mainnet.
    // Mainnet genesis: 0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03
    let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
    let genesis_bytes = genesis_hash.as_ref();
    let mainnet_genesis =
        hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");

    let mut cleared_hotkeys: u64 = 0;

    if genesis_bytes == mainnet_genesis {
        let fixes: &[HotkeySwapFix] = &[
            HotkeySwapFix {
                new_hotkey_ss58: "5H6BqkzjYvViiqp7rQLXjpnaEmW7U9CoKxXhQ4efMqtX1mQw",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5EnpBz2DoMTzMztFSVPSpi8jP2yfGadU6kgZgsjqnfvonMgu",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5ChzWkapDYgVxT88ZmBQS8QM63V9VWSA3eFpSipsX2xbTNZN",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5DAmVrUgpTX9xmRyZ7R3UUFNSzh7ZNY6qYxv9N4VeCq6mHHL",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5ECzcM7sixWNEeD6RbpeEHW1YcYMFejwHuvDBgQxVSjGyrMS",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5DF3nhgzpr4EZas8dXZYa4mYZBxRCU7AuiCV7Qs2JWAGA6sY",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5E4pFBKCyk2RxQqifEBu37jb5vgoj9ZrVS7iQdQy4PNr33Ge",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5DhQbRT3ZfHcVumNtAm5BbzeGHrFRHHi7nofgu76VWipnGSb",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5Gj37iVQG5hMSxU3AE89x5p3aEEfPZk6Rtmtbwepght4tbri",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5DyM1rxnDu8QSjbbh5bPV2GMK6UTPRXdUM6mNViBBut9Ma6w",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5Ci5t4vPK3eCGhFWneB58fodg3x9oS2m8seKoDApFKUqyw4e",
            },
            HotkeySwapFix {
                new_hotkey_ss58: "5Et5VQUMX7VqGyvZycjv5FBBC5FQbLGUJiRMWMnEVnMLXKm9",
            },
        ];

        for fix in fixes {
            let new_hotkey = match decode_account_id32::<T>(fix.new_hotkey_ss58) {
                Some(h) => h,
                None => {
                    log::error!(
                        "Failed to decode new hotkey {}, skipping",
                        fix.new_hotkey_ss58
                    );
                    continue;
                }
            };

            let root_stake = Pallet::<T>::get_stake_for_hotkey_on_subnet(&new_hotkey, NetUid::ROOT);
            weight.saturating_accrue(T::DbWeight::get().reads(1));

            if root_stake != AlphaBalance::zero() {
                log::info!(
                    "Skipping {} — new hotkey still has root stake",
                    fix.new_hotkey_ss58
                );
                continue;
            }

            RootClaimable::<T>::remove(&new_hotkey);
            weight.saturating_accrue(T::DbWeight::get().writes(1));
            cleared_hotkeys = cleared_hotkeys.saturating_add(1);
        }
    }

    // Mark migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration 'migrate_fix_root_claimed_overclaim' completed. \
         Cleared RootClaimable for {cleared_hotkeys} hotkeys."
    );

    weight
}
