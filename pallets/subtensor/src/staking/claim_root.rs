use super::*;
use frame_support::dispatch::DispatchResult;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_std::collections::btree_set::BTreeSet;
use substrate_fixed::types::I96F32;
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
    pub fn block_hash_to_indices(block_hash: T::Hash, k: u64, n: u64) -> Vec<u64> {
        let block_hash_bytes = block_hash.as_ref();
        let mut indices: BTreeSet<u64> = BTreeSet::new();
        // k < n
        let start_index: u64 = u64::from_be_bytes(
            block_hash_bytes
                .get(0..8)
                .unwrap_or(&[0; 8])
                .try_into()
                .unwrap_or([0; 8]),
        );
        let mut last_idx = start_index;
        for i in 0..k {
            let bh_idx: usize = ((i.saturating_mul(8)) % 32) as usize;
            let idx_step = u64::from_be_bytes(
                block_hash_bytes
                    .get(bh_idx..(bh_idx.saturating_add(8)))
                    .unwrap_or(&[0; 8])
                    .try_into()
                    .unwrap_or([0; 8]),
            );
            let idx = last_idx
                .saturating_add(idx_step)
                .checked_rem(n)
                .unwrap_or(0);
            indices.insert(idx);
            last_idx = idx;
        }
        indices.into_iter().collect()
    }

    pub fn increase_root_claimable_for_hotkey_and_subnet(
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) {
        // Get total stake on this hotkey on root.
        let total: I96F32 =
            I96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT));

        // Get increment
        let increment: I96F32 = I96F32::saturating_from_num(amount)
            .checked_div(total)
            .unwrap_or(I96F32::saturating_from_num(0.0));

        // Unlikely to happen. This is mostly for test environment sanity checks.
        if u64::from(amount) > total.saturating_to_num::<u64>() {
            log::warn!("Not enough root stake. NetUID = {netuid}");

            let owner = Owner::<T>::get(hotkey);
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &owner, netuid, amount);
            return;
        }

        // Increment claimable for this subnet.
        RootClaimable::<T>::mutate(hotkey, |claimable| {
            claimable
                .entry(netuid)
                .and_modify(|claim_total| *claim_total = claim_total.saturating_add(increment))
                .or_insert(increment);
        });
    }

    pub fn get_root_claimable_for_hotkey_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> I96F32 {
        // Get this keys stake balance on root.
        let root_stake: I96F32 = I96F32::saturating_from_num(
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT),
        );

        // Get the total claimable_rate for this hotkey and this network
        let claimable_rate: I96F32 = *RootClaimable::<T>::get(hotkey)
            .get(&netuid)
            .unwrap_or(&I96F32::from(0));

        // Compute the proportion owed to this coldkey via balance.
        let claimable: I96F32 = claimable_rate.saturating_mul(root_stake);

        claimable
    }

    pub fn get_root_owed_for_hotkey_coldkey_float(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> I96F32 {
        let claimable = Self::get_root_claimable_for_hotkey_coldkey(hotkey, coldkey, netuid);

        // Attain the root claimed to avoid overclaiming.
        let root_claimed: I96F32 =
            I96F32::saturating_from_num(RootClaimed::<T>::get((netuid, hotkey, coldkey)));

        // Subtract the already claimed alpha.
        let owed: I96F32 = claimable.saturating_sub(root_claimed);

        owed
    }

    pub fn get_root_owed_for_hotkey_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> u64 {
        let owed = Self::get_root_owed_for_hotkey_coldkey_float(hotkey, coldkey, netuid);

        // Convert owed to u64, mapping negative values to 0
        let owed_u64: u64 = if owed.is_negative() {
            0
        } else {
            owed.saturating_to_num::<u64>()
        };

        owed_u64
    }

    pub fn root_claim_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
        root_claim_type: RootClaimTypeEnum,
        ignore_minimum_condition: bool,
    ) -> DispatchResult {
        // Subtract the root claimed.
        let owed: I96F32 = Self::get_root_owed_for_hotkey_coldkey_float(hotkey, coldkey, netuid);

        if !ignore_minimum_condition
            && owed < I96F32::saturating_from_num(RootClaimableThreshold::<T>::get(&netuid))
        {
            log::debug!(
                "root claim on subnet {netuid} is skipped: {owed:?} for h={hotkey:?},c={coldkey:?} "
            );
            return Ok(()); // no-op
        }

        // Convert owed to u64, mapping negative values to 0
        let owed_u64: u64 = if owed.is_negative() {
            0
        } else {
            owed.saturating_to_num::<u64>()
        };

        if owed_u64 == 0 {
            log::debug!(
                "root claim on subnet {netuid} is skipped: {owed:?} for h={hotkey:?},c={coldkey:?}"
            );
            return Ok(()); // no-op
        }

        let swap = match root_claim_type {
            RootClaimTypeEnum::Swap => true,
            RootClaimTypeEnum::Keep => false,
            RootClaimTypeEnum::KeepSubnets { subnets } => !subnets.contains(&netuid),
        };

        if swap {
            with_transaction(|| {
                // Increase stake on root. Swap the alpha owed to TAO.
                let owed_tao = match Self::swap_alpha_for_tao(
                    netuid,
                    owed_u64.into(),
                    T::SwapInterface::min_price::<TaoBalance>(),
                    true,
                ) {
                    Ok(owed_tao) => owed_tao,
                    Err(err) => {
                        log::error!("Error swapping alpha for TAO: {err:?}");

                        return TransactionOutcome::Rollback(Err(err));
                    }
                };

                let root_subnet_account_id = match Self::get_subnet_account_id(NetUid::ROOT) {
                    Some(account_id) => account_id,
                    None => {
                        return TransactionOutcome::Rollback(Err(
                            Error::<T>::RootNetworkDoesNotExist.into(),
                        ));
                    }
                };

                if let Err(err) = Self::transfer_tao_from_subnet(
                    netuid,
                    &root_subnet_account_id,
                    owed_tao.amount_paid_out.into(),
                ) {
                    log::error!("Error transferring root claim TAO from subnet: {err:?}");

                    return TransactionOutcome::Rollback(Err(err));
                }

                // Record root sell as protocol outflow (reduces protocol cost).
                let root_sell_tao: TaoBalance = owed_tao.amount_paid_out;
                SubnetRootSellTao::<T>::mutate(netuid, |total| {
                    *total = total.saturating_add(root_sell_tao);
                });
                Self::record_protocol_outflow(netuid, root_sell_tao);

                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    coldkey,
                    NetUid::ROOT,
                    owed_tao.amount_paid_out.to_u64().into(),
                );

                // Increase root subnet SubnetTAO
                SubnetTAO::<T>::mutate(NetUid::ROOT, |total| {
                    *total = total.saturating_add(owed_tao.amount_paid_out.into());
                });

                // Increase root SubnetAlphaOut
                SubnetAlphaOut::<T>::mutate(NetUid::ROOT, |total| {
                    *total = total.saturating_add(u64::from(owed_tao.amount_paid_out).into());
                });

                // Increase Total Stake
                TotalStake::<T>::mutate(|total| {
                    *total = total.saturating_add(owed_tao.amount_paid_out.into());
                });

                Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                    hotkey,
                    coldkey,
                    owed_tao.amount_paid_out.into(),
                );

                TransactionOutcome::Commit(Ok(()))
            })?;
        } else
        /* Keep */
        {
            // Increase the stake with the alpha owned
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                coldkey,
                netuid,
                owed_u64.into(),
            );
        }

        // Increase root claimed by owed amount.
        RootClaimed::<T>::mutate((netuid, hotkey, coldkey), |root_claimed| {
            *root_claimed = root_claimed.saturating_add(owed_u64.into());
        });

        Ok(())
    }

    fn root_claim_on_subnet_weight(_root_claim_type: RootClaimTypeEnum) -> Weight {
        Weight::from_parts(60_000_000, 6987)
            .saturating_add(T::DbWeight::get().reads(7_u64))
            .saturating_add(T::DbWeight::get().writes(5_u64))
    }
    pub fn root_claim_all(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        subnets: Option<BTreeSet<NetUid>>,
    ) -> Result<Weight, DispatchError> {
        let mut weight = Weight::default();

        let root_claim_type = RootClaimType::<T>::get(coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        // Iterate over all the subnets this hotkey has claimable for root.
        let root_claimable = RootClaimable::<T>::get(hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        for (netuid, _) in root_claimable.iter() {
            let skip = subnets
                .as_ref()
                .map(|subnets| !subnets.contains(netuid))
                .unwrap_or(false);

            if skip {
                continue;
            }

            Self::root_claim_on_subnet(hotkey, coldkey, *netuid, root_claim_type.clone(), false)?;
            weight.saturating_accrue(Self::root_claim_on_subnet_weight(root_claim_type.clone()));
        }

        Ok(weight)
    }

    pub fn add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        amount: u64,
    ) {
        // Iterate over all the subnets this hotkey is staked on for root.
        let root_claimable = RootClaimable::<T>::get(hotkey);
        for (netuid, claimable_rate) in root_claimable.iter() {
            // Get current staker root claimed value.
            let root_claimed: u128 = RootClaimed::<T>::get((netuid, hotkey, coldkey));

            // Increase root claimed based on the claimable rate.
            let new_root_claimed = root_claimed.saturating_add(
                claimable_rate
                    .saturating_mul(I96F32::from(u64::from(amount)))
                    .saturating_to_num(),
            );

            // Set the new root claimed value.
            RootClaimed::<T>::insert((netuid, hotkey, coldkey), new_root_claimed);
        }
    }

    pub fn remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        amount: AlphaBalance,
    ) {
        // Iterate over all the subnets this hotkey is staked on for root.
        let root_claimable = RootClaimable::<T>::get(hotkey);
        for (netuid, claimable_rate) in root_claimable.iter() {
            if *netuid == NetUid::ROOT.into() {
                continue; // Skip the root netuid.
            }

            // Get current staker root claimed value.
            let root_claimed: u128 = RootClaimed::<T>::get((netuid, hotkey, coldkey));

            // Decrease root claimed based on the claimable rate.
            let new_root_claimed = root_claimed.saturating_sub(
                claimable_rate
                    .saturating_mul(I96F32::from(u64::from(amount)))
                    .saturating_to_num(),
            );

            // Set the new root_claimed value.
            RootClaimed::<T>::insert((netuid, hotkey, coldkey), new_root_claimed);
        }
    }

    pub fn do_root_claim(
        coldkey: T::AccountId,
        subnets: Option<BTreeSet<NetUid>>,
    ) -> Result<Weight, DispatchError> {
        with_transaction(|| match Self::try_do_root_claim(coldkey, subnets) {
            Ok(weight) => TransactionOutcome::Commit(Ok(weight)),
            Err(err) => TransactionOutcome::Rollback(Err(err)),
        })
    }

    fn try_do_root_claim(
        coldkey: T::AccountId,
        subnets: Option<BTreeSet<NetUid>>,
    ) -> Result<Weight, DispatchError> {
        let mut weight = Weight::default();

        let hotkeys = StakingHotkeys::<T>::get(&coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        for hotkey in hotkeys.iter() {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
            weight.saturating_accrue(Self::root_claim_all(hotkey, &coldkey, subnets.clone())?);
        }

        Self::deposit_event(Event::RootClaimed { coldkey });

        Ok(weight)
    }

    fn block_hash_to_indices_weight(k: u64, _n: u64) -> Weight {
        Weight::from_parts(3_000_000, 1517)
            .saturating_add(Weight::from_parts(100_412, 0).saturating_mul(k.into()))
    }

    pub fn maybe_add_coldkey_index(coldkey: &T::AccountId) {
        if !StakingColdkeys::<T>::contains_key(coldkey) {
            let n = NumStakingColdkeys::<T>::get();
            StakingColdkeysByIndex::<T>::insert(n, coldkey.clone());
            StakingColdkeys::<T>::insert(coldkey.clone(), n);
            NumStakingColdkeys::<T>::mutate(|n| *n = n.saturating_add(1));
        }
    }

    /// Returns true if `coldkey` still holds any root (netuid 0) stake on any of its
    /// staking hotkeys. Used to decide whether the coldkey should remain indexed in the
    /// auto-claim staking-coldkey index.
    pub fn coldkey_has_root_stake(coldkey: &T::AccountId) -> bool {
        StakingHotkeys::<T>::get(coldkey).iter().any(|hotkey| {
            !Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
                .is_zero()
        })
    }

    /// Remove `coldkey` from the staking-coldkey index, compacting by moving the last
    /// entry into the freed slot so the index stays dense in `[0, n)`. This is the inverse
    /// of `maybe_add_coldkey_index` and keeps the
    /// `StakingColdkeys[c] == i <=> StakingColdkeysByIndex[i] == c` bijection consistent.
    pub fn maybe_remove_coldkey_index(coldkey: &T::AccountId) {
        if let Some(idx) = StakingColdkeys::<T>::take(coldkey) {
            let last = NumStakingColdkeys::<T>::get().saturating_sub(1);
            if idx != last
                && let Some(moved) = StakingColdkeysByIndex::<T>::take(last)
            {
                StakingColdkeysByIndex::<T>::insert(idx, moved.clone());
                StakingColdkeys::<T>::insert(moved, idx);
            } else {
                StakingColdkeysByIndex::<T>::remove(idx);
            }
            NumStakingColdkeys::<T>::put(last);
        }
    }

    pub fn run_auto_claim_root_divs(last_block_hash: T::Hash) -> Weight {
        let mut weight: Weight = Weight::default();

        let n = NumStakingColdkeys::<T>::get();
        let k = NumRootClaim::<T>::get();
        weight.saturating_accrue(T::DbWeight::get().reads(2));

        let coldkeys_to_claim: Vec<u64> = Self::block_hash_to_indices(last_block_hash, k, n);
        weight.saturating_accrue(Self::block_hash_to_indices_weight(k, n));

        for i in coldkeys_to_claim.iter() {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
            if let Ok(coldkey) = StakingColdkeysByIndex::<T>::try_get(i) {
                match Self::do_root_claim(coldkey.clone(), None) {
                    Ok(claim_weight) => weight.saturating_accrue(claim_weight),
                    Err(err) => log::error!("Error auto-claiming root dividends: {err:?}"),
                }
            }
        }

        weight
    }

    pub fn change_root_claim_type(coldkey: &T::AccountId, new_type: RootClaimTypeEnum) {
        RootClaimType::<T>::insert(coldkey.clone(), new_type.clone());

        Self::deposit_event(Event::RootClaimTypeSet {
            coldkey: coldkey.clone(),
            root_claim_type: new_type,
        });
    }

    pub fn transfer_root_claimed_for_new_keys(
        netuid: NetUid,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        let old_root_claimed = RootClaimed::<T>::get((netuid, old_hotkey, old_coldkey));
        RootClaimed::<T>::remove((netuid, old_hotkey, old_coldkey));

        RootClaimed::<T>::mutate((netuid, new_hotkey, new_coldkey), |new_root_claimed| {
            // Sum the two already-claimed watermarks. When BOTH the source and the
            // destination hold a legitimate RootClaimed — e.g. a coldkey swap onto a
            // hotkey the new coldkey has already staked to, or a hotkey swap that merges
            // two real positions — the merged "already claimed" total is old + new. Taking
            // the max would drop one side, under-count what has already been claimed, and
            // cause a future over-payment / double-claim of root dividends.
            //
            // GHSA-2026-010 (a *stale residual* watermark on new_hotkey inflating this sum
            // in the hotkey-swap path) is prevented upstream by the root-swap cleanliness
            // gate in `do_swap_hotkey`, which now also requires RootClaimed to be empty on
            // new_hotkey (see `test_do_swap_hotkey_err_new_hotkey_not_clean_for_root`). With
            // that gate the destination is always clean (new == 0) in the swap path, so the
            // sum cannot be inflated there.
            *new_root_claimed = old_root_claimed.saturating_add(*new_root_claimed);
        });
    }
    pub fn transfer_root_claimable_for_new_hotkey(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
    ) {
        let src_root_claimable = RootClaimable::<T>::get(old_hotkey);
        let mut dst_root_claimable = RootClaimable::<T>::get(new_hotkey);
        RootClaimable::<T>::remove(old_hotkey);

        for (netuid, claimable_rate) in src_root_claimable.into_iter() {
            dst_root_claimable
                .entry(netuid)
                .and_modify(|total| *total = total.saturating_add(claimable_rate))
                .or_insert(claimable_rate);
        }

        RootClaimable::<T>::insert(new_hotkey, dst_root_claimable);
    }

    /// Claim all root dividends for subnet and remove all associated data.
    pub fn finalize_all_subnet_root_dividends(netuid: NetUid) {
        let hotkeys = RootClaimable::<T>::iter_keys().collect::<Vec<_>>();

        for hotkey in hotkeys.iter() {
            RootClaimable::<T>::mutate(hotkey, |claimable| {
                claimable.remove(&netuid);
            });
        }

        let _ = RootClaimed::<T>::clear_prefix((netuid,), u32::MAX, None);
    }
}
