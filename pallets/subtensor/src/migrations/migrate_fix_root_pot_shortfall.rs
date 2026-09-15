use super::migrate_total_alpha_staked;
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
/// The sum is read from `TotalAlphaStaked[0]`, the O(1) aggregate that every
/// `TotalHotkeyAlpha` write keeps in step, so the upgrade block does no map walk. If that
/// aggregate's backfill has not finished the migration leaves its marker unset and retries
/// at the next upgrade instead of scanning.
///
/// Idempotent and retryable: the `HasMigrationRun` marker is only set once the whole gap
/// has been minted and credited; a partial mint (issuance cap) or a failed credit leaves it
/// unset so the next upgrade finishes the job, and a re-run finds a zero gap anyway.
///
/// Registered in the runtime `Migrations` tuple through [`fix_root_pot_shortfall::Migration`]
/// so try-runtime validates the reconciliation invariants against real network state.
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

    weight = weight.saturating_add(T::DbWeight::get().reads(1));
    if migrate_total_alpha_staked::in_progress::<T>() {
        log::error!(
            "Migration '{}' deferred: TotalAlphaStaked backfill still in progress, root holdings are not yet aggregated.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    // Exact root holdings: sum of every hotkey's alpha on netuid 0 (root alpha is TAO 1:1),
    // maintained live in TotalAlphaStaked.
    let holdings: TaoBalance = TotalAlphaStaked::<T>::get(NetUid::ROOT).to_u64().into();
    let recorded = SubnetTAO::<T>::get(NetUid::ROOT);
    weight = weight.saturating_add(T::DbWeight::get().reads(2));

    let gap = holdings.saturating_sub(recorded);
    log::info!("Root holdings = {holdings}, SubnetTAO[0] = {recorded}, shortfall = {gap}");

    let mut reconciled = false;
    if gap.is_zero() {
        log::info!("Root pot is not short; nothing to top up.");
        reconciled = true;
    } else if let Some(root_pot) = Pallet::<T>::get_subnet_account_id(NetUid::ROOT) {
        let issuance_before = TotalIssuance::<T>::get();
        let credit = Pallet::<T>::mint_tao(gap);
        let minted = credit.peek();
        match Pallet::<T>::spend_tao(&root_pot, credit, minted) {
            Ok(_) => {
                SubnetTAO::<T>::mutate(NetUid::ROOT, |tao| *tao = tao.saturating_add(minted));
                TotalStake::<T>::mutate(|total| *total = total.saturating_add(minted));
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(4, 4));
                log::info!(
                    "Minted {minted} into the root pot; SubnetTAO[0] and TotalStake raised by the same amount."
                );
                if minted < gap {
                    log::warn!(
                        "Issuance cap allowed only {minted} of the {gap} shortfall to be minted; the migration stays pending and retries at the next upgrade."
                    );
                } else {
                    reconciled = true;
                }

                // Post-conditions: counter matches holdings, the account backs the counter,
                // and issuance moved by exactly what was minted. try-runtime enforces the
                // same checks through `fix_root_pot_shortfall::Migration`.
                let counter_after = SubnetTAO::<T>::get(NetUid::ROOT);
                let pot_after = Pallet::<T>::get_coldkey_balance(&root_pot);
                let issuance_after = TotalIssuance::<T>::get();
                weight = weight.saturating_add(T::DbWeight::get().reads(3));
                if reconciled && counter_after != holdings {
                    log::error!(
                        "Root pot reconciliation left SubnetTAO[0] = {counter_after} but holdings = {holdings}"
                    );
                }
                if pot_after < counter_after {
                    log::error!(
                        "Root pot account {pot_after} still below SubnetTAO[0] = {counter_after} after top-up"
                    );
                }
                if issuance_after != issuance_before.saturating_add(minted) {
                    log::error!(
                        "TotalIssuance moved from {issuance_before} to {issuance_after}, expected +{minted}"
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
                    "Could not credit {minted} to the root pot; issuance reverted, counters untouched, migration stays pending."
                );
            }
        }
    } else {
        log::error!(
            "Root subnet account is unavailable; root pot left unchanged, migration stays pending."
        );
    }

    if reconciled {
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
        log::info!(
            target: "runtime",
            "Migration '{}' completed successfully.",
            String::from_utf8_lossy(&migration_name)
        );
    }

    weight
}

/// [`OnRuntimeUpgrade`](frame_support::traits::OnRuntimeUpgrade) wrapper with try-runtime
/// pre/post-upgrade invariant validation, registered in the runtime `Migrations` tuple so the
/// try-runtime CI jobs verify the reconciliation against real mainnet/testnet/devnet state.
///
/// Validated invariants: the holdings aggregate is complete before the upgrade; root
/// holdings are unchanged by the upgrade; `SubnetTAO[0]` rises by exactly the pre-upgrade
/// shortfall and ends equal to holdings; the root subnet account receives exactly that
/// amount and backs the counter; `TotalStake`, the pallet `TotalIssuance` and the balances
/// total issuance all move by exactly the minted amount; the `HasMigrationRun` marker ends
/// set. On a chain where the migration already ran nothing may move.
pub mod fix_root_pot_shortfall {
    use super::*;
    use frame_support::traits::OnRuntimeUpgrade;
    use sp_std::marker::PhantomData;

    #[cfg(feature = "try-runtime")]
    use codec::{Decode, Encode};
    #[cfg(feature = "try-runtime")]
    use frame_support::ensure;
    #[cfg(feature = "try-runtime")]
    use frame_support::traits::fungible::Inspect;
    #[cfg(feature = "try-runtime")]
    use sp_runtime::TryRuntimeError;

    /// State carried from `pre_upgrade` to `post_upgrade`.
    #[cfg(feature = "try-runtime")]
    #[derive(Encode, Decode)]
    struct PreUpgradeState {
        already_run: bool,
        holdings: u64,
        recorded: u64,
        pot_balance: u64,
        total_issuance: u64,
        balances_issuance: u64,
        total_stake: u64,
    }

    pub struct Migration<T: Config>(PhantomData<T>);

    impl<T: Config> OnRuntimeUpgrade for Migration<T> {
        fn on_runtime_upgrade() -> Weight {
            migrate_fix_root_pot_shortfall::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
            ensure!(
                !migrate_total_alpha_staked::in_progress::<T>(),
                "TotalAlphaStaked backfill must be complete before the root pot is reconciled"
            );
            let root_pot = Pallet::<T>::get_subnet_account_id(NetUid::ROOT)
                .ok_or("root subnet account must resolve")?;
            Ok(PreUpgradeState {
                already_run: HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                holdings: TotalAlphaStaked::<T>::get(NetUid::ROOT).to_u64(),
                recorded: SubnetTAO::<T>::get(NetUid::ROOT).to_u64(),
                pot_balance: Pallet::<T>::get_coldkey_balance(&root_pot).to_u64(),
                total_issuance: TotalIssuance::<T>::get().to_u64(),
                balances_issuance: <T as Config>::Currency::total_issuance().to_u64(),
                total_stake: TotalStake::<T>::get().to_u64(),
            }
            .encode())
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
            let before: PreUpgradeState =
                Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
            let root_pot = Pallet::<T>::get_subnet_account_id(NetUid::ROOT)
                .ok_or("root subnet account must resolve")?;

            let holdings = TotalAlphaStaked::<T>::get(NetUid::ROOT).to_u64();
            let recorded = SubnetTAO::<T>::get(NetUid::ROOT).to_u64();
            let pot_balance = Pallet::<T>::get_coldkey_balance(&root_pot).to_u64();
            let total_issuance = TotalIssuance::<T>::get().to_u64();
            let balances_issuance = <T as Config>::Currency::total_issuance().to_u64();
            let total_stake = TotalStake::<T>::get().to_u64();

            ensure!(
                holdings == before.holdings,
                "root holdings must not change during reconciliation"
            );
            let expected_mint = if before.already_run {
                0
            } else {
                before.holdings.saturating_sub(before.recorded)
            };
            ensure!(
                recorded == before.recorded.saturating_add(expected_mint),
                "SubnetTAO[0] must rise by exactly the pre-upgrade shortfall"
            );
            ensure!(
                recorded >= holdings,
                "SubnetTAO[0] must cover root holdings after reconciliation"
            );
            ensure!(
                pot_balance == before.pot_balance.saturating_add(expected_mint),
                "the root subnet account must receive exactly the minted amount"
            );
            ensure!(
                pot_balance >= recorded,
                "the root subnet account must back SubnetTAO[0]"
            );
            ensure!(
                total_stake == before.total_stake.saturating_add(expected_mint),
                "TotalStake must rise by exactly the minted amount"
            );
            ensure!(
                total_issuance == before.total_issuance.saturating_add(expected_mint),
                "TotalIssuance must rise by exactly the minted amount"
            );
            ensure!(
                balances_issuance == before.balances_issuance.saturating_add(expected_mint),
                "balances total issuance must rise by exactly the minted amount"
            );
            ensure!(
                HasMigrationRun::<T>::get(MIGRATION_NAME.to_vec()),
                "migrate_fix_root_pot_shortfall must mark itself as run"
            );
            Ok(())
        }
    }
}
