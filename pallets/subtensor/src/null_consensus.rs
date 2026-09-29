//! Equal emission without scoring, bonds, validator selection, or miner epochs.
//!
//! Mining identities live outside Yuma's u16 metagraph. One fixed-size reward
//! index advances per emitting subnet; registration and claims touch one miner.
use crate::weights::WeightInfo;
use crate::*;
use sp_core::{H256, U256};
use subtensor_runtime_common::MechId;

// Retained for legacy metadata/benchmarks. These do not cap null membership.
pub const MAX_NULL_UIDS: u16 = 32_768;
pub const MAX_NULL_VALIDATORS: u16 = 64;
const FRACTION_BITS: usize = 64;

impl<T: Config> Pallet<T> {
    pub fn pow_register_weight(_netuid: NetUid) -> frame_support::weights::Weight {
        <T as Config>::WeightInfo::register().saturating_add(T::DbWeight::get().writes(1))
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
                NullMinerCount::<T>::insert(netuid, 0);
                for (_, hotkey) in Keys::<T>::iter_prefix(netuid) {
                    let coldkey = Owner::<T>::get(&hotkey);
                    Self::enroll_null_miner(netuid, &hotkey, &coldkey)?;
                }
            }
        }
        if enabled {
            Self::update_voting_power_from_epoch(
                netuid,
                Keys::<T>::iter_prefix(netuid)
                    .map(|(_, hotkey)| (hotkey, false, AlphaBalance::ZERO)),
            );
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
        let next = uid
            .checked_add(1)
            .ok_or(Error::<T>::NullConsensusCapacityReached)?;
        let index = NullRewardIndex::<T>::get(netuid);
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

    /// Domain-separate work by subnet generation and recipient. A mempool observer
    /// cannot reuse another coldkey's seal to steal a miner's equal reward share.
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
        // Deliberately do not append to Yuma vectors, OwnedHotkeys, StakingHotkeys,
        // or the global Owner map. Null mining does not create a staking account.
        Self::enroll_null_miner(netuid, &hotkey, &coldkey)?;
        RegistrationsThisBlock::<T>::mutate(netuid, |n| *n = n.saturating_add(1));
        Ok(())
    }

    /// Budget is already minted into alpha-out. This reserves it, never mints it
    /// again. Q64 remainder is carried so small per-block rewards are not lost.
    // A u64 shifted by 64 fits U256; division follows the nonzero count check.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn accrue_null_rewards(netuid: NetUid, budget: AlphaBalance) {
        if budget.is_zero() {
            return;
        }
        let count = NullMinerCount::<T>::get(netuid);
        if count == 0 {
            Self::recycle_subnet_alpha(netuid, budget);
            return;
        }
        let scaled = (U256::from(budget.to_u64()) << FRACTION_BITS)
            .saturating_add(U256::from(NullRewardRemainder::<T>::get(netuid)));
        let divisor = U256::from(count);
        NullRewardIndex::<T>::mutate(netuid, |index| {
            *index = index.saturating_add(scaled / divisor)
        });
        NullRewardRemainder::<T>::insert(netuid, (scaled % divisor).low_u64());
        NullUnclaimedAlpha::<T>::mutate(netuid, |value| *value = value.saturating_add(budget));
    }

    /// Resolve at most 64 coldkey swaps. Longer histories are advanced by
    /// repeated calls, never by an unbounded walk or a population-wide rewrite.
    pub(crate) fn refresh_null_miner_owner(
        netuid: NetUid,
        hotkey: &T::AccountId,
        mut owner: T::AccountId,
        checkpoint: U256,
    ) -> (T::AccountId, bool) {
        let mut generation = NullMinerOwnerGeneration::<T>::get(netuid, hotkey);
        for _ in 0..64 {
            let Some((next, next_generation)) = NullColdkeySuccessor::<T>::get(&owner, generation)
            else {
                NullMiners::<T>::insert(netuid, hotkey, (&owner, checkpoint));
                NullMinerOwnerGeneration::<T>::insert(netuid, hotkey, generation);
                return (owner, true);
            };
            owner = next;
            generation = next_generation;
        }
        NullMiners::<T>::insert(netuid, hotkey, (&owner, checkpoint));
        NullMinerOwnerGeneration::<T>::insert(netuid, hotkey, generation);
        (owner, false)
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
        let earned = NullRewardIndex::<T>::get(netuid).saturating_sub(checkpoint) >> FRACTION_BITS;
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
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            stake_hotkey,
            coldkey,
            netuid,
            amount,
        );
        amount
    }

    // Keep the previous call's SCALE discriminant reserved. Validation rejects
    // it before any coldkey fee; null mode has no scores or scoring permissions.
    pub fn do_set_null_weights(
        origin: OriginFor<T>,
        _netuid: NetUid,
        _dests: Vec<u16>,
        _values: Vec<u32>,
        _version_key: u64,
    ) -> DispatchResult {
        ensure_signed(origin)?;
        Err(Error::<T>::NullConsensusHasNoWeights.into())
    }
    pub(crate) fn validate_null_weights(
        _hotkey: &T::AccountId,
        _netuid: NetUid,
        _dests: &[u16],
        _values: &[u32],
        _version_key: u64,
    ) -> Result<u16, Error<T>> {
        Err(Error::<T>::NullConsensusHasNoWeights)
    }
}
