use super::*;
use alloc::string::String;
use frame_support::traits::Imbalance;

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_fix_root_pot_shortfall";

/// Reconcile the root subnet's TAO pot with what root stakers actually hold.
///
/// Root (netuid 0) pays unstakes 1:1 out of `SubnetTAO[0]`, which is backed by the root
/// subnet account. Historical root dividends were credited to staker holdings
/// (`TotalHotkeyAlpha[·, 0]`) without moving TAO into that pot or its counter, so the pot is
/// short by the accumulated difference and the last unstakers cannot exit.
///
/// The migration recomputes the exact gap at upgrade time,
/// `sum(TotalHotkeyAlpha[·, 0]) - SubnetTAO[0]`, mints it into the root subnet account and
/// raises `SubnetTAO[0]` and `TotalStake` by the same amount. Minting (rather than only
/// bumping counters) is required because the account itself is physically short.
///
/// Idempotent: guarded by `HasMigrationRun`, and a re-run would find a zero gap anyway.
pub fn migrate_fix_root_pot_shortfall<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
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

    // Exact root holdings: every hotkey's alpha on netuid 0 (root alpha is TAO 1:1).
    let mut holdings = TaoBalance::ZERO;
    let mut rows_read: u64 = 0;
    for (_, netuid, alpha) in TotalHotkeyAlpha::<T>::iter() {
        rows_read = rows_read.saturating_add(1);
        if netuid.is_root() {
            holdings = holdings.saturating_add(alpha.to_u64().into());
        }
    }
    let recorded = SubnetTAO::<T>::get(NetUid::ROOT);
    weight = weight.saturating_add(T::DbWeight::get().reads(rows_read.saturating_add(1)));

    let gap = holdings.saturating_sub(recorded);
    log::info!(
        "Root holdings = {holdings}, SubnetTAO[0] = {recorded}, shortfall = {gap} ({rows_read} TotalHotkeyAlpha rows scanned)"
    );

    if gap.is_zero() {
        log::info!("Root pot is not short; nothing to top up.");
    } else if let Some(root_pot) = Pallet::<T>::get_subnet_account_id(NetUid::ROOT) {
        let credit = Pallet::<T>::mint_tao(gap);
        let minted = credit.peek();
        match Pallet::<T>::spend_tao(&root_pot, credit, minted) {
            Ok(_) => {
                SubnetTAO::<T>::mutate(NetUid::ROOT, |tao| *tao = tao.saturating_add(minted));
                TotalStake::<T>::mutate(|total| *total = total.saturating_add(minted));
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(3, 4));
                log::info!(
                    "Minted {minted} into the root pot; SubnetTAO[0] and TotalStake raised by the same amount."
                );
                if minted < gap {
                    log::warn!(
                        "Issuance cap allowed only {minted} of the {gap} shortfall to be minted."
                    );
                }
            }
            Err(unspent) => {
                // Undo the issuance bookkeeping; dropping the credit burns it in balances.
                let unspent_amount = unspent.peek();
                TotalIssuance::<T>::mutate(|total| *total = total.saturating_sub(unspent_amount));
                drop(unspent);
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
                log::error!(
                    "Could not credit {minted} to the root pot; issuance reverted, counters untouched."
                );
            }
        }
    } else {
        log::error!("Root subnet account is unavailable; root pot left unchanged.");
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
