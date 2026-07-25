//! Epoch dividend / incentive calculation and stake payout.

use super::*;
use super::{as_u96f32, to_u64};
use alloc::collections::BTreeMap;
use safe_math::*;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

impl<T: Config> Pallet<T> {
    /// Fold epoch `(hotkey, incentive, dividend)` rows into per-hotkey incentive totals and
    /// parent-aware dividend totals (via [`Pallet::get_parent_child_dividends_distribution`]).
    pub fn calculate_dividends_and_incentives(
        netuid: NetUid,
        hotkey_emission: Vec<(T::AccountId, AlphaBalance, AlphaBalance)>,
    ) -> (
        BTreeMap<T::AccountId, AlphaBalance>,
        BTreeMap<T::AccountId, U96F32>,
    ) {
        // Accumulate emission of dividends and incentive per hotkey.
        let mut incentives: BTreeMap<T::AccountId, AlphaBalance> = BTreeMap::new();
        let mut dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, incentive, dividend) in hotkey_emission {
            // Accumulate incentives to miners.
            incentives
                .entry(hotkey.clone())
                .and_modify(|e| *e = e.saturating_add(incentive))
                .or_insert(incentive);
            // Accumulate dividends to parents.
            let div_tuples: Vec<(T::AccountId, AlphaBalance)> =
                Self::get_parent_child_dividends_distribution(&hotkey, netuid, dividend);
            // Accumulate dividends per hotkey.
            for (parent, parent_div) in div_tuples {
                dividends
                    .entry(parent)
                    .and_modify(|e| *e = e.saturating_add(as_u96f32!(parent_div)))
                    .or_insert(as_u96f32!(parent_div));
            }
        }
        log::debug!("incentives: {incentives:?}");
        log::debug!("dividends: {dividends:?}");

        (incentives, dividends)
    }

    /// Split each hotkey's dividend into proportional alpha vs root-alpha claimables using
    /// subnet stake vs root stake weighted by `tao_weight`.
    pub fn calculate_dividend_distribution(
        pending_alpha: AlphaBalance,
        pending_root_alpha: AlphaBalance,
        tao_weight: U96F32,
        stake_map: BTreeMap<T::AccountId, (AlphaBalance, AlphaBalance)>,
        dividends: BTreeMap<T::AccountId, U96F32>,
    ) -> (
        BTreeMap<T::AccountId, U96F32>,
        BTreeMap<T::AccountId, U96F32>,
    ) {
        log::debug!("dividends: {dividends:?}");
        log::debug!("stake_map: {stake_map:?}");
        log::debug!("pending_alpha: {pending_alpha:?}");
        log::debug!("pending_root_alpha: {pending_root_alpha:?}");
        log::debug!("tao_weight: {tao_weight:?}");

        // Setup.
        let zero: U96F32 = as_u96f32!(0.0);

        // Accumulate root alpha divs and alpha_divs. For each hotkey we compute their
        // local and root dividend proportion based on their alpha_stake/root_stake
        let mut total_root_divs: U96F32 = as_u96f32!(0);
        let mut total_alpha_divs: U96F32 = as_u96f32!(0);
        let mut root_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        let mut alpha_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, dividend) in dividends {
            if let Some((alpha_stake, root_stake)) = stake_map.get(&hotkey) {
                let alpha_stake = alpha_stake.to_u64();
                let root_stake = root_stake.to_u64();
                // Get hotkey ALPHA on subnet.
                let alpha_stake = as_u96f32!(alpha_stake);
                // Get hotkey TAO on root.
                let root_stake = as_u96f32!(root_stake);

                // Convert TAO to alpha with weight.
                let root_alpha = root_stake.saturating_mul(tao_weight);
                // Get total from root and local
                let total_alpha = alpha_stake.saturating_add(root_alpha);
                // Compute root prop.
                let root_prop = root_alpha.checked_div(total_alpha).unwrap_or(zero);
                // Compute root dividends
                let root_divs = dividend.saturating_mul(root_prop);
                // Compute alpha dividends
                let alpha_divs = dividend.saturating_sub(root_divs);
                // Record the alpha dividends.
                alpha_dividends
                    .entry(hotkey.clone())
                    .and_modify(|e| *e = e.saturating_add(alpha_divs))
                    .or_insert(alpha_divs);
                // Accumulate total alpha divs.
                total_alpha_divs = total_alpha_divs.saturating_add(alpha_divs);
                // Record the root dividends.
                root_dividends
                    .entry(hotkey.clone())
                    .and_modify(|e| *e = e.saturating_add(root_divs))
                    .or_insert(root_divs);
                // Accumulate total root divs.
                total_root_divs = total_root_divs.saturating_add(root_divs);
            }
        }
        log::debug!("alpha_dividends: {alpha_dividends:?}");
        log::debug!("root_dividends: {root_dividends:?}");
        log::debug!("total_root_divs: {total_root_divs:?}");
        log::debug!("total_alpha_divs: {total_alpha_divs:?}");

        // Compute root alpha divs. Here we take
        let mut root_alpha_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, root_divs) in root_dividends {
            // Root proportion.
            let root_share: U96F32 = root_divs.checked_div(total_root_divs).unwrap_or(zero);
            log::debug!("hotkey: {hotkey:?}, root_share: {root_share:?}");
            // Root proportion in alpha
            let root_alpha: U96F32 = as_u96f32!(pending_root_alpha).saturating_mul(root_share);
            log::debug!("hotkey: {hotkey:?}, root_alpha: {root_alpha:?}");
            // Record root dividends as TAO.
            root_alpha_dividends
                .entry(hotkey)
                .and_modify(|e| *e = root_alpha)
                .or_insert(root_alpha);
        }
        log::debug!("root_alpha_dividends: {root_alpha_dividends:?}");

        // Compute proportional alpha divs using the pending alpha and total alpha divs from the epoch.
        let mut prop_alpha_dividends: BTreeMap<T::AccountId, U96F32> = BTreeMap::new();
        for (hotkey, alpha_divs) in alpha_dividends {
            // Alpha proportion.
            let alpha_share: U96F32 = alpha_divs.checked_div(total_alpha_divs).unwrap_or(zero);
            log::debug!("hotkey: {hotkey:?}, alpha_share: {alpha_share:?}");

            // Compute the proportional pending_alpha to this hotkey.
            let prop_alpha = as_u96f32!(pending_alpha).saturating_mul(alpha_share);
            log::debug!("hotkey: {hotkey:?}, prop_alpha: {prop_alpha:?}");
            // Record the proportional alpha dividends.
            prop_alpha_dividends
                .entry(hotkey.clone())
                .and_modify(|e| *e = prop_alpha)
                .or_insert(prop_alpha);
        }
        log::debug!("prop_alpha_dividends: {prop_alpha_dividends:?}");

        (prop_alpha_dividends, root_alpha_dividends)
    }

    /// Hotkeys immune from miner emission on this subnet: SN owner hotkey first, then the
    /// coldkey's owned hotkeys ordered by newest registration.
    fn owner_immune_hotkeys_on_subnet(netuid: NetUid, coldkey: &T::AccountId) -> Vec<T::AccountId> {
        // Gather (block, uid, hotkey) only for hotkeys that have a UID and a registration block.
        let mut triples: Vec<(u64, u16, T::AccountId)> = OwnedHotkeys::<T>::get(coldkey)
            .into_iter()
            .filter_map(|hotkey| {
                // Uids must exist, filter_map ignores hotkeys without UID
                Uids::<T>::get(netuid, &hotkey).map(|uid| {
                    let block = BlockAtRegistration::<T>::get(netuid, uid);
                    (block, uid, hotkey)
                })
            })
            .collect();

        // Sort by BlockAtRegistration (descending), then by uid (ascending)
        // Recent registration is priority so that we can let older keys expire (get non-immune)
        triples.sort_by(|(b1, u1, _), (b2, u2, _)| b2.cmp(b1).then(u1.cmp(u2)));

        // Project to just hotkeys
        let mut owner_hotkeys: Vec<T::AccountId> =
            triples.into_iter().map(|(_, _, hk)| hk).collect();

        // Insert subnet owner hotkey in the beginning of the list if valid and not
        // already present
        if let Ok(owner_hk) = SubnetOwnerHotkey::<T>::try_get(netuid)
            && Uids::<T>::get(netuid, &owner_hk).is_some()
            && !owner_hotkeys.contains(&owner_hk)
        {
            owner_hotkeys.insert(0, owner_hk);
        }

        owner_hotkeys
    }

    /// Pay owner cut, miner incentives (recycle/burn immune keys), and validator alpha /
    /// root-alpha dividends, updating per-subnet dividend storage.
    pub fn distribute_dividends_and_incentives(
        netuid: NetUid,
        owner_cut: AlphaBalance,
        incentives: BTreeMap<T::AccountId, AlphaBalance>,
        alpha_dividends: BTreeMap<T::AccountId, U96F32>,
        root_alpha_dividends: BTreeMap<T::AccountId, U96F32>,
    ) {
        // Distribute the owner cut.
        if let Ok(owner_coldkey) = SubnetOwner::<T>::try_get(netuid)
            && let Ok(owner_hotkey) = SubnetOwnerHotkey::<T>::try_get(netuid)
        {
            // Increase stake for owner hotkey and coldkey.
            log::debug!(
                "owner_hotkey: {owner_hotkey:?} owner_coldkey: {owner_coldkey:?}, owner_cut: {owner_cut:?}"
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &owner_hotkey,
                &owner_coldkey,
                netuid,
                owner_cut,
            );
            // If the subnet is leased, notify the lease logic that owner cut has been distributed.
            if let Some(lease_id) = SubnetUidToLeaseId::<T>::get(netuid) {
                Self::distribute_leased_network_dividends(lease_id, owner_cut);
            }

            // Auto-lock owner's cut
            Self::auto_lock_owner_cut(netuid, owner_cut);
        }

        // Distribute mining incentives.
        let subnet_owner_coldkey = SubnetOwner::<T>::get(netuid);
        let owner_hotkeys = Self::owner_immune_hotkeys_on_subnet(netuid, &subnet_owner_coldkey);
        log::debug!("incentives: owner hotkeys: {owner_hotkeys:?}");
        // Track total miner emission vs the portion withheld from miners this tempo
        // (directed to an owner/immune hotkey) to record the withheld proportion.
        let mut total_incentive: AlphaBalance = AlphaBalance::ZERO;
        let mut withheld_incentive: AlphaBalance = AlphaBalance::ZERO;
        for (hotkey, incentive) in incentives {
            log::debug!("incentives: hotkey: {incentive:?}");
            total_incentive = total_incentive.saturating_add(incentive);

            // Skip/burn miner-emission for immune keys
            if owner_hotkeys.contains(&hotkey) {
                log::debug!(
                    "incentives: hotkey: {hotkey:?} is SN owner hotkey or associated hotkey, skipping {incentive:?}"
                );
                // Miner emission directed to an owner (immune) hotkey is withheld from
                // miners whether it is recycled or burned. Count both toward the withheld
                // proportion so the emission penalty cannot be dodged by choosing Recycle
                // and an unset RecycleOrBurn config is not uniquely penalized.
                withheld_incentive = withheld_incentive.saturating_add(incentive);
                // Check if we should recycle or burn the incentive
                match RecycleOrBurn::<T>::try_get(netuid) {
                    Ok(RecycleOrBurnEnum::Recycle) => {
                        log::debug!("recycling {incentive:?}");
                        Self::recycle_subnet_alpha(netuid, incentive);
                    }
                    Ok(RecycleOrBurnEnum::Burn) | Err(_) => {
                        log::debug!("burning {incentive:?}");
                        Self::burn_subnet_alpha(netuid, incentive);
                    }
                }
                continue;
            }

            let owner: T::AccountId = Owner::<T>::get(&hotkey);

            // Settle collateral first: below a miner-set floor, part of the
            // emission is captured into the lock (staked to the registered
            // hotkey itself, never the auto-stake destination, so it lands on
            // the guarded position); above the floor, earned emission
            // releases locked collateral. Miner incentive is fully
            // capturable. Only the uncaptured remainder is credited below.
            let captured =
                Self::settle_miner_collateral(netuid, &hotkey, &owner, incentive, incentive);
            let liquid = incentive.saturating_sub(captured);
            if liquid.is_zero() {
                continue;
            }

            let maybe_dest = AutoStakeDestination::<T>::get(&owner, netuid);

            // Always stake but only emit event if autostake is set.
            let destination = maybe_dest.clone().unwrap_or(hotkey.clone());

            if let Some(dest) = maybe_dest {
                log::debug!("incentives: auto staking {liquid:?} to {dest:?}");
                Self::deposit_event(Event::<T>::AutoStakeAdded {
                    netuid,
                    destination: dest,
                    hotkey: hotkey.clone(),
                    owner: owner.clone(),
                    incentive: liquid,
                });
            }

            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &destination,
                &owner,
                netuid,
                liquid,
            );
        }

        // Record the proportion of this tempo's miner emission that was withheld from
        // miners (directed to owner/immune hotkeys, whether recycled or burned).
        let withheld_proportion: U96F32 = U96F32::saturating_from_num(withheld_incentive.to_u64())
            .checked_div(U96F32::saturating_from_num(total_incentive.to_u64()))
            .unwrap_or_else(|| U96F32::saturating_from_num(0));
        MinerBurned::<T>::insert(netuid, withheld_proportion);

        // Distribute alpha divs. Split take vs nominators first so nominator
        // shares can never be floor-captured into owner collateral. Full
        // dividend emission still drives release rate / earned; only the
        // validator take is capturable.
        let _ = AlphaDividendsPerSubnet::<T>::clear_prefix(netuid, u32::MAX, None);
        for (hotkey, alpha_divs) in alpha_dividends {
            let owner: T::AccountId = Owner::<T>::get(&hotkey);
            let total: AlphaBalance = to_u64!(alpha_divs).into();
            let alpha_take: U96F32 =
                Self::get_hotkey_take_float(&hotkey).saturating_mul(alpha_divs);
            let nominator_divs: U96F32 = alpha_divs.saturating_sub(alpha_take);
            let take: AlphaBalance = to_u64!(alpha_take).into();
            let captured = Self::settle_miner_collateral(netuid, &hotkey, &owner, total, take);
            let liquid_take = take.saturating_sub(captured);
            if !liquid_take.is_zero() {
                log::debug!("hotkey: {hotkey:?} alpha_take: {liquid_take:?}");
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    &owner,
                    netuid,
                    liquid_take,
                );
            }
            let nominator_alpha: AlphaBalance = to_u64!(nominator_divs).into();
            if !nominator_alpha.is_zero() {
                log::debug!("hotkey: {hotkey:?} alpha_divs: {nominator_divs:?}");
                Self::increase_stake_for_hotkey_on_subnet(&hotkey, netuid, nominator_alpha);
                AlphaDividendsPerSubnet::<T>::mutate(netuid, &hotkey, |divs| {
                    *divs = divs.saturating_add(nominator_alpha);
                });
            }
            let total_hotkey_alpha = TotalHotkeyAlpha::<T>::get(&hotkey, netuid);
            TotalHotkeyAlphaLastEpoch::<T>::insert(hotkey, netuid, total_hotkey_alpha);
        }

        // Distribute root alpha divs. Same ownership rule: full root emission
        // for release/earned; only validator take is capturable.
        let _ = RootAlphaDividendsPerSubnet::<T>::clear_prefix(netuid, u32::MAX, None);
        for (hotkey, root_alpha) in root_alpha_dividends {
            let owner: T::AccountId = Owner::<T>::get(&hotkey);
            let total: AlphaBalance = to_u64!(root_alpha).into();
            let alpha_take: U96F32 =
                Self::get_hotkey_take_float(&hotkey).saturating_mul(root_alpha);
            let root_claimable: U96F32 = root_alpha.saturating_sub(alpha_take);
            let take: AlphaBalance = to_u64!(alpha_take).into();
            let captured = Self::settle_miner_collateral(netuid, &hotkey, &owner, total, take);
            let liquid_take = take.saturating_sub(captured);
            if !liquid_take.is_zero() {
                log::debug!("hotkey: {hotkey:?} alpha_take: {liquid_take:?}");
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    &owner,
                    netuid,
                    liquid_take,
                );
            }

            let root_claimable_alpha: AlphaBalance = to_u64!(root_claimable).into();
            if !root_claimable_alpha.is_zero() {
                Self::increase_root_claimable_for_hotkey_and_subnet(
                    &hotkey,
                    netuid,
                    root_claimable_alpha,
                );

                RootAlphaDividendsPerSubnet::<T>::mutate(netuid, &hotkey, |divs| {
                    *divs = divs.saturating_add(root_claimable_alpha);
                });
            }
        }
    }

    /// Map each hotkey to `(alpha_stake_on_subnet, root_tao_stake)` for dividend weighting.
    pub fn get_stake_map(
        netuid: NetUid,
        hotkeys: Vec<&T::AccountId>,
    ) -> BTreeMap<T::AccountId, (AlphaBalance, AlphaBalance)> {
        let mut stake_map: BTreeMap<T::AccountId, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        for hotkey in hotkeys {
            // Get hotkey ALPHA on subnet.
            let alpha_stake = Self::get_stake_for_hotkey_on_subnet(hotkey, netuid);
            // Get hotkey TAO on root.
            let root_stake = Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT);
            stake_map.insert(hotkey.clone(), (alpha_stake, root_stake));
        }
        stake_map
    }

    /// Run incentive/dividend aggregation then alpha vs root split for one subnet epoch.
    pub fn calculate_dividend_and_incentive_distribution(
        netuid: NetUid,
        pending_root_alpha: AlphaBalance,
        pending_validator_alpha: AlphaBalance,
        hotkey_emission: Vec<(T::AccountId, AlphaBalance, AlphaBalance)>,
        tao_weight: U96F32,
    ) -> (
        BTreeMap<T::AccountId, AlphaBalance>,
        (
            BTreeMap<T::AccountId, U96F32>,
            BTreeMap<T::AccountId, U96F32>,
        ),
    ) {
        let (incentives, dividends) =
            Self::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let stake_map = Self::get_stake_map(netuid, dividends.keys().collect::<Vec<_>>());

        let (alpha_dividends, root_alpha_dividends) = Self::calculate_dividend_distribution(
            pending_validator_alpha,
            pending_root_alpha,
            tao_weight,
            stake_map,
            dividends,
        );

        (incentives, (alpha_dividends, root_alpha_dividends))
    }

    /// Run [`Pallet::epoch_with_mechanisms`] for drained pending alpha and pay out the
    /// resulting incentives and dividends.
    pub fn distribute_emission(
        netuid: NetUid,
        pending_server_alpha: AlphaBalance,
        pending_validator_alpha: AlphaBalance,
        pending_root_alpha: AlphaBalance,
        pending_owner_cut: AlphaBalance,
    ) {
        log::debug!(
            "Draining pending alpha emission for netuid {netuid:?}, pending_server_alpha: {pending_server_alpha:?}, pending_validator_alpha: {pending_validator_alpha:?}, pending_root_alpha: {pending_root_alpha:?}, pending_owner_cut: {pending_owner_cut:?}"
        );

        let tao_weight = Self::get_tao_weight();
        let total_alpha_minus_owner_cut = pending_server_alpha
            .saturating_add(pending_validator_alpha)
            .saturating_add(pending_root_alpha);

        // Run the epoch, using the alpha going to both the servers and the validators.
        let hotkey_emission: Vec<(T::AccountId, AlphaBalance, AlphaBalance)> =
            Self::epoch_with_mechanisms(netuid, total_alpha_minus_owner_cut);
        log::debug!("hotkey_emission: {hotkey_emission:?}");

        // Compute the pending validator alpha.
        // This is the total alpha being injected,
        // minus the the alpha for the miners, (50%)
        // and minus the alpha swapped for TAO (pending_swapped).
        // Important! If the incentives are 0, then Validators get 100% of the alpha.
        let incentive_sum = hotkey_emission
            .iter()
            .fold(AlphaBalance::default(), |acc, (_, incentive, _)| {
                acc.saturating_add(*incentive)
            });
        log::debug!("incentive_sum: {incentive_sum:?}");

        let validator_alpha = if !incentive_sum.is_zero() {
            pending_validator_alpha
        } else {
            // If the incentive is 0, then Alpha Validators get both the server and validator alpha.
            pending_validator_alpha.saturating_add(pending_server_alpha)
        };
        let root_alpha = pending_root_alpha;
        let owner_cut = pending_owner_cut;

        let (incentives, (alpha_dividends, root_alpha_dividends)) =
            Self::calculate_dividend_and_incentive_distribution(
                netuid,
                root_alpha,
                validator_alpha,
                hotkey_emission,
                tao_weight,
            );

        Self::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            alpha_dividends,
            root_alpha_dividends,
        );
    }

    /// Returns the self contribution of a hotkey on a subnet.
    /// This is the portion of the hotkey's stake that is provided by itself, and not delegated to other hotkeys.
    pub fn get_self_contribution(hotkey: &T::AccountId, netuid: NetUid) -> u64 {
        // Get all childkeys for this hotkey.
        let childkeys = Self::get_children(hotkey, netuid);
        let mut remaining_proportion: U96F32 = U96F32::saturating_from_num(1.0);
        for (proportion, _) in childkeys {
            remaining_proportion = remaining_proportion.saturating_sub(
                U96F32::saturating_from_num(proportion) // Normalize
                    .safe_div(U96F32::saturating_from_num(u64::MAX)),
            );
        }

        // Get TAO weight
        let tao_weight: U96F32 = Self::get_tao_weight();

        // Get the hotkey's stake including weight
        let root_stake: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT));
        let alpha_stake: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, netuid));

        // Calculate the
        let alpha_contribution: U96F32 = alpha_stake.saturating_mul(remaining_proportion);
        let root_contribution: U96F32 = root_stake
            .saturating_mul(remaining_proportion)
            .saturating_mul(tao_weight);
        let combined_contribution: U96F32 = alpha_contribution.saturating_add(root_contribution);

        // Return the combined contribution as a u64
        combined_contribution.saturating_to_num::<u64>()
    }

    /// Returns a list of tuples for each parent associated with this hotkey including self
    /// Each tuples contains the dividends owed to that hotkey given their parent proportion
    /// The hotkey child take proportion is removed from this and added to the tuples for self.
    /// The hotkey also gets a portion based on its own stake contribution, this is added to the childkey take.
    ///
    /// # Arguments
    /// * `hotkye`: The hotkey to distribute out from.
    /// * `netuid`: The netuid we are computing on.
    /// * `dividends`: the dividends to distribute.
    ///
    /// # Returns
    /// * dividend_tuples: `Vec<(T::AccountId, u64)>` - Vector of (hotkey, divs) for each parent including self.
    ///
    pub fn get_parent_child_dividends_distribution(
        hotkey: &T::AccountId,
        netuid: NetUid,
        dividends: AlphaBalance,
    ) -> Vec<(T::AccountId, AlphaBalance)> {
        // hotkey dividends.
        let mut dividend_tuples: Vec<(T::AccountId, AlphaBalance)> = vec![];

        // Calculate the hotkey's share of the validator emission based on its childkey take
        let validating_emission: U96F32 = U96F32::saturating_from_num(dividends);
        let mut remaining_emission: U96F32 = validating_emission;
        let burn_take_proportion: U96F32 = Self::get_ck_burn();
        let child_take_proportion: U96F32 =
            U96F32::saturating_from_num(Self::get_childkey_take(hotkey, netuid))
                .safe_div(U96F32::saturating_from_num(u16::MAX));
        log::debug!("Childkey take proportion: {child_take_proportion:?} for hotkey {hotkey:?}");
        // NOTE: Only the validation emission should be split amongst parents.

        // Grab the owner of the childkey.
        let childkey_owner = Self::get_owning_coldkey_for_hotkey(hotkey);

        // Initialize variables to track emission distribution
        let mut to_parents: u64 = 0;
        let mut total_child_take: U96F32 = U96F32::saturating_from_num(0);

        // Initialize variables to calculate total stakes from parents
        let mut total_contribution: U96F32 = U96F32::saturating_from_num(0);
        let mut parent_contributions: Vec<(T::AccountId, U96F32)> = Vec::new();

        // Get the weights for root and alpha stakes in emission distribution
        let tao_weight: U96F32 = Self::get_tao_weight();

        // Get self contribution, removing any childkey proportions.
        let self_contribution = Self::get_self_contribution(hotkey, netuid);
        log::debug!(
            "Self contribution for hotkey {hotkey:?} on netuid {netuid:?}: {self_contribution:?}"
        );
        // Add self contribution to total contribution but not to the parent contributions.
        total_contribution =
            total_contribution.saturating_add(U96F32::saturating_from_num(self_contribution));

        // Calculate total root and alpha (subnet-specific) stakes from all parents
        for (proportion, parent) in Self::get_parents(hotkey, netuid) {
            // Convert the parent's stake proportion to a fractional value
            let parent_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));

            // Get the parent's root and subnet-specific (alpha) stakes
            let parent_root: U96F32 = U96F32::saturating_from_num(
                Self::get_stake_for_hotkey_on_subnet(&parent, NetUid::ROOT),
            );
            let parent_alpha: U96F32 =
                U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(&parent, netuid));

            // Calculate the parent's contribution to the hotkey's stakes
            let parent_alpha_contribution: U96F32 = parent_alpha.saturating_mul(parent_proportion);
            let parent_root_contribution: U96F32 = parent_root
                .saturating_mul(parent_proportion)
                .saturating_mul(tao_weight);
            let combined_contribution: U96F32 =
                parent_alpha_contribution.saturating_add(parent_root_contribution);

            // Add to the total stakes
            total_contribution = total_contribution.saturating_add(combined_contribution);
            // Store the parent's contributions for later use
            parent_contributions.push((parent.clone(), combined_contribution));
            log::debug!(
                "Parent contribution for hotkey {hotkey:?} from parent {parent:?}: {combined_contribution:?}"
            );
        }

        // Distribute emission to parents based on their contributions.
        // Deduct childkey take from parent contribution.
        for (parent, contribution) in parent_contributions {
            let parent_owner = Self::get_owning_coldkey_for_hotkey(&parent);

            // Get the stake contribution of this parent key of the total stake.
            let emission_factor: U96F32 = contribution
                .checked_div(total_contribution)
                .unwrap_or(U96F32::saturating_from_num(0));

            // Get the parent's portion of the validating emission based on their contribution.
            let mut parent_emission: U96F32 = validating_emission.saturating_mul(emission_factor);
            // Remove this emission from the remaining emission.
            remaining_emission = remaining_emission.saturating_sub(parent_emission);

            // Get the childkey take for this parent.
            let mut burn_take: U96F32 = U96F32::saturating_from_num(0);
            let mut child_take: U96F32 = U96F32::saturating_from_num(0);
            if parent_owner != childkey_owner {
                // The parent is from a different coldkey, we burn some proportion
                burn_take = burn_take_proportion.saturating_mul(parent_emission);
                child_take = child_take_proportion.saturating_mul(parent_emission);
                parent_emission = parent_emission.saturating_sub(burn_take);
                parent_emission = parent_emission.saturating_sub(child_take);
                total_child_take = total_child_take.saturating_add(child_take);

                Self::recycle_subnet_alpha(
                    netuid,
                    AlphaBalance::from(burn_take.saturating_to_num::<u64>()),
                );
            };
            log::debug!("burn_takee: {burn_take:?} for hotkey {hotkey:?}");
            log::debug!("child_take: {child_take:?} for hotkey {hotkey:?}");
            log::debug!("parent_emission: {parent_emission:?} for hotkey {hotkey:?}");
            log::debug!("total_child_take: {total_child_take:?} for hotkey {hotkey:?}");

            log::debug!("remaining emission: {remaining_emission:?}");

            // Add the parent's emission to the distribution list
            dividend_tuples.push((
                parent.clone(),
                parent_emission.saturating_to_num::<u64>().into(),
            ));

            // Keep track of total emission distributed to parents
            to_parents = to_parents.saturating_add(parent_emission.saturating_to_num::<u64>());
            log::debug!(
                "Parent contribution for parent {parent:?} with contribution: {contribution:?}, of total: {total_contribution:?} ({emission_factor:?}), of emission: {validating_emission:?} gets: {parent_emission:?}",
            );
        }
        // Calculate the final emission for the hotkey itself.
        // This includes the take left from the parents and the self contribution.
        let child_emission = remaining_emission
            .saturating_add(total_child_take)
            .saturating_to_num::<u64>()
            .into();

        // Add the hotkey's own emission to the distribution list
        dividend_tuples.push((hotkey.clone(), child_emission));

        dividend_tuples
    }
}
