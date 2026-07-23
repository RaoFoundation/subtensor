use super::*;
use frame_support::pallet_prelude::{Blake2_128Concat, Identity, ValueQuery};
use frame_support::storage_alias;
use frame_support::weights::Weight;
use scale_info::prelude::string::String;
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::{I96F32, U96F32};
use subtensor_runtime_common::{AlphaBalance, NetUid};
use subtensor_swap_interface::SwapHandler;

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
/// NOTE: this scans `RootClaimed` per `(netuid, hotkey)` to total already-claimed amounts.
/// On a large state this is heavy; if it cannot fit a single block it should be converted to a
/// multi-block migration before mainnet deployment.
pub fn migrate_seed_beta_basket_v2<T: Config>() -> Weight {
    let migration_name = b"migrate_seed_beta_basket_v2".to_vec();
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

    let escrow = Pallet::<T>::get_beta_escrow_account_id();
    weight.saturating_accrue(T::DbWeight::get().reads(1));

    let hotkeys: Vec<T::AccountId> = RootClaimable::<T>::iter_keys().collect();
    weight.saturating_accrue(T::DbWeight::get().reads(hotkeys.len() as u64));

    let mut seeded_slots: u64 = 0;

    for hotkey in hotkeys.iter() {
        let total_root: I96F32 = I96F32::saturating_from_num(
            Pallet::<T>::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT).saturating_sub(
                // On a v1 chain the escrow may already hold a root-slot position; it is custody,
                // not a claimant, so it is excluded from the claimant base like everywhere else.
                Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    &escrow,
                    NetUid::ROOT,
                ),
            ),
        );
        weight.saturating_accrue(T::DbWeight::get().reads(2));

        let claimable = RootClaimable::<T>::take(hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        let mut fund_rate: I96F32 = I96F32::saturating_from_num(0);
        let mut fund_shares: u64 = 0;
        let mut fund_claimed: BTreeMap<T::AccountId, i128> = BTreeMap::new();

        for (netuid, rate) in claimable.iter() {
            // Fixed conversion price for this subnet: the moving/EMA price (manipulation
            // resistant), falling back to spot if the EMA has not warmed up yet so legacy
            // claims never convert to zero shares on a young subnet. Root converts 1:1.
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

            // Gross credited principal (alpha) = rate * total_root_stake.
            let gross: I96F32 = rate.saturating_mul(total_root);

            // Total already claimed by all coldkeys on this (netuid, hotkey), converting each
            // coldkey's watermark to TAO-valued fund shares while we scan.
            let mut claimed_sum: I96F32 = I96F32::saturating_from_num(0);
            for (coldkey, claimed) in RootClaimed::<T>::drain_prefix((*netuid, hotkey)) {
                claimed_sum = claimed_sum.saturating_add(I96F32::saturating_from_num(claimed));
                let claimed_shares: i128 = U96F32::saturating_from_num(claimed)
                    .saturating_mul(price)
                    .saturating_to_num::<i128>();
                fund_claimed
                    .entry(coldkey)
                    .and_modify(|c| *c = c.saturating_add(claimed_shares))
                    .or_insert(claimed_shares);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
            }

            // Remaining unclaimed (still-outstanding) principal, in alpha.
            let remaining_f: I96F32 = gross.saturating_sub(claimed_sum);
            let mut remaining: u64 = if remaining_f.is_negative() {
                0
            } else {
                remaining_f.saturating_to_num::<u64>()
            };

            // Unified rate contribution: the legacy alpha-rate re-denominated to shares at p_s.
            // (May be haircut below for an underbacked root slot.)
            let mut rate_contribution: I96F32 =
                rate.saturating_mul(I96F32::saturating_from_num(price));

            let existing: u64 =
                Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, *netuid)
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
                    &escrow,
                    *netuid,
                    AlphaBalance::from(remaining.saturating_sub(existing)),
                );
                weight.saturating_accrue(T::DbWeight::get().writes(1));
            }

            fund_rate = fund_rate.saturating_add(rate_contribution);

            if remaining == 0 {
                continue;
            }

            // Outstanding fund shares: TAO value of the remaining alpha at p_s.
            fund_shares = fund_shares.saturating_add(
                U96F32::saturating_from_num(remaining)
                    .saturating_mul(price)
                    .saturating_to_num::<u64>(),
            );
            seeded_slots = seeded_slots.saturating_add(1);
        }

        if fund_rate != I96F32::saturating_from_num(0) {
            BasketRate::<T>::insert(hotkey, fund_rate);
            weight.saturating_accrue(T::DbWeight::get().writes(1));
        }
        if fund_shares != 0 {
            BasketShares::<T>::insert(hotkey, fund_shares);
            weight.saturating_accrue(T::DbWeight::get().writes(1));
        }
        for (coldkey, claimed) in fund_claimed {
            if claimed != 0 {
                BasketClaimed::<T>::insert(hotkey, coldkey, claimed);
                weight.saturating_accrue(T::DbWeight::get().writes(1));
            }
        }
    }

    // Clear per-slot principal orphaned by the superseded v1 migration (no-op on fresh chains).
    let principal_removal = deprecated::BasketPrincipal::<T>::clear(u32::MAX, None);
    weight.saturating_accrue(T::DbWeight::get().reads_writes(
        principal_removal.loops as u64,
        principal_removal.backend as u64,
    ));

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration 'migrate_seed_beta_basket_v2' completed. Seeded {seeded_slots} slots, cleared {} orphaned BasketPrincipal entries.",
        principal_removal.backend
    );

    weight
}
