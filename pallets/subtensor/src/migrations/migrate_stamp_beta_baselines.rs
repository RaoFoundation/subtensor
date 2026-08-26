use super::*;
use crate::migrations::beta_baseline_table::BETA_BASELINES;
use codec::Decode;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use substrate_fixed::types::{I96F32, U64F64};

/// Seeds the on-chain [`BetaBaseline`] map from the frozen SDK baseline table
/// (`beta_baseline_table.rs`), so funds that predate on-chain stamping keep the exact
/// historical splice the SDK has been displaying: their display divisor, first-sighting
/// `BasketRate`, total-return splice level, and first-sighting block.
///
/// Only funds that are live on this chain (outstanding `BasketShares`) and not already
/// stamped are seeded; everything else in the table is skipped. Funds live on-chain but
/// missing from the table (born after the table freeze) are stamped by
/// `stamp_beta_baseline_if_new` at their next share mint, exactly like a newborn fund.
pub fn migrate_stamp_beta_baselines<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"migrate_stamp_beta_baselines".to_vec();
    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(&mig_name)
        );
        return total_weight;
    }
    log::info!("Running migration '{}'", String::from_utf8_lossy(&mig_name));

    let mut stamped: u64 = 0;
    for (pubkey, divisor_bits, rate0_bits, tr_splice_bits, first_block) in BETA_BASELINES {
        // Shares plus existing-baseline reads per table row.
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(2));

        let Ok(hotkey) = T::AccountId::decode(&mut &pubkey[..]) else {
            continue;
        };
        // Not a live fund on this chain (e.g. testnets, or the fund fully drained).
        if BasketShares::<T>::get(&hotkey) == 0 {
            continue;
        }
        if BetaBaseline::<T>::contains_key(&hotkey) {
            continue;
        }

        BetaBaseline::<T>::insert(
            &hotkey,
            BetaBaselineOf {
                price_divisor: U64F64::from_bits(*divisor_bits),
                rate0: I96F32::from_bits(*rate0_bits),
                tr_splice: U64F64::from_bits(*tr_splice_bits),
                first_block: *first_block,
            },
        );
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        stamped = stamped.saturating_add(1);
    }

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed: stamped {stamped} fund baseline(s)",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}
