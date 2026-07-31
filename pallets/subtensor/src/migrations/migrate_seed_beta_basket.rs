use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::pallet_prelude::{Blake2_128Concat, Identity, OptionQuery, ValueQuery};
use frame_support::storage_alias;
use frame_support::weights::Weight;
use scale_info::TypeInfo;
use scale_info::prelude::string::String;
use sp_std::collections::btree_map::BTreeMap;
use sp_std::vec::Vec;
use substrate_fixed::types::{I96F32, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid};
use subtensor_swap_interface::SwapHandler;

/// Absolute API ceiling on how many `RootClaimable` hotkeys may be converted in one pass.
/// Normal throughput is lower and is derived from the remaining `on_idle` weight.
pub const MAX_SEED_BETA_BASKET_HOTKEYS_PER_PASS: u32 = u32::MAX;

/// Absolute API ceiling on orphaned `BasketPrincipal` entries cleared in one pass.
/// `StorageDoubleMap::clear` accepts a `u32` limit; the adaptive weight meter remains the
/// effective per-block bound.
pub const MAX_SEED_BETA_BASKET_PRINCIPAL_CLEAR_PER_PASS: u32 = u32::MAX;

/// Absolute API ceiling on `RootClaimed` rows drained while converting hotkeys. A single subnet
/// slot's `(netuid, hotkey)` prefix can hold one row per historical claimant coldkey, which is
/// unbounded in principle. Normal throughput is derived from the remaining `on_idle` weight.
pub const MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS: u32 = u32::MAX;

const MIGRATION_NAME: &[u8] = b"migrate_seed_beta_basket_v2";

pub mod deprecated {
    use super::*;

    /// Per-slot outstanding basket principal written by the superseded v1 seed migration
    /// (`migrate_seed_beta_basket`) and the intermediate per-subnet-slot runtime. No longer
    /// declared in the pallet; v2 clears any orphaned entries.
    #[storage_alias]
    pub type BasketPrincipal<T: Config> = StorageDoubleMap<
        Pallet<T>,
        Blake2_128Concat,
        AccountIdOf<T>,
        Identity,
        NetUid,
        AlphaBalance,
        ValueQuery,
    >;
}

/// Resumable state for a hotkey whose conversion was paused mid-way because the per-pass
/// `RootClaimed` drain budget ([`MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS`]) ran out before
/// every subnet slot finished draining.
///
/// `RootClaimable[hotkey]` is left untouched in storage for as long as this state exists; it is
/// only `remove`d once every slot has been fully processed, so an interrupted conversion never
/// loses track of a hotkey's still-unconverted slots.
///
/// Valuation inputs (`total_root`, and the in-progress slot's `price`) are snapshotted here and
/// reused on every resume so already-written `BasketClaimed` rows stay consistent with later
/// drains / `fund_rate` / `fund_shares` if stake or moving prices change between blocks.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct HotkeyConvertState {
    /// SCALE-encoded `T::AccountId` of the hotkey being converted.
    pub hotkey: Vec<u8>,
    /// Highest `NetUid` (in `RootClaimable[hotkey]`'s `BTreeMap` order) fully converted so far.
    /// `None` means no slot has completed yet. Resuming re-reads `RootClaimable[hotkey]`
    /// (still present in storage) and skips every entry up to and including this one.
    pub done_through: Option<NetUid>,
    /// Claimant-base root stake snapshotted at the start of this hotkey's conversion.
    pub total_root: I96F32,
    /// The subnet slot whose `RootClaimed` drain was paused mid-way, if any:
    /// `(netuid, rate, price, claimed_cursor, claimed_sum_so_far)`. `price` is the fixed
    /// conversion price for this slot; `claimed_cursor` resumes `RootClaimed::iter_prefix_from`;
    /// `claimed_sum_so_far` is the alpha sum of rows already drained for this slot.
    pub partial: Option<(NetUid, I96F32, U96F32, Vec<u8>, I96F32)>,
    /// Unified rate accumulated from fully-completed slots.
    pub fund_rate: I96F32,
    /// Unified fund shares accumulated from fully-completed slots.
    pub fund_shares: u64,
    /// Subnet slots that have contributed outstanding shares so far, for the seeded-slots
    /// log/weight accounting.
    pub seeded_slots: u64,
}

/// Persistent cursor for the multi-block `migrate_seed_beta_basket_v2` migration.
///
/// Present only while the migration is in progress. Cleared when `HasMigrationRun` is set.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub enum SeedBetaBasketV2Progress {
    /// Convert legacy per-hotkey claim state. `after` is the hashed key of the last completed
    /// hotkey (`None` = start from the beginning). `iter_keys_from` skips that key. `hotkey` is
    /// `Some` when the hotkey after `after` was only partially converted because the per-pass
    /// `RootClaimed` drain budget ran out mid-way.
    Convert {
        after: Option<Vec<u8>>,
        hotkey: Option<HotkeyConvertState>,
    },
    /// Clear the provisional per-slot share supply written by `Convert`. Legacy overclaims can
    /// make `Σ_s max(gross_s - claimed_s, 0)` differ from the unified model's
    /// `Σ_c max(BasketRate * root_stake_c - BasketClaimed_c, 0)`, so the final supply must be
    /// rebuilt from claimant positions rather than retaining the per-slot total.
    ClearShares { cursor: Option<Vec<u8>> },
    /// Rebuild `BasketShares` from uncapped runtime owed positions. `StakingHotkeys` is the
    /// authoritative claimant set used by the claim API; unlike the auxiliary dense coldkey
    /// index, it is complete on legacy mainnet state. `after` resumes the outer storage map and
    /// `coldkey`/`hotkey_index` resume within one coldkey's vector.
    ReconcileClaimants {
        after: Option<Vec<u8>>,
        coldkey: Option<Vec<u8>>,
        hotkey_index: u32,
    },
    /// Bounded clear of legacy `RootClaimed` rows that had no matching
    /// `RootClaimable[hotkey][netuid]` slot. Such watermarks are inert because there is no
    /// corresponding accrual rate to convert, but they must still be removed before the
    /// migration can declare the legacy maps fully drained.
    ClearClaimed { cursor: Option<Vec<u8>> },
    /// Bounded clear of orphaned v1 `BasketPrincipal` entries.
    ClearPrincipal { cursor: Option<Vec<u8>> },
}

#[storage_alias]
pub type SeedBetaBasketV2Migration<T: Config> =
    StorageValue<Pallet<T>, SeedBetaBasketV2Progress, OptionQuery>;

/// Seeds the unified beta-basket fund from pre-existing per-subnet claim state.
///
/// Legacy model: a validator's root dividends accrued as a per-subnet *rate*
/// (`RootClaimable[hotkey][netuid]`, alpha-per-root-stake) with per-subnet claimed watermarks
/// (`RootClaimed[(netuid, hotkey, coldkey)]`), backed by unattributed outstanding alpha in
/// `SubnetAlphaOut`. The beta basket instead is a single *fund* per validator: escrow stake
/// positions `(hotkey, escrow, netuid)` are its holdings, `BasketShares` its outstanding
/// TAO-denominated shares `P`, `BasketRate` the single shares-per-root-stake accumulator, and
/// `BasketClaimed[(hotkey, coldkey)]` the per-staker watermark.
///
/// Conversion fixes each subnet's moving price `p_s` at the migration block (spot fallback for
/// cold EMAs; 1:1 for the root slot) and re-denominates every legacy alpha-unit quantity into
/// TAO-valued fund shares:
///
/// * holdings: the still-outstanding legacy alpha `remaining_s = rate_s * total_root - Σ claimed`
///   is attributed to the validator under the escrow coldkey on subnet `s`;
/// * `BasketRate[hot]   = Σ_s rate_s * p_s`
/// * provisional `BasketShares[hot] = Σ_s remaining_s * p_s`
/// * `BasketClaimed[hot, ck] = Σ_s claimed_s(ck) * p_s`
///
/// Legacy state can contain per-subnet overclaims. Flooring each subnet's remaining backing and
/// then aggregating claims into one signed watermark does not commute: the provisional supply can
/// differ from `Σ_c max(BasketRate * root_stake_c - BasketClaimed_c, 0)`. After conversion, a
/// bounded reconciliation therefore clears the provisional supply and rebuilds `BasketShares`
/// from every uncapped runtime claimant position. This guarantees every share is owned exactly
/// once and prevents claim-order-dependent loss. On inconsistent legacy funds the initial
/// `NAV / BasketShares` may differ from one; that share price applies the backing surplus or
/// shortfall pro rata instead of letting early claimants drain later claimants' value.
///
/// The drained legacy maps are cleared so no per-subnet claim state survives.
///
/// ## Chains that already ran the superseded v1 seed migration
///
/// This is **v2** under a fresh `HasMigrationRun` key: the v1 migration
/// (`"migrate_seed_beta_basket"`) seeded the abandoned per-slot `BasketPrincipal` model on dev
/// and test chains, consuming the old key. Reusing the old name would silently skip this
/// migration there and strand every basket. v2 therefore also tolerates v1 state:
///
/// * escrow positions may already exist (v1 staked `remaining` at its run block, and the
///   intermediate runtime compounded/claimed against them). The escrow is only topped up when
///   it holds *less* than the recomputed `remaining`; when it holds more (compounding), the
///   surplus stays and simply carries the old slot's `E/P` multiplier into the fund's `N/P`.
/// * legacy `RootClaimable` may contain root-slot (netuid 0) entries created by the
///   intermediate runtime. These convert at price 1, but never mint a top-up (root has no pool
///   to attribute from); their share contribution is capped at the escrow's actual root stake
///   so shares are never unbacked.
/// * orphaned `BasketPrincipal` entries are cleared.
///
/// ## Multi-block / resumable
///
/// `on_runtime_upgrade` only **kicks off** the seed via [`kickoff_seed_beta_basket_v2`]
/// (writes the cursor, no conversion). That keeps the upgrade hook idempotent for
/// try-runtime's double-execution check. All conversion runs from `on_idle` through
/// [`migrate_seed_beta_basket_v2`].
///
/// Work is chunked across blocks at two levels. Each pass converts at most
/// [`MAX_SEED_BETA_BASKET_HOTKEYS_PER_PASS`] hotkeys, then clears at most
/// [`MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS`] orphaned `RootClaimed` rows and
/// [`MAX_SEED_BETA_BASKET_PRINCIPAL_CLEAR_PER_PASS`] orphaned principal rows. Within the convert
/// phase, the same claimed-row limit bounds how many matching `RootClaimed` rows are drained: a
/// hotkey with many claimant coldkeys on one subnet slot is converted across several passes
/// rather than draining its whole (unbounded) prefix at once.
/// While a hotkey is only partially converted, `RootClaimable[hotkey]` is left untouched in
/// storage and the in-flight state (completed slots, the in-progress slot's drain cursor, and
/// accumulators) is persisted in [`SeedBetaBasketV2Migration`]'s `Convert::hotkey`. Progress is
/// continued from `on_idle` until finished. `HasMigrationRun` is set only when every phase
/// completes.
pub fn migrate_seed_beta_basket_v2<T: Config>() -> Weight {
    migrate_seed_beta_basket_v2_with_limit::<T>(Weight::MAX)
}

/// Continue the seed using limits derived from the weight still available to `on_idle`.
///
/// Fixed pass overhead is reserved first. Each work type gets a ceiling derived from the full
/// remaining budget, while the conversion loop uses one shared meter to stop before their
/// combined reported weight reaches `limit`. This lets hotkeys consume capacity unused by
/// claimed rows (and vice versa) without allowing their independent ceilings to add up to an
/// overweight pass. Emergency item ceilings remain as a final guard against a bad weight
/// configuration.
///
/// Progress is deliberately fail-open: every active phase receives a minimum item budget of
/// one even if `limit` is zero or one iteration is individually overweight. In that exceptional
/// case the returned weight may exceed `limit`, but the cursor advances instead of retrying the
/// same oversized item forever.
pub fn migrate_seed_beta_basket_v2_with_limit<T: Config>(limit: Weight) -> Weight {
    let overhead = adaptive_pass_overhead::<T>();
    let work_budget = limit.saturating_sub(overhead);

    let hotkeys = affordable_items(work_budget, hotkey_attempt_weight::<T>())
        .max(1)
        .min(u64::from(MAX_SEED_BETA_BASKET_HOTKEYS_PER_PASS)) as u32;
    let claimed = affordable_items(work_budget, claimed_row_base_weight::<T>())
        .max(1)
        .min(u64::from(MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS)) as u32;
    let principal = affordable_items(work_budget, principal_clear_item_weight::<T>())
        .max(1)
        .min(u64::from(MAX_SEED_BETA_BASKET_PRINCIPAL_CLEAR_PER_PASS)) as u32;

    migrate_seed_beta_basket_v2_inner::<T>(hotkeys, principal, claimed, Some(limit))
}

fn adaptive_pass_overhead<T: Config>() -> Weight {
    // HasMigrationRun + cursor + escrow reads, plus enough writes to persist progress or finish.
    T::DbWeight::get().reads_writes(3, 2)
}

fn claimed_row_base_weight<T: Config>() -> Weight {
    // Iterate/read and remove one RootClaimed row.
    T::DbWeight::get().reads_writes(1, 1)
}

fn claimed_coldkey_flush_weight<T: Config>() -> Weight {
    // Mutate one aggregated BasketClaimed watermark.
    T::DbWeight::get().reads_writes(1, 1)
}

fn claimed_row_worst_case_weight<T: Config>() -> Weight {
    // A row plus the first BasketClaimed update for its coldkey in this bounded pass.
    claimed_row_base_weight::<T>().saturating_add(claimed_coldkey_flush_weight::<T>())
}

fn hotkey_attempt_weight<T: Config>() -> Weight {
    // Conservative fixed storage envelope; per-slot operations are charged as they occur.
    T::DbWeight::get().reads_writes(4, 4)
}

fn principal_clear_item_weight<T: Config>() -> Weight {
    T::DbWeight::get().reads_writes(1, 1)
}

fn reconcile_coldkey_weight<T: Config>() -> Weight {
    // One StakingHotkeys map entry (key plus claimant hotkey vector).
    T::DbWeight::get().reads(1)
}

fn reconcile_position_weight<T: Config>() -> Weight {
    // Conservative envelope for BasketRate/BasketClaimed, the root share pool's legacy + v2
    // numerator/denominator/value reads, and the BasketShares accumulation.
    T::DbWeight::get().reads_writes(10, 1)
}

fn affordable_items(budget: Weight, per_item: Weight) -> u64 {
    let by_ref_time = if per_item.ref_time() == 0 {
        u64::MAX
    } else {
        budget.ref_time() / per_item.ref_time()
    };
    let by_proof_size = if per_item.proof_size() == 0 {
        u64::MAX
    } else {
        budget.proof_size() / per_item.proof_size()
    };
    by_ref_time.min(by_proof_size)
}

fn next_item_fits<T: Config>(used: Weight, item: Weight, limit: Weight) -> bool {
    // Keep room to persist a cursor, or to kill it and mark completion.
    used.saturating_add(item)
        .saturating_add(T::DbWeight::get().writes(2))
        .all_lte(limit)
}

/// Idempotent `on_runtime_upgrade` entry: ensure the multi-block seed cursor exists, but do
/// **not** convert any hotkeys. Conversion is owned by `on_idle` via
/// [`migrate_seed_beta_basket_v2`].
///
/// Running conversion inside ORU breaks try-runtime idempotency: the second ORU pass would
/// advance the cursor further and change storage.
pub fn kickoff_seed_beta_basket_v2<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    weight.saturating_accrue(T::DbWeight::get().reads(1));
    if SeedBetaBasketV2Migration::<T>::exists() {
        // Cursor already present — on_idle owns progress. No-op so a second ORU matches.
        return weight;
    }

    log::info!(
        target: "runtime",
        "Kicking off migration '{}' (conversion continues on_idle)",
        String::from_utf8_lossy(&migration_name)
    );
    SeedBetaBasketV2Migration::<T>::put(SeedBetaBasketV2Progress::Convert {
        after: None,
        hotkey: None,
    });
    weight.saturating_accrue(T::DbWeight::get().writes(1));
    weight
}

/// True while the multi-block seed is unfinished (cursor present). Basket deposits and claims
/// must not run while this is set — see `distribute_root_alpha_to_basket` /
/// `do_stake_into_basket` / `do_root_claim`.
pub fn seed_beta_basket_v2_in_progress<T: Config>() -> bool {
    SeedBetaBasketV2Migration::<T>::exists()
}

/// Same as [`migrate_seed_beta_basket_v2`] but with explicit per-pass limits (for tests).
pub fn migrate_seed_beta_basket_v2_limited<T: Config>(
    hotkeys_per_pass: u32,
    principal_clear_per_pass: u32,
    claimed_drains_per_pass: u32,
) -> Weight {
    migrate_seed_beta_basket_v2_inner::<T>(
        hotkeys_per_pass,
        principal_clear_per_pass,
        claimed_drains_per_pass,
        None,
    )
}

fn migrate_seed_beta_basket_v2_inner<T: Config>(
    hotkeys_per_pass: u32,
    principal_clear_per_pass: u32,
    claimed_drains_per_pass: u32,
    adaptive_limit: Option<Weight>,
) -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    weight.saturating_accrue(T::DbWeight::get().reads(1));
    let mut progress = match SeedBetaBasketV2Migration::<T>::get() {
        Some(p) => p,
        None => {
            log::info!(
                "Running migration '{}'",
                String::from_utf8_lossy(&migration_name)
            );
            // Persist the cursor immediately so `seed_beta_basket_v2_in_progress` is true for
            // the rest of this block (and until completion). Basket deposits/claims gate on
            // that flag so coinbase cannot write live BasketRate/Shares that a later pass
            // would overwrite.
            let p = SeedBetaBasketV2Progress::Convert {
                after: None,
                hotkey: None,
            };
            SeedBetaBasketV2Migration::<T>::put(&p);
            weight.saturating_accrue(T::DbWeight::get().writes(1));
            p
        }
    };

    let escrow = Pallet::<T>::get_beta_escrow_account_id();
    weight.saturating_accrue(T::DbWeight::get().reads(1));

    let mut seeded_slots: u64 = 0;
    let mut made_progress = false;

    'phases: loop {
        match progress {
            SeedBetaBasketV2Progress::Convert {
                mut after,
                mut hotkey,
            } => {
                let mut claimed_budget = claimed_drains_per_pass;
                let mut hotkeys_done_this_pass: u32 = 0;

                loop {
                    let resume_state = hotkey.take();
                    let current_hotkey: T::AccountId = if let Some(ref state) = resume_state {
                        match T::AccountId::decode(&mut &state.hotkey[..]) {
                            Ok(hk) => hk,
                            Err(_) => {
                                // Cannot happen in practice (we only ever encode a valid
                                // `T::AccountId` into this field). Fail closed if storage is
                                // corrupt: preserve the cursor and all legacy state for explicit
                                // operator recovery rather than silently declaring completion.
                                log::error!(
                                    "Migration 'migrate_seed_beta_basket_v2' found an undecodable hotkey cursor; preserving it and stopping this pass."
                                );
                                return weight;
                            }
                        }
                    } else {
                        if hotkeys_done_this_pass >= hotkeys_per_pass {
                            progress = SeedBetaBasketV2Progress::Convert {
                                after,
                                hotkey: None,
                            };
                            SeedBetaBasketV2Migration::<T>::put(progress);
                            weight.saturating_accrue(T::DbWeight::get().writes(1));
                            log::info!(
                                "Migration 'migrate_seed_beta_basket_v2' paused after converting {} hotkey(s); will resume on the next pass.",
                                hotkeys_done_this_pass
                            );
                            return weight;
                        }
                        if made_progress
                            && adaptive_limit.is_some_and(|limit| {
                                !next_item_fits::<T>(weight, hotkey_attempt_weight::<T>(), limit)
                            })
                        {
                            progress = SeedBetaBasketV2Progress::Convert {
                                after,
                                hotkey: None,
                            };
                            SeedBetaBasketV2Migration::<T>::put(progress);
                            weight.saturating_accrue(T::DbWeight::get().writes(1));
                            log::info!(
                                "Migration 'migrate_seed_beta_basket_v2' paused after converting {} hotkey(s) at its adaptive weight limit; will resume on_idle.",
                                hotkeys_done_this_pass
                            );
                            return weight;
                        }

                        let mut keys_iter = match after {
                            Some(ref raw) => RootClaimable::<T>::iter_keys_from(raw.clone()),
                            None => RootClaimable::<T>::iter_keys(),
                        };
                        match keys_iter.next() {
                            Some(hk) => {
                                weight.saturating_accrue(T::DbWeight::get().reads(1));
                                after = Some(RootClaimable::<T>::hashed_key_for(&hk));
                                hk
                            }
                            None => {
                                progress = SeedBetaBasketV2Progress::ClearShares { cursor: None };
                                continue 'phases;
                            }
                        }
                    };

                    match convert_hotkey_step::<T>(
                        &current_hotkey,
                        &escrow,
                        resume_state,
                        &mut claimed_budget,
                        &mut weight,
                        adaptive_limit,
                        &mut made_progress,
                    ) {
                        ConvertOutcome::Done(slots) => {
                            seeded_slots = seeded_slots.saturating_add(slots);
                            hotkeys_done_this_pass = hotkeys_done_this_pass.saturating_add(1);
                            made_progress = true;

                            if hotkeys_done_this_pass >= hotkeys_per_pass || claimed_budget == 0 {
                                progress = SeedBetaBasketV2Progress::Convert {
                                    after,
                                    hotkey: None,
                                };
                                SeedBetaBasketV2Migration::<T>::put(progress);
                                weight.saturating_accrue(T::DbWeight::get().writes(1));
                                log::info!(
                                    "Migration 'migrate_seed_beta_basket_v2' paused after converting {} hotkey(s); will resume on the next pass.",
                                    hotkeys_done_this_pass
                                );
                                return weight;
                            }
                            // Budget remains and the hotkey cap isn't hit: keep converting.
                        }
                        ConvertOutcome::Paused(state) => {
                            progress = SeedBetaBasketV2Progress::Convert {
                                after,
                                hotkey: Some(state),
                            };
                            SeedBetaBasketV2Migration::<T>::put(progress);
                            weight.saturating_accrue(T::DbWeight::get().writes(1));
                            log::info!(
                                "Migration 'migrate_seed_beta_basket_v2' paused mid-hotkey after reaching its {} RootClaimed-row/adaptive weight budget; will resume on_idle.",
                                claimed_drains_per_pass
                            );
                            return weight;
                        }
                    }
                }
            }
            SeedBetaBasketV2Progress::ClearShares { cursor } => {
                let mut clear_limit = principal_clear_per_pass;
                if let Some(limit) = adaptive_limit {
                    let remaining = limit
                        .saturating_sub(weight)
                        .saturating_sub(T::DbWeight::get().writes(2));
                    let affordable =
                        affordable_items(remaining, principal_clear_item_weight::<T>());
                    if affordable == 0 && made_progress {
                        SeedBetaBasketV2Migration::<T>::put(
                            SeedBetaBasketV2Progress::ClearShares { cursor },
                        );
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                        log::info!(
                            "Migration 'migrate_seed_beta_basket_v2' paused before provisional share cleanup at its adaptive weight limit; will resume on_idle."
                        );
                        return weight;
                    }
                    clear_limit =
                        clear_limit.min(affordable.max(1).min(u64::from(u32::MAX)) as u32);
                }

                let shares_removal = BasketShares::<T>::clear(clear_limit, cursor.as_deref());
                weight.saturating_accrue(
                    T::DbWeight::get()
                        .reads_writes(shares_removal.loops as u64, shares_removal.backend as u64),
                );
                made_progress |= shares_removal.backend != 0;

                if let Some(next) = shares_removal.maybe_cursor {
                    progress = SeedBetaBasketV2Progress::ClearShares { cursor: Some(next) };
                    SeedBetaBasketV2Migration::<T>::put(progress);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                    log::info!(
                        "Migration 'migrate_seed_beta_basket_v2' paused after clearing {} provisional BasketShares entries; will resume on the next pass.",
                        shares_removal.backend
                    );
                    return weight;
                }

                progress = SeedBetaBasketV2Progress::ReconcileClaimants {
                    after: None,
                    coldkey: None,
                    hotkey_index: 0,
                };
                continue 'phases;
            }
            SeedBetaBasketV2Progress::ReconcileClaimants {
                mut after,
                mut coldkey,
                mut hotkey_index,
            } => {
                let mut coldkeys_done_this_pass = 0u32;
                let mut positions_done_this_pass = 0u32;

                loop {
                    if coldkey.is_none() && coldkeys_done_this_pass >= hotkeys_per_pass {
                        SeedBetaBasketV2Migration::<T>::put(
                            SeedBetaBasketV2Progress::ReconcileClaimants {
                                after,
                                coldkey,
                                hotkey_index,
                            },
                        );
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                        return weight;
                    }
                    if coldkey.is_none()
                        && made_progress
                        && adaptive_limit.is_some_and(|limit| {
                            !next_item_fits::<T>(weight, reconcile_coldkey_weight::<T>(), limit)
                        })
                    {
                        SeedBetaBasketV2Migration::<T>::put(
                            SeedBetaBasketV2Progress::ReconcileClaimants {
                                after,
                                coldkey,
                                hotkey_index,
                            },
                        );
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                        return weight;
                    }

                    let (current_coldkey, hotkeys) = if let Some(encoded) = coldkey.as_ref() {
                        let current = match T::AccountId::decode(&mut &encoded[..]) {
                            Ok(current) => current,
                            Err(_) => {
                                log::error!(
                                    "Migration 'migrate_seed_beta_basket_v2' found an undecodable claimant cursor; preserving it and stopping this pass."
                                );
                                return weight;
                            }
                        };
                        let hotkeys = StakingHotkeys::<T>::get(&current);
                        weight.saturating_accrue(T::DbWeight::get().reads(1));
                        (current, hotkeys)
                    } else {
                        let mut iter = match after.as_ref() {
                            Some(raw) => StakingHotkeys::<T>::iter_from(raw.clone()),
                            None => StakingHotkeys::<T>::iter(),
                        };
                        let Some((current, hotkeys)) = iter.next() else {
                            progress = SeedBetaBasketV2Progress::ClearClaimed { cursor: None };
                            continue 'phases;
                        };
                        weight.saturating_accrue(T::DbWeight::get().reads(1));
                        coldkey = Some(current.encode());
                        hotkey_index = 0;
                        (current, hotkeys)
                    };

                    // The escrow is fund custody, never a claimant.
                    if current_coldkey == escrow {
                        after = Some(StakingHotkeys::<T>::hashed_key_for(&current_coldkey));
                        coldkey = None;
                        hotkey_index = 0;
                        coldkeys_done_this_pass = coldkeys_done_this_pass.saturating_add(1);
                        made_progress = true;
                        continue;
                    }

                    while (hotkey_index as usize) < hotkeys.len() {
                        if positions_done_this_pass >= claimed_drains_per_pass
                            || (made_progress
                                && adaptive_limit.is_some_and(|limit| {
                                    !next_item_fits::<T>(
                                        weight,
                                        reconcile_position_weight::<T>(),
                                        limit,
                                    )
                                }))
                        {
                            SeedBetaBasketV2Migration::<T>::put(
                                SeedBetaBasketV2Progress::ReconcileClaimants {
                                    after,
                                    coldkey,
                                    hotkey_index,
                                },
                            );
                            weight.saturating_accrue(T::DbWeight::get().writes(1));
                            log::info!(
                                "Migration 'migrate_seed_beta_basket_v2' paused after reconciling {} claimant position(s); will resume on_idle.",
                                positions_done_this_pass
                            );
                            return weight;
                        }

                        let hotkey = &hotkeys[hotkey_index as usize];
                        let owed = Pallet::<T>::get_basket_owed_shares(hotkey, &current_coldkey);
                        if owed != 0 {
                            BasketShares::<T>::mutate(hotkey, |shares| {
                                *shares = shares.saturating_add(owed);
                            });
                        }
                        weight.saturating_accrue(reconcile_position_weight::<T>());
                        hotkey_index = hotkey_index.saturating_add(1);
                        positions_done_this_pass = positions_done_this_pass.saturating_add(1);
                        made_progress = true;
                    }

                    after = Some(StakingHotkeys::<T>::hashed_key_for(&current_coldkey));
                    coldkey = None;
                    hotkey_index = 0;
                    coldkeys_done_this_pass = coldkeys_done_this_pass.saturating_add(1);
                }
            }
            SeedBetaBasketV2Progress::ClearClaimed { cursor } => {
                let mut clear_limit = claimed_drains_per_pass;
                if let Some(limit) = adaptive_limit {
                    let remaining = limit
                        .saturating_sub(weight)
                        .saturating_sub(T::DbWeight::get().writes(2));
                    let affordable = affordable_items(remaining, claimed_row_base_weight::<T>());
                    if affordable == 0 && made_progress {
                        SeedBetaBasketV2Migration::<T>::put(
                            SeedBetaBasketV2Progress::ClearClaimed { cursor },
                        );
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                        log::info!(
                            "Migration 'migrate_seed_beta_basket_v2' paused before orphaned RootClaimed cleanup at its adaptive weight limit; will resume on_idle."
                        );
                        return weight;
                    }
                    clear_limit =
                        clear_limit.min(affordable.max(1).min(u64::from(u32::MAX)) as u32);
                }

                let claimed_removal = RootClaimed::<T>::clear(clear_limit, cursor.as_deref());
                weight.saturating_accrue(
                    T::DbWeight::get()
                        .reads_writes(claimed_removal.loops as u64, claimed_removal.backend as u64),
                );
                made_progress |= claimed_removal.backend != 0;

                if let Some(next) = claimed_removal.maybe_cursor {
                    progress = SeedBetaBasketV2Progress::ClearClaimed { cursor: Some(next) };
                    SeedBetaBasketV2Migration::<T>::put(progress);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                    log::info!(
                        "Migration 'migrate_seed_beta_basket_v2' paused after clearing {} orphaned RootClaimed entries; will resume on the next pass.",
                        claimed_removal.backend
                    );
                    return weight;
                }

                progress = SeedBetaBasketV2Progress::ClearPrincipal { cursor: None };
                continue 'phases;
            }
            SeedBetaBasketV2Progress::ClearPrincipal { cursor } => {
                let mut clear_limit = principal_clear_per_pass;
                if let Some(limit) = adaptive_limit {
                    let remaining = limit
                        .saturating_sub(weight)
                        .saturating_sub(T::DbWeight::get().writes(2));
                    let affordable =
                        affordable_items(remaining, principal_clear_item_weight::<T>());
                    if affordable == 0 && made_progress {
                        SeedBetaBasketV2Migration::<T>::put(
                            SeedBetaBasketV2Progress::ClearPrincipal { cursor },
                        );
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                        log::info!(
                            "Migration 'migrate_seed_beta_basket_v2' paused before principal cleanup at its adaptive weight limit; will resume on_idle."
                        );
                        return weight;
                    }
                    clear_limit =
                        clear_limit.min(affordable.max(1).min(u64::from(u32::MAX)) as u32);
                }

                let principal_removal =
                    deprecated::BasketPrincipal::<T>::clear(clear_limit, cursor.as_deref());
                weight.saturating_accrue(T::DbWeight::get().reads_writes(
                    principal_removal.loops as u64,
                    principal_removal.backend as u64,
                ));

                if let Some(next) = principal_removal.maybe_cursor {
                    progress = SeedBetaBasketV2Progress::ClearPrincipal { cursor: Some(next) };
                    SeedBetaBasketV2Migration::<T>::put(progress);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                    log::info!(
                        "Migration 'migrate_seed_beta_basket_v2' paused after clearing {} BasketPrincipal entries; will resume on the next pass.",
                        principal_removal.backend
                    );
                    return weight;
                }

                // Fully finished.
                SeedBetaBasketV2Migration::<T>::kill();
                HasMigrationRun::<T>::insert(&migration_name, true);
                weight.saturating_accrue(T::DbWeight::get().writes(2));

                log::info!(
                    "Migration 'migrate_seed_beta_basket_v2' completed. Seeded {seeded_slots} slots this pass, cleared {} orphaned BasketPrincipal entries this pass.",
                    principal_removal.backend
                );
                return weight;
            }
        }
    }
}

/// Outcome of a single [`convert_hotkey_step`] call.
enum ConvertOutcome {
    /// The hotkey's legacy state was fully converted; carries the number of subnet slots that
    /// contributed outstanding shares.
    Done(u64),
    /// The per-pass `RootClaimed` drain budget ran out before the hotkey finished; carries the
    /// state needed to resume.
    Paused(HotkeyConvertState),
}

/// Convert one hotkey's legacy claim state into the unified fund, resuming from `resume` if
/// given. Bounded by `claimed_budget`, which is decremented for every `RootClaimed` row drained
/// and shared across every hotkey processed in the pass. `RootClaimable[hotkey]` is only
/// `remove`d once every subnet slot has been fully processed — never on a paused/resumed call.
///
/// Per-slot conversion math matches the original one-shot migration body exactly. Claimed-share
/// increments are aggregated by coldkey in memory for this bounded call and flushed before any
/// cursor is persisted, avoiding repeated `BasketClaimed` reads/writes when the same coldkey has
/// legacy watermarks across many subnet slots.
fn convert_hotkey_step<T: Config>(
    hotkey: &T::AccountId,
    escrow: &T::AccountId,
    resume: Option<HotkeyConvertState>,
    claimed_budget: &mut u32,
    weight: &mut Weight,
    adaptive_limit: Option<Weight>,
    made_progress: &mut bool,
) -> ConvertOutcome {
    // Peeked, not taken: the legacy entry must survive until conversion fully completes.
    let claimable = RootClaimable::<T>::get(hotkey);
    weight.saturating_accrue(T::DbWeight::get().reads(1));

    let (
        mut done_through,
        mut partial,
        mut fund_rate,
        mut fund_shares,
        mut seeded_slots,
        total_root,
    ) = match resume {
        Some(state) => (
            state.done_through,
            state.partial,
            state.fund_rate,
            state.fund_shares,
            state.seeded_slots,
            state.total_root,
        ),
        None => {
            // Snapshot once at the start of this hotkey's conversion; resumes reuse it.
            let total_root: I96F32 = I96F32::saturating_from_num(
                Pallet::<T>::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT).saturating_sub(
                    // On a v1 chain the escrow may already hold a root-slot position; it is
                    // custody, not a claimant, so it is excluded from the claimant base.
                    Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey,
                        escrow,
                        NetUid::ROOT,
                    ),
                ),
            );
            weight.saturating_accrue(T::DbWeight::get().reads(2));
            (
                None,
                None,
                I96F32::saturating_from_num(0),
                0u64,
                0u64,
                total_root,
            )
        }
    };

    // Copied once: subsequent slots are always greater in `BTreeMap` order, so the filter never
    // needs to observe `done_through` updates made later in this loop.
    let initial_done_through = done_through;
    let slots = claimable
        .iter()
        .filter(|(netuid, _)| match initial_done_through {
            Some(d) => **netuid > d,
            None => true,
        });
    let mut drained_row_this_step = false;
    let mut claimed_increments: BTreeMap<T::AccountId, i128> = BTreeMap::new();

    for (netuid, rate) in slots {
        // Resume the in-progress slot's partial drain (and its snapshotted price), if this is
        // it. Otherwise fix a fresh conversion price for the slot: moving/EMA (manipulation
        // resistant), spot fallback for cold EMAs, 1:1 for root.
        let (mut claimed_sum, drain_from, price) =
            if let Some((p_netuid, p_rate, p_price, cursor, sum)) = partial.take() {
                debug_assert_eq!(p_netuid, *netuid);
                debug_assert_eq!(p_rate, *rate);
                (sum, Some(cursor), p_price)
            } else {
                let price: U96F32 = if netuid.is_root() {
                    U96F32::saturating_from_num(1)
                } else {
                    let moving: U96F32 =
                        U96F32::saturating_from_num(Pallet::<T>::get_moving_alpha_price(*netuid));
                    if moving > U96F32::saturating_from_num(0) {
                        moving
                    } else {
                        U96F32::saturating_from_num(T::SwapInterface::current_alpha_price(
                            (*netuid).into(),
                        ))
                    }
                };
                weight.saturating_accrue(T::DbWeight::get().reads(1));
                (I96F32::saturating_from_num(0), None, price)
            };

        // Gross credited principal (alpha) = rate * total_root_stake.
        let gross: I96F32 = rate.saturating_mul(total_root);

        // Total already claimed by all coldkeys on this (netuid, hotkey), converting each
        // coldkey's watermark to TAO-valued fund shares while we scan. Bounded: at most
        // `claimed_budget` rows are drained (and explicitly removed) per call; the raw storage
        // cursor is persisted on pause so draining resumes exactly where it left off.
        let mut claimed_iter = match drain_from {
            Some(cursor) => RootClaimed::<T>::iter_prefix_from((*netuid, hotkey), cursor),
            None => RootClaimed::<T>::iter_prefix((*netuid, hotkey)),
        };

        let mut budget_exhausted = false;
        loop {
            if *claimed_budget == 0 {
                budget_exhausted = true;
                break;
            }
            if drained_row_this_step
                && adaptive_limit.is_some_and(|limit| {
                    !next_item_fits::<T>(*weight, claimed_row_worst_case_weight::<T>(), limit)
                })
            {
                budget_exhausted = true;
                break;
            }
            let Some((coldkey, claimed)) = claimed_iter.next() else {
                break;
            };

            claimed_sum = claimed_sum.saturating_add(I96F32::saturating_from_num(claimed));
            let claimed_shares: i128 = U96F32::saturating_from_num(claimed)
                .saturating_mul(price)
                .saturating_to_num::<i128>();
            let first_for_coldkey = !claimed_increments.contains_key(&coldkey);
            claimed_increments
                .entry(coldkey.clone())
                .and_modify(|increment| {
                    *increment = increment.saturating_add(claimed_shares);
                })
                .or_insert(claimed_shares);
            RootClaimed::<T>::remove((*netuid, hotkey, &coldkey));
            weight.saturating_accrue(if first_for_coldkey {
                // Reserve the later BasketClaimed flush on the first row for this coldkey.
                claimed_row_worst_case_weight::<T>()
            } else {
                claimed_row_base_weight::<T>()
            });
            *claimed_budget = claimed_budget.saturating_sub(1);
            *made_progress = true;
            drained_row_this_step = true;
        }

        if budget_exhausted {
            flush_claimed_increments::<T>(hotkey, &mut claimed_increments);
            return ConvertOutcome::Paused(HotkeyConvertState {
                hotkey: hotkey.encode(),
                done_through,
                total_root,
                partial: Some((
                    *netuid,
                    *rate,
                    price,
                    claimed_iter.last_raw_key().to_vec(),
                    claimed_sum,
                )),
                fund_rate,
                fund_shares,
                seeded_slots,
            });
        }

        // `RootClaimed` for this slot is fully drained — finalize its contribution.

        // Remaining unclaimed (still-outstanding) principal, in alpha.
        let remaining_f: I96F32 = gross.saturating_sub(claimed_sum);
        let mut remaining: u64 = if remaining_f.is_negative() {
            0
        } else {
            remaining_f.saturating_to_num::<u64>()
        };

        // Unified rate contribution: the legacy alpha-rate re-denominated to shares at p_s.
        // (May be haircut below for an underbacked root slot.)
        let mut rate_contribution: I96F32 = rate.saturating_mul(I96F32::saturating_from_num(price));

        let existing: u64 =
            Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, *netuid)
                .to_u64();
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        if netuid.is_root() {
            // Root-slot entries only exist on v1 chains. Root has no pool to attribute
            // unbacked alpha from, so the share contribution is capped at the escrow's
            // actual root stake (never top up, never mint unbacked shares).
            let capped = remaining.min(existing);
            if capped < remaining {
                // Underbacked (degenerate v1 state): haircut the rate so `Σ owed == P`
                // still holds — solve `rate_eff * total_root - claimed_sum == capped`,
                // spreading the shortfall pro-rata by stake.
                rate_contribution = I96F32::saturating_from_num(capped)
                    .saturating_add(claimed_sum)
                    .checked_div(total_root)
                    .unwrap_or(I96F32::saturating_from_num(0));
            }
            remaining = capped;
        } else if remaining > 0 && existing < remaining {
            // Attribute the still-unattributed outstanding alpha to the validator under the
            // escrow coldkey. On a fresh (mainnet) chain `existing == 0` and this stakes the
            // full `remaining`; on a v1 chain it only tops up any shortfall, and a
            // compounded surplus (`existing > remaining`) is left in place so the old slot's
            // `E/P` multiplier carries into the fund's `N/P`.
            Pallet::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                escrow,
                *netuid,
                AlphaBalance::from(remaining.saturating_sub(existing)),
            );
            weight.saturating_accrue(T::DbWeight::get().writes(1));
        }

        fund_rate = fund_rate.saturating_add(rate_contribution);

        if remaining != 0 {
            // Outstanding fund shares: TAO value of the remaining alpha at p_s.
            fund_shares = fund_shares.saturating_add(
                U96F32::saturating_from_num(remaining)
                    .saturating_mul(price)
                    .saturating_to_num::<u64>(),
            );
            seeded_slots = seeded_slots.saturating_add(1);
        }

        done_through = Some(*netuid);
    }

    flush_claimed_increments::<T>(hotkey, &mut claimed_increments);

    // Every subnet slot converted — only now is it safe to drop the legacy claimable entry.
    RootClaimable::<T>::remove(hotkey);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    if fund_rate != I96F32::saturating_from_num(0) {
        BasketRate::<T>::insert(hotkey, fund_rate);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
    }
    if fund_shares != 0 {
        BasketShares::<T>::insert(hotkey, fund_shares);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
    }

    ConvertOutcome::Done(seeded_slots)
}

/// Flush the bounded in-memory aggregation. Its read/write weight was reserved when each
/// coldkey first entered the map, so callers must not charge it again here.
fn flush_claimed_increments<T: Config>(
    hotkey: &T::AccountId,
    increments: &mut BTreeMap<T::AccountId, i128>,
) {
    for (coldkey, increment) in sp_std::mem::take(increments) {
        if increment != 0 {
            BasketClaimed::<T>::mutate(hotkey, &coldkey, |claimed| {
                *claimed = claimed.saturating_add(increment);
            });
        }
    }
}
