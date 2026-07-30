use super::*;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;
use share_pool::SafeFloat;
use sp_core::Get;
use sp_std::collections::btree_map::BTreeMap;
use subtensor_runtime_common::{MechId, NetUid, Token};

struct PreparedHotkeyStake<AccountId> {
    positions: Vec<(AccountId, NetUid, SafeFloat)>,
    coldkeys_by_netuid: BTreeMap<NetUid, Vec<AccountId>>,
}

impl<T: Config> Pallet<T> {
    /// Use the generated all-subnet stake-moving benchmark for every v2 path
    /// that scans and moves stake. Preserve the previous lightweight weight for
    /// the single-subnet `keep_stake` path, which does not scan stake prefixes.
    /// Root-touching swaps also reserve weight for migrating `BasketClaimed` rows.
    pub fn swap_hotkey_v2_dispatch_weight(
        old_hotkey: &T::AccountId,
        netuid: &Option<NetUid>,
        keep_stake: bool,
    ) -> Weight {
        let base = if netuid.is_none() || !keep_stake {
            <<T as crate::pallet::Config>::WeightInfo as crate::weights::WeightInfo>::swap_hotkey()
        } else {
            // +1 read / +4 writes vs the pre-lineage keep_stake path: root lookup,
            // LastHotkeySwap, successor clear+insert, root insert.
            Weight::from_parts(275_300_000, 0)
                .saturating_add(T::DbWeight::get().reads(53_u64))
                .saturating_add(T::DbWeight::get().writes(39_u64))
        };
        base.saturating_add(Self::basket_claimed_swap_weight(old_hotkey, netuid))
    }

    /// Pre-dispatch weight for moving `BasketClaimed` on a root-touching hotkey swap.
    /// Counts the prefix and charges per row so the block scheduler reserves the work up
    /// front (post-dispatch accrual alone does not expand the inclusion reservation).
    pub fn basket_claimed_swap_weight(
        old_hotkey: &T::AccountId,
        netuid: &Option<NetUid>,
    ) -> Weight {
        let touches_root = match netuid {
            None => true,
            Some(n) => *n == NetUid::ROOT,
        };
        if !touches_root {
            return Weight::zero();
        }
        let claimed_rows = BasketClaimed::<T>::iter_prefix(old_hotkey).count() as u64;
        // One read per scanned row + remove/insert pair per row.
        T::DbWeight::get().reads_writes(
            claimed_rows.saturating_mul(2).saturating_add(1),
            claimed_rows.saturating_mul(2),
        )
    }

    /// Read and merge the old hotkey's V1/V2 stake rows once. V2 keeps the
    /// existing precedence over a duplicate legacy row.
    fn prepare_hotkey_stake(old_hotkey: &T::AccountId) -> PreparedHotkeyStake<T::AccountId> {
        let positions: Vec<(T::AccountId, NetUid, SafeFloat)> =
            Self::alpha_iter_single_prefix(old_hotkey).collect();
        let mut coldkeys_by_netuid: BTreeMap<NetUid, Vec<T::AccountId>> = BTreeMap::new();

        for (coldkey, netuid, _) in &positions {
            coldkeys_by_netuid
                .entry(*netuid)
                .or_default()
                .push(coldkey.clone());
        }

        PreparedHotkeyStake {
            positions,
            coldkeys_by_netuid,
        }
    }

    /// Swaps the hotkey of a coldkey account.
    ///
    /// # Arguments
    ///
    /// * `origin`: The origin of the transaction, and also the coldkey account.
    /// * `old_hotkey`: The old hotkey to be swapped.
    /// * `new_hotkey`: The new hotkey to replace the old one.
    /// * `netuid`: The hotkey swap in a subnet or all subnets.
    /// * `keep_stake`: If `true`, stake remains on the old hotkey and the rest metadata
    ///
    /// # Returns
    ///
    /// * `DispatchResultWithPostInfo`: The result of the dispatch.
    ///
    /// # Errors
    ///
    /// * `NonAssociatedColdKey`: If the coldkey does not own the old hotkey.
    /// * `NewHotKeyIsSameWithOld`: If the new hotkey is the same as the old hotkey.
    /// * `HotKeyAlreadyRegisteredInSubNet`: If the new hotkey is already registered in the subnet.
    /// * `NewHotKeyNotCleanForRootSwap`: If the swap touches root and the new hotkey
    ///   has an outstanding basket fund (`BasketRate`/`BasketShares`), residual legacy
    ///   claim state, or non-zero root stake.
    /// * `NotEnoughBalanceToPaySwapHotKey`: If there is not enough balance to pay for the swap.
    pub fn do_swap_hotkey(
        origin: OriginFor<T>,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: Option<NetUid>,
        keep_stake: bool,
    ) -> DispatchResultWithPostInfo {
        // // 1. Ensure the origin is signed and get the coldkey
        let coldkey = ensure_signed(origin)?;

        if let Some(netuid) = netuid {
            ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        }

        // 2. Ensure the coldkey owns the old hotkey
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, old_hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // 3. Initialize the weight for this operation. The coldkey/old_hotkey
        // ownership check above reads Owner twice.
        let mut weight = T::DbWeight::get().reads(2);

        // 4. If the new hotkey already exists globally, ensure the coldkey owns it
        weight.saturating_accrue(T::DbWeight::get().reads(1));
        if Self::hotkey_account_exists(new_hotkey) {
            weight.saturating_accrue(T::DbWeight::get().reads(2));
            ensure!(
                Self::coldkey_owns_hotkey(&coldkey, new_hotkey),
                Error::<T>::NonAssociatedColdKey
            );
        } else {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
        }

        // 5. Ensure the new hotkey is different from the old one
        ensure!(old_hotkey != new_hotkey, Error::<T>::NewHotKeyIsSameWithOld);

        // 5.1 Bonded actors may rename (`keep_stake=false` migrates collateral
        // with the UID; lineage maps track the alias). `keep_stake` would leave
        // the lock on the old hotkey while the UID moves — reject that split.
        if keep_stake {
            ensure!(
                !Self::has_miner_collateral(old_hotkey, &coldkey, netuid),
                Error::<T>::KeepStakeBlockedByCollateral
            );
            weight.saturating_accrue(match netuid {
                Some(_) => T::DbWeight::get().reads(1),
                None => T::DbWeight::get()
                    .reads(Self::get_all_subnet_netuids().len().saturating_mul(1) as u64),
            });
        }

        // 6. Get the current block number
        let block: u64 = Self::get_current_block_as_u64();

        match netuid {
            // 8. Ensure the hotkey is not registered on the network before, if netuid is provided
            Some(netuid) => {
                ensure!(
                    !Self::is_hotkey_registered_on_specific_network(new_hotkey, netuid),
                    Error::<T>::HotKeyAlreadyRegisteredInSubNet
                );
            }
            // 8.1 Ensure the new hotkey is not already registered on any network, only if netuid is none
            None => {
                ensure!(
                    !Self::is_hotkey_registered_on_any_network(new_hotkey),
                    Error::<T>::HotKeyAlreadyRegisteredInSubNet
                );
            }
        }

        // 8.2 If the swap touches the root subnet, require that new_hotkey is clean
        // on root (no outstanding claimable rate and no existing root stake). Merging
        // a non-empty rate-book would either violate total conservation or misallocate
        // dividends across coldkeys that never staked on old_hotkey.
        let touches_root = match netuid {
            None => true,
            Some(n) => n == NetUid::ROOT,
        };
        if touches_root {
            // The multi-block seed keeps `RootClaimable`/`RootClaimed` and any
            // `HotkeyConvertState` keyed to the old hotkey until that hotkey finishes
            // converting. Moving root stake/basket mid-migration would value those
            // leftovers against zero stake under the retired key.
            Self::ensure_beta_basket_seed_idle()?;
            ensure!(
                !BasketRate::<T>::contains_key(new_hotkey)
                    && BasketShares::<T>::get(new_hotkey) == 0
                    && Self::get_stake_for_hotkey_on_subnet(new_hotkey, NetUid::ROOT).is_zero()
                    // Legacy claim state (drained by `migrate_seed_beta_basket`) must also be
                    // clean: a residual watermark would be seeded into the basket fund and
                    // inflate the merged claim total (GHSA-2026-010).
                    && RootClaimable::<T>::get(new_hotkey).is_empty()
                    && RootClaimed::<T>::iter_prefix((NetUid::ROOT, new_hotkey))
                        .next()
                        .is_none(),
                Error::<T>::NewHotKeyNotCleanForRootSwap
            );
        }

        // Read and group stake once before any hotkey-swap mutation. Execution
        // reuses this snapshot instead of rescanning both prefixes per subnet.
        let prepared_stake = if keep_stake {
            None
        } else {
            Some(Self::prepare_hotkey_stake(old_hotkey))
        };

        // Preflight collateral-index capacity before charging or writing so a
        // full-cap / unindexed legacy row cannot fail mid-swap. The mutation
        // body below is also transactional for any other fallible path.
        if !keep_stake {
            Self::ensure_hotkey_collateral_swappable(old_hotkey, new_hotkey, &coldkey, netuid)?;
            weight.saturating_accrue(match netuid {
                Some(_) => T::DbWeight::get().reads(2),
                None => T::DbWeight::get()
                    .reads(Self::get_all_subnet_netuids().len().saturating_mul(2) as u64),
            });
        }

        // All fee charges and storage mutations run in one storage transaction
        // so a late failure (including collateral index) rolls back ownership,
        // membership, UID, fee, and other writes together.
        with_transaction(|| {
            let result = (|| -> DispatchResultWithPostInfo {
                // Swap LastTxBlockDelegateTake / ChildKeyTake.
                let last_tx_block_delegate_take: u64 =
                    Self::get_last_tx_block_delegate_take(old_hotkey);
                Self::set_last_tx_block_delegate_take(new_hotkey, last_tx_block_delegate_take);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

                let last_tx_block_child_key_take: u64 =
                    Self::get_last_tx_block_childkey_take(old_hotkey);
                Self::set_last_tx_block_childkey(new_hotkey, last_tx_block_child_key_take);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

                // Per-subnet path after common checks.
                if let Some(netuid) = netuid {
                    return Self::swap_hotkey_on_subnet(
                        &coldkey,
                        old_hotkey,
                        new_hotkey,
                        netuid,
                        weight,
                        keep_stake,
                        prepared_stake.as_ref(),
                    );
                }

                // All-subnets path: enforce per-subnet hotkey-swap cooldown.
                // The all-subnets swap moves the identity on every subnet the old
                // hotkey actively participates in, so it must respect (and record)
                // `LastHotkeySwapOnNetuid` for each of those subnets.
                //
                // "Participates in" is membership OR being a parent (has childkeys).
                // Residual collateral is NOT cooldown-gated — it only needs a
                // lineage record after the swap.
                //
                // We deliberately do NOT gate on the *child* side (`ParentKeys`):
                // being someone's child is set by the parent via `do_set_children`
                // WITHOUT the child's consent, so a third party could otherwise add
                // a victim's hotkey as a child on an arbitrary subnet and impose
                // swap-cooldowns on it — a griefing vector.
                let hotkey_swap_interval = T::HotkeySwapOnSubnetInterval::get();
                let all_netuids = Self::get_all_subnet_netuids();
                weight.saturating_accrue(
                    T::DbWeight::get().reads(
                        (all_netuids.len() as u64)
                            .saturating_mul(2)
                            .saturating_add(1),
                    ),
                );
                let mut cooldown_netuids: Vec<NetUid> = Vec::new();
                let mut residual_collateral_netuids: Vec<NetUid> = Vec::new();
                for netuid in all_netuids {
                    let in_cooldown = IsNetworkMember::<T>::get(old_hotkey, netuid)
                        || !ChildKeys::<T>::get(old_hotkey, netuid).is_empty();
                    if in_cooldown {
                        cooldown_netuids.push(netuid);
                    } else {
                        weight.saturating_accrue(T::DbWeight::get().reads(1));
                        if MinerCollateral::<T>::contains_key((netuid, old_hotkey, &coldkey)) {
                            residual_collateral_netuids.push(netuid);
                        }
                    }
                }
                for netuid in cooldown_netuids.iter() {
                    let last_hotkey_swap_block =
                        LastHotkeySwapOnNetuid::<T>::get(*netuid, &coldkey);
                    // Only enforce the cooldown when a prior swap was recorded.
                    // A first swap (no recorded timestamp) must not be gated by
                    // chain age.
                    if last_hotkey_swap_block != 0 {
                        ensure!(
                            last_hotkey_swap_block.saturating_add(hotkey_swap_interval) < block,
                            Error::<T>::HotKeySwapOnSubnetIntervalNotPassed
                        );
                    }
                    weight.saturating_accrue(T::DbWeight::get().reads(1));
                }

                let swap_cost = Self::get_key_swap_cost();
                log::debug!("Swap cost: {swap_cost:?}");
                ensure!(
                    Self::can_remove_balance_from_coldkey_account(&coldkey, swap_cost.into()),
                    Error::<T>::NotEnoughBalanceToPaySwapHotKey
                );
                weight.saturating_accrue(T::DbWeight::get().reads_writes(3, 0));

                Self::recycle_tao(&coldkey, swap_cost.into())?;
                weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 2));

                Self::perform_hotkey_swap_on_all_subnets_prepared(
                    old_hotkey,
                    new_hotkey,
                    &coldkey,
                    &mut weight,
                    keep_stake,
                    prepared_stake.as_ref(),
                )?;

                for netuid in cooldown_netuids.iter() {
                    Self::record_hotkey_swap_on_netuid(
                        *netuid,
                        &coldkey,
                        old_hotkey,
                        new_hotkey,
                        block,
                        &mut weight,
                    );
                }
                for netuid in residual_collateral_netuids.iter() {
                    Self::record_hotkey_swap_lineage(*netuid, old_hotkey, new_hotkey);
                    weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 3));
                }

                Self::deposit_event(Event::HotkeySwapped {
                    coldkey,
                    old_hotkey: old_hotkey.clone(),
                    new_hotkey: new_hotkey.clone(),
                });

                // Stake-moving paths retain the generated pre-dispatch benchmark weight.
                // `keep_stake` does not inspect stake prefixes and may keep its dynamic refund.
                if keep_stake {
                    Ok(Some(weight).into())
                } else {
                    Ok(None.into())
                }
            })();

            match result {
                Ok(info) => TransactionOutcome::Commit(Ok(info)),
                Err(e) => TransactionOutcome::Rollback(Err(e)),
            }
        })
    }

    /// Performs the hotkey swap operation, transferring all associated data and state from the old hotkey to the new hotkey.
    ///
    /// This function executes a series of steps to ensure a complete transfer of all relevant information:
    /// 1. Swaps the owner of the hotkey.
    /// 2. Updates the list of owned hotkeys for the coldkey.
    /// 3. Transfers the total hotkey stake.
    /// 4. Moves all stake-related data for the interval.
    /// 5. Updates the last transaction block for the new hotkey.
    /// 6. Transfers the delegate take information.
    /// 7. Updates delegate information.
    /// 8. For each subnet:
    ///    - Updates network membership status.
    ///    - Transfers UID and key information.
    ///    - Moves Prometheus data.
    ///    - Updates axon information.
    ///    - Transfers weight commits.
    ///    - Updates loaded emission data.
    /// 9. Transfers all stake information, including updating staking hotkeys for each coldkey.
    ///
    /// Throughout the process, the function accumulates the computational weight of operations performed.
    ///
    /// # Arguments
    /// * `old_hotkey`: The AccountId of the current hotkey to be replaced.
    /// * `new_hotkey`: The AccountId of the new hotkey to replace the old one.
    /// * `coldkey`: The AccountId of the coldkey that owns both hotkeys.
    /// * `weight`: A mutable reference to the Weight, updated as operations are performed.
    ///
    /// # Returns
    /// * `DispatchResult`: Ok(()) if the swap was successful, or an error if any operation failed.
    ///
    /// # Note
    /// This function performs extensive storage reads and writes, which can be computationally expensive.
    /// The accumulated weight should be carefully considered in the context of block limits.
    pub fn perform_hotkey_swap_on_all_subnets(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        weight: &mut Weight,
        keep_stake: bool,
    ) -> DispatchResult {
        let prepared_stake = if keep_stake {
            None
        } else {
            Some(Self::prepare_hotkey_stake(old_hotkey))
        };

        Self::perform_hotkey_swap_on_all_subnets_prepared(
            old_hotkey,
            new_hotkey,
            coldkey,
            weight,
            keep_stake,
            prepared_stake.as_ref(),
        )
    }

    fn perform_hotkey_swap_on_all_subnets_prepared(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        weight: &mut Weight,
        keep_stake: bool,
        prepared_stake: Option<&PreparedHotkeyStake<T::AccountId>>,
    ) -> DispatchResult {
        // 2. Swap the stake locks
        let (reads, writes) = Self::swap_hotkey_locks(old_hotkey, new_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(reads, writes));

        // 3. Swap owner.
        // Owner( hotkey ) -> coldkey -- the coldkey that owns the hotkey.
        Owner::<T>::remove(old_hotkey);
        Self::set_hotkey_owner(coldkey, new_hotkey)?;
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 4. Swap OwnedHotkeys.
        // OwnedHotkeys( coldkey ) -> Vec<hotkey> -- the hotkeys that the coldkey owns.
        let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
        // Add the new key if needed.
        if !hotkeys.contains(new_hotkey) {
            hotkeys.push(new_hotkey.clone());
        }

        // 5. Remove the old key.
        hotkeys.retain(|hk| *hk != *old_hotkey);
        OwnedHotkeys::<T>::insert(coldkey, hotkeys);

        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 6. execute the hotkey swap on all subnets
        for netuid in Self::get_all_subnet_netuids() {
            let stake_coldkeys = prepared_stake
                .and_then(|prepared| prepared.coldkeys_by_netuid.get(&netuid))
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            Self::perform_hotkey_swap_on_one_subnet_prepared(
                old_hotkey,
                new_hotkey,
                weight,
                netuid,
                keep_stake,
                stake_coldkeys,
            )?;
        }

        // 7. Swap LastTxBlock
        // LastTxBlock( hotkey ) --> u64 -- the last transaction block for the hotkey.
        Self::remove_last_tx_block(old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 8. Swap LastTxBlockDelegateTake
        // LastTxBlockDelegateTake( hotkey ) --> u64 -- the last transaction block for the hotkey delegate take.
        Self::remove_last_tx_block_delegate_take(old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 9. Swap LastTxBlockChildKeyTake
        // LastTxBlockChildKeyTake( hotkey ) --> u64 -- the last transaction block for the hotkey child key take.
        Self::remove_last_tx_block_childkey(old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 10. Swap delegates.
        // Delegates( hotkey ) -> take value -- the hotkey delegate take value.
        if Delegates::<T>::contains_key(old_hotkey) {
            let old_delegate_take = Delegates::<T>::get(old_hotkey);
            Delegates::<T>::remove(old_hotkey);
            Delegates::<T>::insert(new_hotkey, old_delegate_take);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));
        }

        // 11. Alphas already update in perform_hotkey_swap_on_one_subnet
        // Update the StakingHotkeys for the case where hotkey staked by multiple coldkeys.
        if let Some(prepared_stake) = prepared_stake {
            for (coldkey, _netuid, alpha_share) in &prepared_stake.positions {
                // Swap StakingHotkeys.
                // StakingHotkeys( coldkey ) --> Vec<hotkey> -- the hotkeys that the coldkey stakes.
                if !alpha_share.is_zero() {
                    let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
                    weight.saturating_accrue(T::DbWeight::get().reads(1));
                    if staking_hotkeys.contains(old_hotkey) {
                        staking_hotkeys.retain(|hk| *hk != *old_hotkey && *hk != *new_hotkey);
                        if !staking_hotkeys.contains(new_hotkey) {
                            staking_hotkeys.push(new_hotkey.clone());
                        }
                        StakingHotkeys::<T>::insert(coldkey, staking_hotkeys);
                        weight.saturating_accrue(T::DbWeight::get().writes(1));
                    }
                }
            }
        }

        // Return successful after swapping all the relevant terms.
        Ok(())
    }

    #[allow(unused)]
    fn swap_hotkey_on_subnet(
        coldkey: &T::AccountId,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
        init_weight: Weight,
        keep_stake: bool,
        prepared_stake: Option<&PreparedHotkeyStake<T::AccountId>>,
    ) -> DispatchResultWithPostInfo {
        // 1. Ensure coldkey not swap hotkey too frequently on this subnet.
        // Mirror the all-subnets path: only enforce when a prior swap was recorded.
        // A first swap (default 0) must not be gated by chain age / interval size —
        // otherwise young clones and fresh coldkeys fail with
        // `HotKeySwapOnSubnetIntervalNotPassed` whenever `interval >= block`.
        let mut weight: Weight = init_weight;
        let block: u64 = Self::get_current_block_as_u64();
        let hotkey_swap_interval = T::HotkeySwapOnSubnetInterval::get();
        let last_hotkey_swap_block = LastHotkeySwapOnNetuid::<T>::get(netuid, coldkey);

        if last_hotkey_swap_block != 0 {
            ensure!(
                last_hotkey_swap_block.saturating_add(hotkey_swap_interval) < block,
                Error::<T>::HotKeySwapOnSubnetIntervalNotPassed
            );
        }
        weight.saturating_accrue(T::DbWeight::get().reads_writes(3, 0));

        // Check that new hotkey is a non-system hotkey
        ensure!(
            Self::is_subnet_account_id(new_hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );

        // 2. Ensure the hotkey not registered on the network before.
        ensure!(
            !Self::is_hotkey_registered_on_specific_network(new_hotkey, netuid),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );

        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));

        // 3. Get the cost for swapping the key on the subnet
        let swap_cost = T::KeySwapOnSubnetCost::get();
        log::debug!("Swap cost in subnet {netuid:?}: {swap_cost:?}");
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));

        // 4. Ensure the coldkey has enough balance to pay for the swap
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, swap_cost),
            Error::<T>::NotEnoughBalanceToPaySwapHotKey
        );

        // 5. Remove the swap cost from the coldkey's account + Recycle the tokens
        Self::recycle_tao(coldkey, swap_cost)?;
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 7. Swap owner.
        // Owner( hotkey ) -> coldkey -- the coldkey that owns the hotkey.
        // Owner::<T>::remove(old_hotkey);
        Owner::<T>::insert(new_hotkey, coldkey.clone());
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));

        // 8. Swap OwnedHotkeys.
        // OwnedHotkeys( coldkey ) -> Vec<hotkey> -- the hotkeys that the coldkey owns.
        let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
        // Add the new key if needed.
        if !hotkeys.contains(new_hotkey) {
            hotkeys.push(new_hotkey.clone());
            OwnedHotkeys::<T>::insert(coldkey, hotkeys);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
        }

        // 9. Swap the stake locks
        let (reads, writes) = Self::swap_hotkey_locks_on_subnet(old_hotkey, new_hotkey, netuid);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(reads, writes));

        // 10. Perform the hotkey swap using the preflight snapshot.
        let stake_coldkeys = prepared_stake
            .and_then(|prepared| prepared.coldkeys_by_netuid.get(&netuid))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        Self::perform_hotkey_swap_on_one_subnet_prepared(
            old_hotkey,
            new_hotkey,
            &mut weight,
            netuid,
            keep_stake,
            stake_coldkeys,
        )?;

        // 10. Record cooldown + lineage for the HotkeySwapOnSubnetInterval gate.
        Self::record_hotkey_swap_on_netuid(
            netuid,
            coldkey,
            old_hotkey,
            new_hotkey,
            block,
            &mut weight,
        );

        // 12. Emit an event for the hotkey swap
        Self::deposit_event(Event::HotkeySwappedOnSubnet {
            coldkey: coldkey.clone(),
            old_hotkey: old_hotkey.clone(),
            new_hotkey: new_hotkey.clone(),
            netuid,
        });

        if keep_stake {
            Ok(Some(weight).into())
        } else {
            Ok(None.into())
        }
    }

    // do hotkey swap public part for both swap all subnets and just swap one subnet
    pub fn perform_hotkey_swap_on_one_subnet(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        weight: &mut Weight,
        netuid: NetUid,
        keep_stake: bool,
    ) -> DispatchResult {
        let prepared_stake = if keep_stake {
            None
        } else {
            Some(Self::prepare_hotkey_stake(old_hotkey))
        };
        let stake_coldkeys = prepared_stake
            .as_ref()
            .and_then(|prepared| prepared.coldkeys_by_netuid.get(&netuid))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        Self::perform_hotkey_swap_on_one_subnet_prepared(
            old_hotkey,
            new_hotkey,
            weight,
            netuid,
            keep_stake,
            stake_coldkeys,
        )
    }

    fn perform_hotkey_swap_on_one_subnet_prepared(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        weight: &mut Weight,
        netuid: NetUid,
        keep_stake: bool,
        stake_coldkeys: &[T::AccountId],
    ) -> DispatchResult {
        // 3. Swap all subnet specific info.

        // 3.1 Remove the previous hotkey and insert the new hotkey from membership.
        // IsNetworkMember( hotkey, netuid ) -> bool -- is the hotkey a subnet member.
        let is_network_member: bool = IsNetworkMember::<T>::get(old_hotkey, netuid);
        IsNetworkMember::<T>::remove(old_hotkey, netuid);
        IsNetworkMember::<T>::insert(new_hotkey, netuid, is_network_member);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

        // 3.2 Swap Uids + Keys.
        // Keys( netuid, hotkey ) -> uid -- the uid the hotkey has in the network if it is a member.
        // Uids( netuid, hotkey ) -> uid -- the uids that the hotkey has.
        if is_network_member {
            // 3.2.1 Swap the UIDS
            if let Ok(old_uid) = Uids::<T>::try_get(netuid, old_hotkey) {
                Uids::<T>::remove(netuid, old_hotkey);
                Uids::<T>::insert(netuid, new_hotkey, old_uid);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));

                // 3.2.2 Swap the keys.
                Keys::<T>::insert(netuid, old_uid, new_hotkey.clone());
                weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 1));
            }
        }

        // 3.3 Swap Prometheus.
        // Prometheus( netuid, hotkey ) -> prometheus -- the prometheus data that a hotkey has in the network.
        if is_network_member
            && let Ok(old_prometheus_info) = Prometheus::<T>::try_get(netuid, old_hotkey)
        {
            Prometheus::<T>::remove(netuid, old_hotkey);
            Prometheus::<T>::insert(netuid, new_hotkey, old_prometheus_info);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.4. Swap axons.
        // Axons( netuid, hotkey ) -> axon -- the axon that the hotkey has.
        if is_network_member && let Ok(old_axon_info) = Axons::<T>::try_get(netuid, old_hotkey) {
            Axons::<T>::remove(netuid, old_hotkey);
            Axons::<T>::insert(netuid, new_hotkey, old_axon_info);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.5 Swap WeightCommits
        // WeightCommits( hotkey ) --> Vec<u64> -- the weight commits for the hotkey.
        if is_network_member {
            for mecid in 0..MechanismCountCurrent::<T>::get(netuid).into() {
                let netuid_index = Self::get_mechanism_storage_index(netuid, MechId::from(mecid));
                if let Ok(old_weight_commits) =
                    WeightCommits::<T>::try_get(netuid_index, old_hotkey)
                {
                    WeightCommits::<T>::remove(netuid_index, old_hotkey);
                    WeightCommits::<T>::insert(netuid_index, new_hotkey, old_weight_commits);
                    weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
                }
            }
        }

        // 3.6. Swap the subnet loaded emission.
        // LoadedEmission( netuid ) --> Vec<(hotkey, u64)> -- the loaded emission for the subnet.
        if is_network_member && let Some(mut old_loaded_emission) = LoadedEmission::<T>::get(netuid)
        {
            for emission in old_loaded_emission.iter_mut() {
                if emission.0 == *old_hotkey {
                    emission.0 = new_hotkey.clone();
                }
            }
            LoadedEmission::<T>::remove(netuid);
            LoadedEmission::<T>::insert(netuid, old_loaded_emission);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.7. Swap neuron TLS certificates.
        // NeuronCertificates( netuid, hotkey ) -> Vec<u8> -- the neuron certificate for the hotkey.
        if is_network_member
            && let Ok(old_neuron_certificates) =
                NeuronCertificates::<T>::try_get(netuid, old_hotkey)
        {
            NeuronCertificates::<T>::remove(netuid, old_hotkey);
            NeuronCertificates::<T>::insert(netuid, new_hotkey, old_neuron_certificates);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.8. Swap ChildkeyTake.
        // ChildkeyTake( hotkey, netuid ) --> u16 -- the per-subnet childkey take for the hotkey.
        // Only migrate when an explicit value exists, to preserve the storage default
        // semantics (don't materialize a floor value for hotkeys that never set a take).
        // `take` reads + removes the old row in one operation, avoiding orphaned storage.
        if ChildkeyTake::<T>::contains_key(old_hotkey, netuid) {
            let childkey_take = ChildkeyTake::<T>::take(old_hotkey, netuid);
            ChildkeyTake::<T>::insert(new_hotkey, netuid, childkey_take);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        }

        // 3.9. Swap miner registration collateral (no-op when none).
        // `keep_stake` with standing collateral is rejected earlier; when
        // `keep_stake` is false, the bond follows the UID. Ownership may
        // already have moved off `old_hotkey`, so read the coldkey from the
        // new hotkey.
        if !keep_stake {
            let owner = Owner::<T>::get(new_hotkey);
            Self::swap_miner_collateral(old_hotkey, new_hotkey, &owner, netuid)?;
            weight.saturating_accrue(T::DbWeight::get().reads_writes(5, 4));
        }

        // 4. Swap ChildKeys.
        // 5. Swap ParentKeys.
        // 6. Swap PendingChildKeys.
        Self::parent_child_swap_hotkey(old_hotkey, new_hotkey, netuid, weight)?;

        // Also check for others with our hotkey as a child
        for (hotkey, (children, cool_down_block)) in PendingChildKeys::<T>::iter_prefix(netuid) {
            weight.saturating_accrue(T::DbWeight::get().reads(1));

            if let Some(potential_idx) =
                children.iter().position(|(_, child)| *child == *old_hotkey)
            {
                let mut new_children = children.clone();
                let entry_to_remove = new_children.remove(potential_idx);
                new_children.push((entry_to_remove.0, new_hotkey.clone())); // Keep the proportion.

                PendingChildKeys::<T>::remove(netuid, hotkey.clone());
                PendingChildKeys::<T>::insert(netuid, hotkey, (new_children, cool_down_block));
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
            }
        }

        // 6.4 Swap AutoStakeDestination
        if let Ok(old_auto_stake_coldkeys) =
            AutoStakeDestinationColdkeys::<T>::try_get(old_hotkey, netuid)
        {
            // Move the vector from old hotkey to new hotkey.
            for coldkey in &old_auto_stake_coldkeys {
                AutoStakeDestination::<T>::insert(coldkey, netuid, new_hotkey);
            }
            AutoStakeDestinationColdkeys::<T>::remove(old_hotkey, netuid);
            AutoStakeDestinationColdkeys::<T>::insert(new_hotkey, netuid, old_auto_stake_coldkeys);
        }

        // 7. Swap SubnetOwnerHotkey
        // SubnetOwnerHotkey( netuid ) --> hotkey -- the hotkey that is the owner of the subnet.
        if let Ok(old_subnet_owner_hotkey) = SubnetOwnerHotkey::<T>::try_get(netuid) {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
            if old_subnet_owner_hotkey == *old_hotkey {
                Self::set_subnet_owner_hotkey(netuid, new_hotkey)?;
                weight.saturating_accrue(T::DbWeight::get().writes(1));
            }
        }

        // 8. Swap dividend records
        if !keep_stake {
            // 8.1 Swap TotalHotkeyAlphaLastEpoch
            let old_alpha = TotalHotkeyAlphaLastEpoch::<T>::take(old_hotkey, netuid);
            let new_total_hotkey_alpha = TotalHotkeyAlphaLastEpoch::<T>::get(new_hotkey, netuid);
            TotalHotkeyAlphaLastEpoch::<T>::insert(
                new_hotkey,
                netuid,
                old_alpha.saturating_add(new_total_hotkey_alpha),
            );
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

            // 8.2 Swap AlphaDividendsPerSubnet
            let old_hotkey_alpha_dividends = AlphaDividendsPerSubnet::<T>::get(netuid, old_hotkey);
            let new_hotkey_alpha_dividends = AlphaDividendsPerSubnet::<T>::get(netuid, new_hotkey);
            AlphaDividendsPerSubnet::<T>::remove(netuid, old_hotkey);
            AlphaDividendsPerSubnet::<T>::insert(
                netuid,
                new_hotkey,
                old_hotkey_alpha_dividends.saturating_add(new_hotkey_alpha_dividends),
            );
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

            // 8.3 Swap TaoDividendsPerSubnet
            // Tao dividends were removed

            // 8.4 Swap VotingPower
            // VotingPower( netuid, hotkey ) --> u64 -- the voting power EMA for the hotkey.
            Self::swap_voting_power_for_hotkey(old_hotkey, new_hotkey, netuid);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

            // The beta escrow's basket positions are tied to the validator's root identity, not
            // to per-subnet membership. Skip it here and migrate baskets atomically in the root
            // branch below, so a non-root single-subnet swap never moves a basket out from under
            // its (unchanged) root accounting.
            let beta_escrow = Self::get_beta_escrow_account_id();

            // Move only the positions prepared for this subnet. The V1/V2
            // prefixes were already scanned and deduplicated once during preflight.
            for coldkey in stake_coldkeys {
                if *coldkey == beta_escrow {
                    continue;
                }
                let alpha_old =
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(old_hotkey, coldkey, netuid);
                Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                    old_hotkey, coldkey, netuid, alpha_old,
                );
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    new_hotkey, coldkey, netuid, alpha_old,
                );
                weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));

                let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
                weight.saturating_accrue(T::DbWeight::get().reads(1));

                if staking_hotkeys.contains(old_hotkey) && !staking_hotkeys.contains(new_hotkey) {
                    staking_hotkeys.push(new_hotkey.clone());
                    StakingHotkeys::<T>::insert(coldkey, staking_hotkeys);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                }
            }

            if netuid == NetUid::ROOT {
                // 9. Migrate the validator's whole basket fund only for the root subnet: shares,
                // rate, per-coldkey claimed watermarks, and every escrow holding move by value.
                // The clean-hotkey guard above makes this a move, not a merge. Claimant rows are
                // unbounded (like stake coldkeys); charge weight for each watermark moved.
                let num_holdings = Self::get_basket_holdings(old_hotkey).len() as u64;
                let claimed_count =
                    Self::transfer_basket_for_new_hotkey(old_hotkey, new_hotkey) as u64;
                let ops = num_holdings
                    .saturating_mul(2)
                    .saturating_add(4)
                    .saturating_add(claimed_count.saturating_mul(2));
                weight.saturating_accrue(T::DbWeight::get().reads_writes(ops, ops));

                // Transfer AutoParentDelegationEnabled flag from old_hotkey to new_hotkey.
                // Only migrate if it was explicitly set, to preserve the storage default semantics.
                if AutoParentDelegationEnabled::<T>::contains_key(old_hotkey) {
                    let enabled = AutoParentDelegationEnabled::<T>::take(old_hotkey);
                    AutoParentDelegationEnabled::<T>::insert(new_hotkey, enabled);
                    weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
                }
            }
        }

        Ok(())
    }
}
