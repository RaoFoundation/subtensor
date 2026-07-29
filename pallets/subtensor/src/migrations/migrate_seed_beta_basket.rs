use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::pallet_prelude::{Blake2_128Concat, Identity, OptionQuery, ValueQuery};
use frame_support::storage_alias;
use frame_support::weights::Weight;
use scale_info::TypeInfo;
use scale_info::prelude::string::String;
use sp_std::vec::Vec;
use substrate_fixed::types::{I96F32, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid};
use subtensor_swap_interface::SwapHandler;

/// Hard per-pass cap on how many `RootClaimable` hotkeys are converted in one block.
/// Sized to keep a single `on_runtime_upgrade` / `on_idle` pass inside the block weight budget
/// even when each hotkey has several subnet slots and claimed watermarks.
pub const MAX_SEED_BETA_BASKET_HOTKEYS_PER_PASS: u32 = 32;

/// Hard per-pass cap on orphaned `BasketPrincipal` entries cleared in one block.
pub const MAX_SEED_BETA_BASKET_PRINCIPAL_CLEAR_PER_PASS: u32 = 256;

/// Hard per-pass cap on `RootClaimed` rows drained while converting hotkeys. A single subnet
/// slot's `(netuid, hotkey)` prefix can hold one row per historical claimant coldkey, which is
/// unbounded in principle; this caps the *whole pass* (across every hotkey touched in that
/// pass), not just one slot. When the budget runs out mid-hotkey (possibly mid-slot), progress
/// is persisted in the `Convert` variant's nested [`HotkeyConvertState`] and resumed on the next
/// `on_idle` pass — `RootClaimable[hotkey]` is left untouched in storage until the hotkey is
/// fully converted.
pub const MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS: u32 = 512;

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
/// * `BasketShares[hot] = Σ_s remaining_s * p_s`
/// * `BasketClaimed[hot, ck] = Σ_s claimed_s(ck) * p_s`
///
/// With NAV marked at the same `p_s`, `N == P` at the seed, and every staker's owed TAO value is
/// preserved exactly: `owed_new = Σ_s p_s (rate_s * stake - claimed_s)`. The drained legacy maps
/// are cleared so no per-subnet claim state survives.
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
/// Work is chunked across blocks at two levels. Each pass converts at most
/// [`MAX_SEED_BETA_BASKET_HOTKEYS_PER_PASS`] hotkeys, then clears at most
/// [`MAX_SEED_BETA_BASKET_PRINCIPAL_CLEAR_PER_PASS`] orphaned principal rows. Within the convert
/// phase, a single pass also drains at most [`MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS`]
/// `RootClaimed` rows in total: a hotkey with many claimant coldkeys on one subnet slot is
/// converted across several passes rather than draining its whole (unbounded) prefix at once.
/// While a hotkey is only partially converted, `RootClaimable[hotkey]` is left untouched in
/// storage and the in-flight state (completed slots, the in-progress slot's drain cursor, and
/// accumulators) is persisted in [`SeedBetaBasketV2Migration`]'s `Convert::hotkey`. Progress is
/// continued from `on_idle` until finished. `HasMigrationRun` is set only when both phases
/// complete.
pub fn migrate_seed_beta_basket_v2<T: Config>() -> Weight {
    migrate_seed_beta_basket_v2_limited::<T>(
        MAX_SEED_BETA_BASKET_HOTKEYS_PER_PASS,
        MAX_SEED_BETA_BASKET_PRINCIPAL_CLEAR_PER_PASS,
        MAX_SEED_BETA_BASKET_CLAIMED_DRAINS_PER_PASS,
    )
}

/// True while a prior pass left unfinished work (cursor present, flag not yet set).
pub fn seed_beta_basket_v2_in_progress<T: Config>() -> bool {
    SeedBetaBasketV2Migration::<T>::exists()
}

/// Same as [`migrate_seed_beta_basket_v2`] but with explicit per-pass limits (for tests).
pub fn migrate_seed_beta_basket_v2_limited<T: Config>(
    hotkeys_per_pass: u32,
    principal_clear_per_pass: u32,
    claimed_drains_per_pass: u32,
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
            SeedBetaBasketV2Progress::Convert {
                after: None,
                hotkey: None,
            }
        }
    };

    let escrow = Pallet::<T>::get_beta_escrow_account_id();
    weight.saturating_accrue(T::DbWeight::get().reads(1));

    let mut seeded_slots: u64 = 0;

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
                                // `T::AccountId` into this field), but never stall the upgrade
                                // over a corrupt cursor: drop it and move on to principal clear.
                                log::error!(
                                    "Migration 'migrate_seed_beta_basket_v2' found an undecodable hotkey cursor; dropping it and moving to principal clear."
                                );
                                progress =
                                    SeedBetaBasketV2Progress::ClearPrincipal { cursor: None };
                                continue 'phases;
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
                                "Migration 'migrate_seed_beta_basket_v2' paused after converting {} hotkey(s); will resume on_idle.",
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
                                progress =
                                    SeedBetaBasketV2Progress::ClearPrincipal { cursor: None };
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
                    ) {
                        ConvertOutcome::Done(slots) => {
                            seeded_slots = seeded_slots.saturating_add(slots);
                            hotkeys_done_this_pass = hotkeys_done_this_pass.saturating_add(1);

                            if hotkeys_done_this_pass >= hotkeys_per_pass || claimed_budget == 0 {
                                progress = SeedBetaBasketV2Progress::Convert {
                                    after,
                                    hotkey: None,
                                };
                                SeedBetaBasketV2Migration::<T>::put(progress);
                                weight.saturating_accrue(T::DbWeight::get().writes(1));
                                log::info!(
                                    "Migration 'migrate_seed_beta_basket_v2' paused after converting {} hotkey(s); will resume on_idle.",
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
                                "Migration 'migrate_seed_beta_basket_v2' paused mid-hotkey after exhausting its {} RootClaimed-row drain budget; will resume on_idle.",
                                claimed_drains_per_pass
                            );
                            return weight;
                        }
                    }
                }
            }
            SeedBetaBasketV2Progress::ClearPrincipal { cursor } => {
                let principal_removal = deprecated::BasketPrincipal::<T>::clear(
                    principal_clear_per_pass,
                    cursor.as_deref(),
                );
                weight.saturating_accrue(T::DbWeight::get().reads_writes(
                    principal_removal.loops as u64,
                    principal_removal.backend as u64,
                ));

                if let Some(next) = principal_removal.maybe_cursor {
                    progress = SeedBetaBasketV2Progress::ClearPrincipal { cursor: Some(next) };
                    SeedBetaBasketV2Migration::<T>::put(progress);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                    log::info!(
                        "Migration 'migrate_seed_beta_basket_v2' paused after clearing {} BasketPrincipal entries; will resume on_idle.",
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
/// Per-slot conversion math matches the original one-shot migration body exactly; only the
/// draining of `RootClaimed` is now bounded and resumable.
fn convert_hotkey_step<T: Config>(
    hotkey: &T::AccountId,
    escrow: &T::AccountId,
    resume: Option<HotkeyConvertState>,
    claimed_budget: &mut u32,
    weight: &mut Weight,
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
            let Some((coldkey, claimed)) = claimed_iter.next() else {
                break;
            };

            claimed_sum = claimed_sum.saturating_add(I96F32::saturating_from_num(claimed));
            let claimed_shares: i128 = U96F32::saturating_from_num(claimed)
                .saturating_mul(price)
                .saturating_to_num::<i128>();
            // Write the watermark incrementally — never accumulate claimants in the cursor
            // value (that map grows without bound across resumes and would also make the
            // hotkey-completion flush unbounded). Bounded by `claimed_budget` per pass.
            if claimed_shares != 0 {
                BasketClaimed::<T>::mutate(hotkey, &coldkey, |c| {
                    *c = c.saturating_add(claimed_shares);
                });
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
            }
            RootClaimed::<T>::remove((*netuid, hotkey, &coldkey));
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
            *claimed_budget = claimed_budget.saturating_sub(1);
        }

        if budget_exhausted {
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
