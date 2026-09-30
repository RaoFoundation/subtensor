//! Equal averaging of normalized validator scores, without Yuma or bonds.
//!
//! Mining identities live outside Yuma's u16 metagraph. Sparse score processing
//! touches only submitted destinations; registration and claims touch one miner.
use crate::weights::WeightInfo;
use crate::*;
use alloc::collections::{BTreeMap, BTreeSet};
use sp_core::{H256, U256};
use substrate_fixed::types::I64F64;
use subtensor_runtime_common::MechId;

// Bound scoring work independently of the u64 miner population.
pub const MAX_NULL_UIDS: u16 = 32_768;
pub const MAX_NULL_VALIDATORS: u16 = 64;
// Bound synchronous reward writes as well as scoring. Two maximum passes
// must fit the production block budget alongside the normal block step.
pub const MAX_NULL_WEIGHT_ENTRIES: u32 = 8_192;
const FRACTION_BITS: usize = 64;
const ROW_UNIT: u128 = 1u128 << FRACTION_BITS;

impl<T: Config> Pallet<T> {
    pub fn pow_register_weight() -> frame_support::weights::Weight {
        <T as Config>::WeightInfo::register().saturating_add(T::DbWeight::get().reads_writes(2, 2))
    }

    /// Compose existing reference bounds for sparse scoring and its database work.
    /// Dedicated benchmarks exercise these paths for reference-hardware regeneration.
    pub fn null_weights_weight(n: u32) -> Weight {
        <T as Config>::WeightInfo::set_null_weights(n.saturating_mul(2))
            .saturating_add(T::DbWeight::get().reads_writes(70, 4))
    }

    pub fn null_epoch_weight(netuid: NetUid) -> Weight {
        Self::null_scoring_weight(NullWeightEntries::<T>::get(netuid))
    }

    pub fn null_scoring_weight(entries: u32) -> Weight {
        <T as Config>::WeightInfo::check_null_weights_extension(entries.saturating_mul(2))
            .saturating_add(<T as Config>::WeightInfo::accrue_null_rewards())
            .saturating_add(T::DbWeight::get().reads_writes(
                1024u64.saturating_add(u64::from(entries)),
                256u64.saturating_add(u64::from(entries)),
            ))
    }

    pub fn do_set_null_consensus(netuid: NetUid, enabled: bool) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(!netuid.is_root(), Error::<T>::NullConsensusOnRoot);
        if NullConsensus::<T>::get(netuid) == enabled {
            return Ok(());
        }
        if enabled {
            ensure!(
                MechanismCountCurrent::<T>::get(netuid) == MechId::from(1),
                Error::<T>::NullConsensusRequiresSingleMechanism
            );
            // Import the bounded Yuma metagraph once. Subsequent toggles never
            // enumerate the independent, potentially enormous miner registry.
            if !NullMinerCount::<T>::contains_key(netuid) {
                ensure!(
                    Self::get_subnetwork_n(netuid) <= DefaultMaxAllowedUids::<T>::get(),
                    Error::<T>::NullConsensusYumaCapacityExceeded
                );
                let miners: Vec<_> = Keys::<T>::iter_prefix(netuid).collect();
                ensure!(
                    miners.len() as u64 <= NullMaxAllowedUids::<T>::get(netuid),
                    Error::<T>::NullConsensusCapacityReached
                );
                NullMinerCount::<T>::insert(netuid, 0);
                for (_, hotkey) in miners {
                    let coldkey = Owner::<T>::get(&hotkey);
                    Self::enroll_null_miner(netuid, &hotkey, &coldkey)?;
                }
            }
        }
        if enabled {
            // Cancel pending encrypted Yuma submissions only after enable checks
            // succeed. Delete epoch buckets without decoding their ciphertexts;
            // returning to Yuma must never replay these cancelled submissions.
            let index = Self::get_mechanism_storage_index(netuid, MechId::MAIN);
            let _ = TimelockedWeightCommits::<T>::clear_prefix(index, u32::MAX, None);
            // This is a Yuma epoch result, not a subnet hyperparameter. Null
            // emission has no withheld miner incentives to refresh it.
            MinerBurned::<T>::remove(netuid);
        }
        // Settle the old mode's pending budget before switching. Null claims
        // remain available while Yuma is active; neither registry is destroyed.
        let server = PendingServerEmission::<T>::take(netuid);
        let validator = PendingValidatorEmission::<T>::take(netuid);
        let root = PendingRootAlphaDivs::<T>::take(netuid);
        let owner = PendingOwnerCut::<T>::take(netuid);
        if NullConsensus::<T>::get(netuid) {
            Self::accrue_null_rewards(
                netuid,
                server.saturating_add(validator).saturating_add(root),
            );
            Self::distribute_owner_cut(netuid, owner);
        } else {
            // Pending Yuma rewards must not turn into retroactive null rewards.
            // Keep their original allocation for the next Yuma epoch.
            NullPausedYumaEmission::<T>::insert(netuid, (server, validator, root, owner));
        }
        if !enabled && let Some((s, v, r, o)) = NullPausedYumaEmission::<T>::take(netuid) {
            PendingServerEmission::<T>::insert(netuid, s);
            PendingValidatorEmission::<T>::insert(netuid, v);
            PendingRootAlphaDivs::<T>::insert(netuid, r);
            PendingOwnerCut::<T>::insert(netuid, o);
        }
        // Scores refer to the bounded validator metagraph, which can change in
        // Yuma. Require fresh submissions on every enable without touching miners.
        let _ = NullWeights::<T>::clear_prefix(netuid, u32::MAX, None);
        NullWeightEntries::<T>::remove(netuid);
        NullWeightsResetAt::<T>::insert(netuid, Self::get_current_block_as_u64());
        NullConsensus::<T>::insert(netuid, enabled);
        Ok(())
    }

    pub(crate) fn enroll_null_miner(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            !NullMiners::<T>::contains_key(netuid, hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );
        let uid = NullMinerCount::<T>::get(netuid);
        ensure!(
            uid < NullMaxAllowedUids::<T>::get(netuid),
            Error::<T>::NullConsensusCapacityReached
        );
        let next = uid
            .checked_add(1)
            .ok_or(Error::<T>::NullConsensusCapacityReached)?;
        // IDs are never reused: a newly admitted miner has no previous rewards.
        let index = U256::zero();
        NullMiners::<T>::insert(netuid, hotkey, (coldkey, index));
        NullMinerOwnerGeneration::<T>::insert(
            netuid,
            hotkey,
            NullColdkeyGeneration::<T>::get(coldkey),
        );
        NullMinerKeys::<T>::insert(netuid, uid, hotkey);
        NullMinerUids::<T>::insert(netuid, hotkey, uid);
        NullMinerCount::<T>::insert(netuid, next);
        Self::deposit_event(Event::NullMinerRegistered {
            netuid,
            uid,
            hotkey: hotkey.clone(),
            coldkey: coldkey.clone(),
        });
        Ok(())
    }

    /// Rotate only null identities when the caller has no global staking
    /// association. Never touch another coldkey's staking ownership or balances.
    #[frame_support::transactional]
    pub(crate) fn swap_null_hotkey(
        coldkey: &T::AccountId,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: Option<NetUid>,
    ) -> DispatchResult {
        ensure!(old_hotkey != new_hotkey, Error::<T>::NewHotKeyIsSameWithOld);
        ensure!(
            Self::is_subnet_account_id(new_hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );
        // As with a Yuma swap, a foreign staking owner reserves the destination.
        if let Ok(owner) = Owner::<T>::try_get(new_hotkey) {
            ensure!(owner == *coldkey, Error::<T>::NonAssociatedColdKey);
        }
        let subnets = if let Some(netuid) = netuid {
            ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
            vec![netuid]
        } else {
            Self::get_all_subnet_netuids()
                .into_iter()
                .filter(|netuid| Self::owns_null_miner(*netuid, old_hotkey, coldkey))
                .collect()
        };
        ensure!(!subnets.is_empty(), Error::<T>::NonAssociatedColdKey);
        let block = Self::get_current_block_as_u64();
        let mut weight = Weight::zero();
        for subnet in subnets {
            let last = LastHotkeySwapOnNetuid::<T>::get(subnet, coldkey);
            ensure!(
                last == 0 || last.saturating_add(T::HotkeySwapOnSubnetInterval::get()) < block,
                Error::<T>::HotKeySwapOnSubnetIntervalNotPassed
            );
            ensure!(
                Self::swap_null_miner(subnet, old_hotkey, new_hotkey, coldkey, true, &mut weight)?,
                Error::<T>::NonAssociatedColdKey
            );
            Self::record_hotkey_swap_on_netuid(
                subnet,
                coldkey,
                old_hotkey,
                new_hotkey,
                block,
                &mut weight,
            );
        }
        let cost = if netuid.is_some() {
            T::KeySwapOnSubnetCost::get()
        } else {
            Self::get_key_swap_cost().into()
        };
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, cost),
            Error::<T>::NotEnoughBalanceToPaySwapHotKey
        );
        Self::recycle_tao(coldkey, cost)?;
        if let Some(netuid) = netuid {
            Self::deposit_event(Event::HotkeySwappedOnSubnet {
                coldkey: coldkey.clone(),
                old_hotkey: old_hotkey.clone(),
                new_hotkey: new_hotkey.clone(),
                netuid,
            });
        } else {
            Self::deposit_event(Event::HotkeySwapped {
                coldkey: coldkey.clone(),
                old_hotkey: old_hotkey.clone(),
                new_hotkey: new_hotkey.clone(),
            });
        }
        Ok(())
    }

    /// Move one null identity and its endpoints, with bounded owner resolution.
    /// The caller must wrap this in a storage transaction.
    pub(crate) fn swap_null_miner(
        netuid: NetUid,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        preserve_yuma: bool,
        weight: &mut Weight,
    ) -> Result<bool, DispatchError> {
        weight.saturating_accrue(T::DbWeight::get().reads(1));
        let Some((owner, checkpoint)) = NullMiners::<T>::get(netuid, old_hotkey) else {
            return Ok(false);
        };
        // Independent null registrations can belong to different coldkeys.
        // A foreign row must neither move nor veto the staking owner's swap.
        let (owner, generation, resolved) =
            Self::resolve_null_miner_owner(netuid, old_hotkey, owner);
        weight.saturating_accrue(T::DbWeight::get().reads(65));
        if !resolved || owner != *coldkey {
            return Ok(false);
        }
        ensure!(
            !NullMiners::<T>::contains_key(netuid, new_hotkey)
                && !Self::is_hotkey_registered_on_network(netuid, new_hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );
        let uid = NullMinerUids::<T>::get(netuid, old_hotkey)
            .ok_or(Error::<T>::NullMinerNotRegistered)?;
        NullMinerOwnerGeneration::<T>::remove(netuid, old_hotkey);
        NullMiners::<T>::remove(netuid, old_hotkey);
        NullMiners::<T>::insert(netuid, new_hotkey, (owner, checkpoint));
        NullMinerUids::<T>::remove(netuid, old_hotkey);
        NullMinerUids::<T>::insert(netuid, new_hotkey, uid);
        NullMinerKeys::<T>::insert(netuid, uid, new_hotkey);
        NullMinerOwnerGeneration::<T>::insert(netuid, new_hotkey, generation);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(3, 7));
        // A null-only rotation must preserve another staking identity's Yuma
        // endpoints. Copy them in that case; only null membership changes.
        let retains_yuma =
            preserve_yuma && Self::is_hotkey_registered_on_network(netuid, old_hotkey);
        weight.saturating_accrue(T::DbWeight::get().reads(5));
        if let Some(value) = Axons::<T>::get(netuid, old_hotkey) {
            Axons::<T>::insert(netuid, new_hotkey, value);
        }
        if let Some(value) = Prometheus::<T>::get(netuid, old_hotkey) {
            Prometheus::<T>::insert(netuid, new_hotkey, value);
        }
        if let Some(value) = NeuronCertificates::<T>::get(netuid, old_hotkey) {
            NeuronCertificates::<T>::insert(netuid, new_hotkey, value);
        }
        if !retains_yuma {
            Axons::<T>::remove(netuid, old_hotkey);
            Prometheus::<T>::remove(netuid, old_hotkey);
            NeuronCertificates::<T>::remove(netuid, old_hotkey);
        }
        weight.saturating_accrue(T::DbWeight::get().writes(6));
        Ok(true)
    }

    /// Domain-separate work by subnet generation and recipient. A mempool observer
    /// cannot reuse another coldkey's seal to claim its miner registration.
    pub fn create_null_seal_hash(
        netuid: NetUid,
        block: u64,
        nonce: u64,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> H256 {
        use codec::Encode;
        let payload = (
            b"subtensor:null:equal:v1",
            netuid,
            RegisteredSubnetCounter::<T>::get(netuid),
            Self::get_block_hash_from_u64(block),
            nonce,
            hotkey,
            coldkey,
        )
            .encode();
        H256::from(sp_io::hashing::keccak_256(&payload))
    }

    pub fn do_null_pow_register(
        origin: OriginFor<T>,
        netuid: NetUid,
        block_number: u64,
        nonce: u64,
        work: Vec<u8>,
        hotkey: T::AccountId,
        coldkey: T::AccountId,
    ) -> DispatchResult {
        let signer = ensure_signed(origin)?;
        ensure!(signer == coldkey, Error::<T>::PowSignerColdkeyMismatch);
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            NullConsensus::<T>::get(netuid),
            Error::<T>::NullConsensusNotEnabled
        );
        ensure!(
            Self::get_network_registration_allowed(netuid),
            Error::<T>::NullConsensusRegistrationDisabled
        );
        ensure!(
            NetworkPowRegistrationAllowed::<T>::get(netuid),
            Error::<T>::NullConsensusPowRegistrationDisabled
        );
        ensure!(
            !NullMiners::<T>::contains_key(netuid, &hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );
        ensure!(
            Self::is_subnet_account_id(&hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );
        if let Ok(owner) = Owner::<T>::try_get(&hotkey) {
            ensure!(owner == coldkey, Error::<T>::NonAssociatedColdKey);
        }
        ensure!(
            RegistrationsThisBlock::<T>::get(netuid) < MaxRegistrationsPerBlock::<T>::get(netuid),
            Error::<T>::TooManyRegistrationsThisBlock
        );
        let now = Self::get_current_block_as_u64();
        ensure!(
            block_number < now && now.saturating_sub(block_number) < 3,
            Error::<T>::InvalidWorkBlock
        );
        ensure!(work.len() == 32, Error::<T>::PowInvalidSealLength);
        let hash = H256::from_slice(&work);
        ensure!(
            hash == Self::create_null_seal_hash(netuid, block_number, nonce, &hotkey, &coldkey),
            Error::<T>::InvalidSeal
        );
        ensure!(
            Self::hash_meets_difficulty(&hash, Self::get_difficulty(netuid).max(U256::one())),
            Error::<T>::InvalidDifficulty
        );
        // Membership moves during hotkey rotation; consumed work must not.
        ensure!(
            !UsedWork::<T>::contains_key(&work),
            Error::<T>::PowWorkAlreadyUsed
        );
        // Deliberately do not append to Yuma vectors, OwnedHotkeys, StakingHotkeys,
        // or the global Owner map. Null mining does not create a staking account.
        Self::enroll_null_miner(netuid, &hotkey, &coldkey)?;
        UsedWork::<T>::insert(work, now);
        RegistrationsThisBlock::<T>::mutate(netuid, |n| *n = n.saturating_add(1));
        Ok(())
    }

    /// Budget is already minted into alpha-out. Average normalized eligible
    /// rows equally, then credit only scored u64 miner IDs. No miner scan, Yuma
    /// clipping, bonds, stake-weighted averaging, or validator dividends.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn accrue_null_rewards(netuid: NetUid, budget: AlphaBalance) {
        let now = Self::get_current_block_as_u64();
        let owner = Self::get_owner_uid(netuid);
        let reset = NullWeightsResetAt::<T>::get(netuid);
        let cutoff = Self::get_activity_cutoff_blocks(netuid);
        let (stake, _, _) = Self::get_stake_weights_for_network(netuid);
        let threshold = I64F64::saturating_from_num(Self::get_stake_threshold());
        let mut candidates: Vec<_> = stake
            .iter()
            .enumerate()
            .filter(|(_, s)| **s >= threshold && **s > I64F64::from_num(0))
            .map(|(uid, s)| (uid, *s))
            .collect();
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut permits = vec![false; stake.len()];
        for (uid, _) in candidates.into_iter().take(usize::from(
            Self::get_max_allowed_validators(netuid).min(MAX_NULL_VALIDATORS),
        )) {
            if let Some(p) = permits.get_mut(uid) {
                *p = true;
            }
        }
        if let Some(permit) = owner.and_then(|uid| permits.get_mut(usize::from(uid))) {
            *permit = true;
        }
        let mut scores = BTreeMap::<u64, u128>::new();
        for (uid, row) in NullWeights::<T>::iter_prefix(netuid) {
            let last = NullLastUpdate::<T>::get(netuid, uid);
            let hotkey = Keys::<T>::get(netuid, uid);
            if (owner != Some(uid) && !permits.get(usize::from(uid)).copied().unwrap_or(false))
                || last <= reset
                || last.saturating_add(cutoff) < now
                || !Self::check_weights_min_stake(&hotkey, netuid)
            {
                Self::remove_null_weights(netuid, uid);
                continue;
            }
            let sum: u128 = row.iter().map(|(_, value)| u128::from(*value)).sum();
            if sum == 0 {
                continue;
            }
            for (dest, value) in row {
                let score = scores.entry(dest).or_default();
                *score = score.saturating_add(u128::from(value) * ROW_UNIT / sum);
            }
        }
        Self::update_voting_power_from_epoch(
            netuid,
            Keys::<T>::iter_prefix(netuid).map(|(uid, hotkey)| {
                let i = usize::from(uid);
                (
                    hotkey,
                    permits.get(i).copied().unwrap_or(false),
                    stake
                        .get(i)
                        .copied()
                        .unwrap_or_default()
                        .saturating_to_num::<u64>()
                        .into(),
                )
            }),
        );
        ValidatorPermit::<T>::insert(netuid, permits);
        let total: u128 = scores.values().copied().sum();
        if total == 0 {
            Self::recycle_subnet_alpha(netuid, budget);
            return;
        }
        let scaled = U256::from(budget.to_u64()) << FRACTION_BITS;
        if budget.is_zero() {
            return;
        }
        // Credit every scored miner before returning. No reward queue or idle
        // work: all entitlements from this pass are immediately claimable.
        for (uid, score) in scores {
            let reward = scaled * U256::from(score) / U256::from(total);
            NullRewardIndex::<T>::mutate(netuid, uid, |index| {
                *index = index.saturating_add(reward);
            });
        }
        NullUnclaimedAlpha::<T>::mutate(netuid, |value| *value = value.saturating_add(budget));
    }

    pub(crate) fn remove_null_weights(netuid: NetUid, uid: u16) {
        let count = NullWeights::<T>::decode_len(netuid, uid).unwrap_or(0) as u32;
        NullWeightEntries::<T>::mutate(netuid, |n| *n = n.saturating_sub(count));
        NullWeights::<T>::remove(netuid, uid);
    }

    pub(crate) fn null_miner_reward_index(netuid: NetUid, hotkey: &T::AccountId) -> U256 {
        NullMinerUids::<T>::get(netuid, hotkey)
            .map(|uid| NullRewardIndex::<T>::get(netuid, uid))
            .unwrap_or_default()
    }

    /// Read-only ownership check, including bounded coldkey succession.
    pub(crate) fn owns_null_miner(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> bool {
        NullMiners::<T>::get(netuid, hotkey).is_some_and(|(owner, _)| {
            let (owner, _, resolved) = Self::resolve_null_miner_owner(netuid, hotkey, owner);
            resolved && owner == *coldkey
        })
    }

    fn resolve_null_miner_owner(
        netuid: NetUid,
        hotkey: &T::AccountId,
        mut owner: T::AccountId,
    ) -> (T::AccountId, u128, bool) {
        let mut generation = NullMinerOwnerGeneration::<T>::get(netuid, hotkey);
        for _ in 0..64 {
            let Some((next, next_generation)) = NullColdkeySuccessor::<T>::get(&owner, generation)
            else {
                return (owner, generation, true);
            };
            owner = next;
            generation = next_generation;
        }
        (owner, generation, false)
    }

    /// Resolve at most 64 coldkey swaps. Longer histories are advanced by
    /// repeated calls, never by an unbounded walk or a population-wide rewrite.
    pub(crate) fn refresh_null_miner_owner(
        netuid: NetUid,
        hotkey: &T::AccountId,
        owner: T::AccountId,
        checkpoint: U256,
    ) -> (T::AccountId, bool) {
        let (owner, generation, resolved) = Self::resolve_null_miner_owner(netuid, hotkey, owner);
        NullMiners::<T>::insert(netuid, hotkey, (&owner, checkpoint));
        NullMinerOwnerGeneration::<T>::insert(netuid, hotkey, generation);
        (owner, resolved)
    }

    #[frame_support::transactional]
    pub fn do_claim_null_rewards(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: T::AccountId,
        stake_hotkey: T::AccountId,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let (owner, checkpoint) =
            NullMiners::<T>::get(netuid, &hotkey).ok_or(Error::<T>::NullMinerNotRegistered)?;
        let (owner, resolved) = Self::refresh_null_miner_owner(netuid, &hotkey, owner, checkpoint);
        if !resolved {
            Self::deposit_event(Event::NullRewardOwnerResolutionAdvanced(netuid, hotkey));
            return Ok(());
        }
        ensure!(coldkey == owner, Error::<T>::NonAssociatedColdKey);
        ensure!(
            Owner::<T>::contains_key(&stake_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        Self::ensure_staking_hotkeys_can_grow(&coldkey, &stake_hotkey)?;
        let amount = Self::settle_null_miner(netuid, &hotkey, &coldkey, checkpoint, &stake_hotkey);
        ensure!(!amount.is_zero(), Error::<T>::NullRewardsNotAvailable);
        Self::deposit_event(Event::NullRewardsClaimed {
            netuid,
            hotkey,
            coldkey,
            stake_hotkey,
            amount,
        });
        Ok(())
    }

    // Fixed 64-bit shifts are in range, and the shifted amount is at most i64::MAX.
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn settle_null_miner(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        checkpoint: U256,
        stake_hotkey: &T::AccountId,
    ) -> AlphaBalance {
        let earned = Self::null_miner_reward_index(netuid, hotkey).saturating_sub(checkpoint)
            >> FRACTION_BITS;
        // Share-pool deltas use i64. Leave excess and fractional entitlement for
        // another claim rather than truncating it or wrapping the pool update.
        let amount: AlphaBalance = earned.min(U256::from(i64::MAX as u64)).low_u64().into();
        if amount.is_zero() {
            return amount;
        }
        NullMiners::<T>::insert(
            netuid,
            hotkey,
            (
                coldkey,
                checkpoint.saturating_add(U256::from(amount.to_u64()) << FRACTION_BITS),
            ),
        );
        NullUnclaimedAlpha::<T>::mutate(netuid, |value| *value = value.saturating_sub(amount));
        // Existing Yuma collateral follows the miner that earned the reward,
        // independently of where the claimant wants the liquid remainder staked.
        let captured = Self::settle_miner_collateral(netuid, hotkey, coldkey, amount, amount);
        let liquid = amount.saturating_sub(captured);
        if !liquid.is_zero() {
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                stake_hotkey,
                coldkey,
                netuid,
                liquid,
            );
        }
        amount
    }

    pub fn do_set_null_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        dests: Vec<u64>,
        values: Vec<u32>,
        version_key: u64,
    ) -> DispatchResult {
        let hotkey = ensure_signed(origin)?;
        let uid = Self::validate_null_weights(&hotkey, netuid, &dests, &values, version_key)?;
        let row: Vec<_> = dests
            .into_iter()
            .zip(values)
            .filter(|(_, v)| *v != 0)
            .collect();
        Self::remove_null_weights(netuid, uid);
        NullWeightEntries::<T>::mutate(netuid, |n| *n = n.saturating_add(row.len() as u32));
        NullWeights::<T>::insert(netuid, uid, row);
        NullLastUpdate::<T>::insert(netuid, uid, Self::get_current_block_as_u64());
        Self::deposit_event(Event::WeightsSet(
            Self::get_mechanism_storage_index(netuid, MechId::MAIN),
            uid,
        ));
        Ok(())
    }

    /// Shared read-only admission protects the coldkey from fees for invalid hotkey calls.
    pub(crate) fn validate_null_weights(
        hotkey: &T::AccountId,
        netuid: NetUid,
        dests: &[u64],
        values: &[u32],
        version_key: u64,
    ) -> Result<u16, Error<T>> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            NullConsensus::<T>::get(netuid),
            Error::<T>::NullConsensusNotEnabled
        );
        ensure!(
            dests.len() == values.len(),
            Error::<T>::WeightVecNotEqualSize
        );
        ensure!(
            dests.len() <= MAX_NULL_WEIGHT_ENTRIES as usize,
            Error::<T>::NullConsensusWeightLimitExceeded
        );
        let uid = Uids::<T>::try_get(netuid, hotkey)
            .map_err(|_| Error::<T>::HotKeyNotRegisteredInSubNet)?;
        ensure!(
            Self::check_weights_min_stake(hotkey, netuid),
            Error::<T>::NotEnoughStakeToSetWeights
        );
        ensure!(
            Self::get_owner_uid(netuid) == Some(uid)
                || Self::get_validator_permit_for_uid(netuid, uid),
            Error::<T>::NeuronNoValidatorPermit
        );
        ensure!(
            Self::check_version_key(netuid, version_key),
            Error::<T>::IncorrectWeightVersionKey
        );
        let now = Self::get_current_block_as_u64();
        let last = NullLastUpdate::<T>::get(netuid, uid);
        ensure!(
            now > NullWeightsResetAt::<T>::get(netuid)
                && (last == 0
                    || now.saturating_sub(last) >= Self::get_weights_set_rate_limit(netuid)),
            Error::<T>::SettingWeightsTooFast
        );
        // Null IDs are append-only and contiguous. Rotation preserves IDs;
        // validating their range avoids a database lookup for every destination.
        let miner_count = NullMinerCount::<T>::get(netuid);
        let mut seen = BTreeSet::new();
        for &dest in dests {
            ensure!(seen.insert(dest), Error::<T>::DuplicateUids);
            ensure!(dest < miner_count, Error::<T>::UidVecContainInvalidOne);
        }
        let sum: u64 = values.iter().map(|&v| u64::from(v)).sum();
        ensure!(sum != 0, Error::<T>::NullConsensusWeightsAllZero);
        let nonzero = values.iter().filter(|&&v| v != 0).count();
        ensure!(
            nonzero as u64
                >= u64::from(Self::get_min_allowed_weights(netuid))
                    .min(NullMinerCount::<T>::get(netuid)),
            Error::<T>::WeightVecLengthIsLow
        );
        let max = u64::from(values.iter().copied().max().unwrap_or_default());
        ensure!(
            max.saturating_mul(u64::from(u16::MAX))
                <= sum.saturating_mul(u64::from(Self::get_max_weight_limit(netuid))),
            Error::<T>::MaxWeightExceeded
        );
        let previous = NullWeights::<T>::decode_len(netuid, uid).unwrap_or(0) as u32;
        ensure!(
            NullWeightEntries::<T>::get(netuid)
                .saturating_sub(previous)
                .saturating_add(nonzero as u32)
                <= MAX_NULL_WEIGHT_ENTRIES,
            Error::<T>::NullConsensusWeightLimitExceeded
        );
        if !NullWeights::<T>::contains_key(netuid, uid) {
            ensure!(
                NullWeights::<T>::iter_key_prefix(netuid)
                    .take(usize::from(MAX_NULL_VALIDATORS).saturating_add(1))
                    .count()
                    <= usize::from(MAX_NULL_VALIDATORS),
                Error::<T>::NullConsensusValidatorLimitExceeded
            );
        }
        Ok(uid)
    }
}
