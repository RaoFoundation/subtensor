use super::*;
use crate::migrations::beta_baseline_table::{
    BETA_BASELINES, BETA_INDEX_LEVEL_BITS, BETA_TR_INDEX_LEVEL_BITS,
};
use codec::Decode;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use substrate_fixed::types::{I96F32, U64F64};

/// `HasMigrationRun` key for this migration.
pub const MIGRATION_NAME: &[u8] = b"migrate_stamp_beta_baselines";

/// Seeds the on-chain [`BetaBaseline`] map from the frozen SDK baseline table
/// (`beta_baseline_table.rs`), so funds that predate on-chain stamping keep the exact
/// historical splice the SDK has been displaying: their display divisor, first-sighting
/// `BasketRate`, total-return splice level, and first-sighting block.
///
/// The seeded `tr_splice` is not the table value verbatim: the live stake series
/// (`stake_price = tr_splice × BasketTwr`) only compounds dividends from this upgrade
/// onward — the TWR storage starts at its 1.0 default here — while the table's splice
/// was sampled at the fund's *first sighting*. The staker history between the two
/// exists on-chain only as the fund's rate delta, so the migration folds it in:
/// `tr_splice = table_splice × (1 + (BasketRate - rate0) × raw price)`, with the raw
/// price marked at the migration block's realizable quotes (the same mark every other
/// permanent artifact uses — see `staking/beta_pricing.rs`, "Two marks"). The fund's
/// stake price is therefore continuous at the upgrade block and compounds via the TWR
/// from then on (see [`seeded_baseline`]).
///
/// Only funds that are live on this chain (outstanding `BasketShares`) and not already
/// stamped are seeded; everything else in the table is skipped. Funds live on-chain but
/// missing from the table (born after the table freeze) are stamped by
/// `stamp_beta_baseline_if_new` at their next share mint, exactly like a newborn fund.
///
/// When any fund is seeded, the initial [`BetaIndexSnapshot`] is also published from the
/// table's frozen index levels, so the on-chain **chained** index continues the SDK's
/// historical series (see `staking/beta_pricing.rs`) instead of restarting at 1.0 — the
/// baselines were stamped against that series, so evaluating them against any other
/// starting level would jump every fund's `vs_index` at the upgrade. On chains where the
/// table seeds nothing (testnets, fresh chains) the snapshot is left alone and the index
/// starts at 1.0, which is the correct epoch convention there.
pub fn migrate_stamp_beta_baselines<T: Config>() -> Weight {
    let mig_name: Vec<u8> = MIGRATION_NAME.to_vec();
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

        let (baseline, rows) = seeded_baseline::<T>(
            &hotkey,
            *divisor_bits,
            *rate0_bits,
            *tr_splice_bits,
            *first_block,
        );
        // The rate read plus the NAV scan's holding rows (each a storage read and a
        // realizable pool quote).
        total_weight = total_weight
            .saturating_add(T::DbWeight::get().reads(rows.saturating_mul(2).saturating_add(2)));
        BetaBaseline::<T>::insert(&hotkey, baseline);
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        stamped = stamped.saturating_add(1);
    }

    total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
    if stamped > 0 && BetaIndexSnapshot::<T>::get().is_none() {
        BetaIndexSnapshot::<T>::put(BetaIndexSnapshotOf {
            bag_level: U64F64::from_bits(BETA_INDEX_LEVEL_BITS),
            stake_level: U64F64::from_bits(BETA_TR_INDEX_LEVEL_BITS),
            block: Pallet::<T>::get_current_block_as_u64(),
        });
        total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
    }

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed: stamped {stamped} fund baseline(s)",
        String::from_utf8_lossy(&mig_name)
    );
    total_weight
}

/// The exact [`BetaBaselineOf`] this migration seeds for one live table row, plus the
/// holding rows scanned to compute it (for weight accounting). Everything but
/// `tr_splice` is the frozen table row verbatim; the splice folds in the fund's
/// pre-upgrade staker entitlement — `1 + (BasketRate - rate0) × raw price` at the
/// migration block's realizable quotes — so the stake series is continuous at the
/// upgrade (see the module docs). A fund with no priceable holdings (zero NAV) folds
/// by exactly 1: with nothing to value the entitlement against, the table splice is
/// kept unchanged.
fn seeded_baseline<T: Config>(
    hotkey: &T::AccountId,
    divisor_bits: u128,
    rate0_bits: i128,
    tr_splice_bits: u128,
    first_block: u64,
) -> (BetaBaselineOf, u64) {
    let rate0 = I96F32::from_bits(rate0_bits);
    let shares = BasketShares::<T>::get(hotkey);
    let (nav, rows) = Pallet::<T>::basket_realizable_nav_rao(hotkey);
    let raw = U64F64::saturating_from_num(nav)
        .checked_div(U64F64::saturating_from_num(shares))
        .unwrap_or_default();
    let pre_upgrade_yield =
        Pallet::<T>::basket_pending_yield(BasketRate::<T>::get(hotkey), rate0, raw);
    let tr_splice = U64F64::from_bits(tr_splice_bits)
        .saturating_mul(U64F64::saturating_from_num(1).saturating_add(pre_upgrade_yield));
    (
        BetaBaselineOf {
            price_divisor: U64F64::from_bits(divisor_bits),
            rate0,
            tr_splice,
            first_block,
        },
        rows,
    )
}

/// The table rows this migration must seed on the current chain: decodable hotkeys that
/// are live (outstanding `BasketShares`) and not already stamped, paired with the exact
/// [`BetaBaselineOf`] the migration derives for them (the frozen row with the
/// pre-upgrade entitlement folded into `tr_splice` — see [`seeded_baseline`]). Empty
/// once the migration has run.
#[cfg(feature = "try-runtime")]
fn expected_seeds<T: Config>() -> Vec<(T::AccountId, BetaBaselineOf)> {
    if HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()) {
        return Vec::new();
    }
    BETA_BASELINES
        .iter()
        .filter_map(
            |(pubkey, divisor_bits, rate0_bits, tr_splice_bits, first_block)| {
                let hotkey = T::AccountId::decode(&mut &pubkey[..]).ok()?;
                if BasketShares::<T>::get(&hotkey) == 0 || BetaBaseline::<T>::contains_key(&hotkey)
                {
                    return None;
                }
                let (baseline, _) = seeded_baseline::<T>(
                    &hotkey,
                    *divisor_bits,
                    *rate0_bits,
                    *tr_splice_bits,
                    *first_block,
                );
                Some((hotkey, baseline))
            },
        )
        .collect()
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper with try-runtime
/// pre/post-upgrade invariant validation, registered in the runtime `Migrations` tuple so
/// the try-runtime CI jobs verify the seed against real mainnet/testnet/devnet state.
///
/// Validated invariants: every live, unstamped fund in the frozen table gets exactly the
/// baseline [`seeded_baseline`] derives from the table (the frozen row with the
/// pre-upgrade entitlement folded into `tr_splice`); every baseline that existed before
/// the upgrade is untouched; when
/// the seed stamps anything on a chain with no published index snapshot, the initial
/// snapshot equals the table's frozen index levels (the SDK-series splice); the
/// `HasMigrationRun` flag ends set; and (via try-runtime's double-execution check plus
/// that flag) the migration is idempotent.
pub mod stamp_beta_baselines {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use codec::Encode;
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;

    /// State carried from `pre_upgrade` to `post_upgrade`: the rows the migration must
    /// seed, every baseline that already existed (and must survive verbatim), and
    /// whether an index snapshot was already published (in which case the migration
    /// must not touch it).
    #[cfg(feature = "try-runtime")]
    type PreUpgradeState<T> = (
        Vec<(<T as frame_system::Config>::AccountId, BetaBaselineOf)>,
        Vec<(<T as frame_system::Config>::AccountId, BetaBaselineOf)>,
        bool,
    );

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_stamp_beta_baselines::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            let expected = expected_seeds::<T>();
            let preexisting: Vec<(T::AccountId, BetaBaselineOf)> =
                BetaBaseline::<T>::iter().collect();
            let had_snapshot = BetaIndexSnapshot::<T>::get().is_some();
            Ok((expected, preexisting, had_snapshot).encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let (expected, preexisting, had_snapshot): PreUpgradeState<T> =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;

            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "migrate_stamp_beta_baselines must mark itself as run"
            );
            if !expected.is_empty() && !had_snapshot {
                let snapshot = BetaIndexSnapshot::<T>::get()
                    .ok_or("a seeding run must publish the initial index snapshot")?;
                ensure!(
                    snapshot.bag_level == U64F64::from_bits(BETA_INDEX_LEVEL_BITS)
                        && snapshot.stake_level == U64F64::from_bits(BETA_TR_INDEX_LEVEL_BITS),
                    "the initial index snapshot must splice onto the frozen SDK series"
                );
            }
            for (hotkey, want) in expected {
                ensure!(
                    BetaBaseline::<T>::get(&hotkey) == Some(want),
                    "every live, unstamped table fund must be seeded with the frozen baseline"
                );
            }
            for (hotkey, before) in preexisting {
                ensure!(
                    BetaBaseline::<T>::get(&hotkey) == Some(before),
                    "pre-existing baselines must never be rewritten"
                );
            }
            Ok(())
        }
    }
}
