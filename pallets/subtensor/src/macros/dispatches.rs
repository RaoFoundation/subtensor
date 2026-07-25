#![allow(clippy::crate_in_macro_def)]
use frame_support::pallet_macros::pallet_section;

/// A [`pallet_section`] that defines SubtensorModule dispatchables (extrinsics).
///
/// Imported into the pallet via [`import_section`]. Call names and `call_index`
/// values are frozen for client/SDK compatibility — edit definition-site docs only.
#[pallet_section]
mod dispatches {
    use frame_support::pallet_prelude::DispatchResultWithPostInfo;
    use frame_support::traits::schedule::v3::Anon as ScheduleAnon;
    use frame_system::pallet_prelude::BlockNumberFor;
    use sp_core::ecdsa::Signature;
    use sp_runtime::{Percent, Saturating, traits::Hash};

    use crate::MAX_CRV3_COMMIT_SIZE_BYTES;
    use crate::MAX_NUM_ROOT_CLAIMS;
    use crate::MAX_ROOT_CLAIM_THRESHOLD;
    use crate::MAX_SUBNET_CLAIMS;

    /// SubtensorModule extrinsics: staking, weights, registration, identity, admin, and related state changes.
    ///
    /// Each call is weight-annotated and returns `DispatchResult` / `DispatchResultWithPostInfo`.
    /// Prefer the opening doc sentence for origin, units, and invariants agents cannot infer from the signature.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Sets validator weights on a subnet (hotkey origin); rejected when commit-reveal is enabled for `netuid`.
        ///
        /// Stored weights are u16s max-upscaled so the largest non-zero input becomes `u16::MAX`.
        /// Inputs are relative and need not sum to a fixed total. Hotkey-signed, so treat as
        /// low-trust until observed on-chain.
        ///
        /// # Note
        /// Input weights are relative. They do not need to sum to a particular
        /// value before submission.
        ///
        /// # Arguments
        /// * `origin`: The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuid`: The network uid we are setting these weights on.
        ///
        /// * `dests`: The edge endpoint for the weight, i.e. j for w_ij.
        ///
        /// * `weights`: Relative u16-encoded weights, max-upscaled by the pallet before storage.
        ///
        /// * `version_key`: The network version key to check if the validator is up to date.
        ///
        /// # Events
        /// * `WeightsSet`: On successfully setting the weights on chain.
        ///
        /// # Errors
        /// * `MechanismDoesNotExist`: Attempting to set weights on a non-existent network.
        ///
        /// * `NotRegistered`: Attempting to set weights from a non registered account.
        ///
        /// * `WeightVecNotEqualSize`: Attempting to set weights with uids not of same length.
        ///
        /// * `DuplicateUids`: Attempting to set weights with duplicate uids.
        ///
        /// * `UidsLengthExceedUidsInSubNet`: Attempting to set weights above the max allowed uids.
        ///
        /// * `UidVecContainInvalidOne`: Attempting to set weights with invalid uids.
        ///
        /// * `WeightVecLengthIsLow`: Attempting to set weights with fewer weights than min.
        ///
        /// * `MaxWeightExceeded`: Attempting to set weights with max value exceeding limit.
        #[pallet::call_index(0)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::set_weights(), DispatchClass::Normal, Pays::No))]
        pub fn set_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            dests: Vec<u16>,
            weights: Vec<u16>,
            version_key: u64,
        ) -> DispatchResult {
            if Self::get_commit_reveal_weights_enabled(netuid) {
                Err(Error::<T>::CommitRevealEnabled.into())
            } else {
                Self::do_set_weights(origin, netuid, dests, weights, version_key)
            }
        }

        /// Sets validator weights for mechanism `mecid` on `netuid` (hotkey origin); rejected when commit-reveal is enabled.
        ///
        /// Same storage/upscaling rules as `set_weights`, scoped to a single mechanism id.
        ///
        /// # Note
        /// Input weights are relative. They do not need to sum to a particular
        /// value before submission.
        ///
        /// # Arguments
        /// * `origin`: The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuid`: The network uid we are setting these weights on.
        ///
        /// * `mecid`: The u8 mechnism identifier.
        ///
        /// * `dests`: The edge endpoint for the weight, i.e. j for w_ij.
        ///
        /// * `weights`: Relative u16-encoded weights, max-upscaled by the pallet before storage.
        ///
        /// * `version_key`: The network version key to check if the validator is up to date.
        ///
        /// # Events
        /// * `WeightsSet`: On successfully setting the weights on chain.
        ///
        /// # Errors
        /// * `MechanismDoesNotExist`: Attempting to set weights on a non-existent network.
        ///
        /// * `NotRegistered`: Attempting to set weights from a non registered account.
        ///
        /// * `WeightVecNotEqualSize`: Attempting to set weights with uids not of same length.
        ///
        /// * `DuplicateUids`: Attempting to set weights with duplicate uids.
        ///
        /// * `UidsLengthExceedUidsInSubNet`: Attempting to set weights above the max allowed uids.
        ///
        /// * `UidVecContainInvalidOne`: Attempting to set weights with invalid uids.
        ///
        /// * `WeightVecLengthIsLow`: Attempting to set weights with fewer weights than min.
        ///
        /// * `MaxWeightExceeded`: Attempting to set weights with max value exceeding limit.
        #[pallet::call_index(119)]
        #[pallet::weight((<T as Config>::WeightInfo::set_mechanism_weights(dests.len() as u32), DispatchClass::Normal, Pays::No))]
        pub fn set_mechanism_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            dests: Vec<u16>,
            weights: Vec<u16>,
            version_key: u64,
        ) -> DispatchResult {
            if Self::get_commit_reveal_weights_enabled(netuid) {
                Err(Error::<T>::CommitRevealEnabled.into())
            } else {
                Self::do_set_mechanism_weights(origin, netuid, mecid, dests, weights, version_key)
            }
        }

        /// Batch `set_weights` across many netuids in one extrinsic; per-item failures emit batch error events without reverting the whole call.
        ///
        /// # Arguments
        /// * `origin`: The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuids`: The network uids we are setting these weights on.
        ///
        /// * `weights`: The weights to set for each network. [(uid, weight), ...].
        ///
        /// * `version_keys`: The network version keys to check if the validator is up to date.
        ///
        /// # Events
        /// * `WeightsSet`: On successfully setting the weights on chain.
        /// * `BatchWeightsCompleted`: On success of the batch.
        /// * `BatchCompletedWithErrors`: On failure of any of the weights in the batch.
        /// * `BatchWeightItemFailed`: On failure for each failed item in the batch.
        ///
        #[pallet::call_index(80)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::batch_set_weights(), DispatchClass::Normal, Pays::No))]
        pub fn batch_set_weights(
            origin: OriginFor<T>,
            netuids: Vec<Compact<NetUid>>,
            weights: Vec<Vec<(Compact<u16>, Compact<u16>)>>,
            version_keys: Vec<Compact<u64>>,
        ) -> DispatchResult {
            Self::do_batch_set_weights(origin, netuids, weights, version_keys)
        }

        /// Commits a Blake2 hash of upcoming weights for later `reveal_weights` (hotkey origin; commit-reveal must be enabled).
        ///
        /// # Arguments
        /// * `origin`: The signature of the committing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `commit_hash`: The hash representing the committed weights.
        ///
        /// # Errors
        /// * `CommitRevealDisabled`: Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`: Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        ///
        #[pallet::call_index(96)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::commit_weights(), DispatchClass::Normal, Pays::No))]
        pub fn commit_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            commit_hash: H256,
        ) -> DispatchResult {
            Self::do_commit_weights(origin, netuid, commit_hash)
        }

        /// Commits a weight hash for mechanism `mecid` on `netuid` for later `reveal_mechanism_weights`.
        ///
        /// # Arguments
        /// * `origin`: The signature of the committing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `mecid`: The u8 mechanism identifier.
        ///
        /// * `commit_hash`: The hash representing the committed weights.
        ///
        /// # Errors
        /// * `CommitRevealDisabled`: Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`: Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        ///
        #[pallet::call_index(115)]
        #[pallet::weight((<T as Config>::WeightInfo::commit_mechanism_weights(), DispatchClass::Normal, Pays::No))]
        pub fn commit_mechanism_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            commit_hash: H256,
        ) -> DispatchResult {
            Self::do_commit_mechanism_weights(origin, netuid, mecid, commit_hash)
        }

        /// Batch `commit_weights` across many netuids; individual commit failures are reported via batch events.
        ///
        /// # Arguments
        /// * `origin`: The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuids`: The network uids we are setting these weights on.
        ///
        /// * `commit_hashes`: The commit hashes to commit.
        ///
        /// # Events
        /// * `WeightsSet`: On successfully setting the weights on chain.
        /// * `BatchWeightsCompleted`: On success of the batch.
        /// * `BatchCompletedWithErrors`: On failure of any of the weights in the batch.
        /// * `BatchWeightItemFailed`: On failure for each failed item in the batch.
        ///
        #[pallet::call_index(100)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::batch_commit_weights(), DispatchClass::Normal, Pays::No))]
        pub fn batch_commit_weights(
            origin: OriginFor<T>,
            netuids: Vec<Compact<NetUid>>,
            commit_hashes: Vec<H256>,
        ) -> DispatchResult {
            Self::do_batch_commit_weights(origin, netuids, commit_hashes)
        }

        /// Reveals previously `commit_weights`-hashed weights (hotkey origin); hash must match `uids`/`values`/`salt`/`version_key`.
        ///
        /// # Arguments
        /// * `origin`: The signature of the revealing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `uids`: The uids for the weights being revealed.
        ///
        /// * `values`: The values of the weights being revealed.
        ///
        /// * `salt`: The salt used to generate the commit hash.
        ///
        /// * `version_key`: The network version key.
        ///
        /// # Errors
        /// * `CommitRevealDisabled`: Attempting to reveal weights when the commit-reveal mechanism is disabled.
        ///
        /// * `NoWeightsCommitFound`: Attempting to reveal weights without an existing commit.
        ///
        /// * `ExpiredWeightCommit`: Attempting to reveal a weight commit that has expired.
        ///
        /// * `RevealTooEarly`: Attempting to reveal weights outside the valid reveal period.
        ///
        /// * `InvalidRevealCommitHashNotMatch`: The revealed hash does not match any committed hash.
        ///
        #[pallet::call_index(97)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::reveal_weights(), DispatchClass::Normal, Pays::No))]
        pub fn reveal_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            uids: Vec<u16>,
            values: Vec<u16>,
            salt: Vec<u16>,
            version_key: u64,
        ) -> DispatchResult {
            Self::do_reveal_weights(origin, netuid, uids, values, salt, version_key)
        }

        /// Reveals mechanism-scoped weights committed via `commit_mechanism_weights` (hotkey origin).
        ///
        /// # Arguments
        /// * `origin`: The signature of the revealing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `mecid`: The u8 mechanism identifier.
        ///
        /// * `uids`: The uids for the weights being revealed.
        ///
        /// * `values`: The values of the weights being revealed.
        ///
        /// * `salt`: The salt used to generate the commit hash.
        ///
        /// * `version_key`: The network version key.
        ///
        /// # Errors
        /// * `CommitRevealDisabled`: Attempting to reveal weights when the commit-reveal mechanism is disabled.
        ///
        /// * `NoWeightsCommitFound`: Attempting to reveal weights without an existing commit.
        ///
        /// * `ExpiredWeightCommit`: Attempting to reveal a weight commit that has expired.
        ///
        /// * `RevealTooEarly`: Attempting to reveal weights outside the valid reveal period.
        ///
        /// * `InvalidRevealCommitHashNotMatch`: The revealed hash does not match any committed hash.
        ///
        #[pallet::call_index(116)]
        #[pallet::weight((<T as Config>::WeightInfo::reveal_mechanism_weights(uids.len() as u32), DispatchClass::Normal, Pays::No))]
        pub fn reveal_mechanism_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            uids: Vec<u16>,
            values: Vec<u16>,
            salt: Vec<u16>,
            version_key: u64,
        ) -> DispatchResult {
            Self::do_reveal_mechanism_weights(
                origin,
                netuid,
                mecid,
                uids,
                values,
                salt,
                version_key,
            )
        }

        // Used to commit encrypted commit-reveal v3 weight values to later be revealed.
        //
        // # Arguments
        // * `origin`: The committing hotkey.
        //
        // * `netuid`: The u16 network identifier.
        //
        // * `commit`: The encrypted compressed commit.
        //     The steps for this are:
        //     1. Instantiate [`WeightsTlockPayload`]
        //     2. Serialize it using the `parity_scale_codec::Encode` trait
        //     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        //        to produce a [`TLECiphertext<TinyBLS381>`] type.
        //     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        //
        // * `reveal_round`: The drand reveal round which will be avaliable during epoch `n+1` from the current
        //      epoch.
        //
        // # Errors
        // * `CommitRevealV3Disabled`: Attempting to commit when the commit-reveal mechanism is disabled.
        //
        // * `TooManyUnrevealedCommits`: Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        //
        // #[pallet::call_index(99)]
        // #[pallet::weight((Weight::from_parts(77_750_000, 0)
        // .saturating_add(T::DbWeight::get().reads(9_u64))
        // .saturating_add(T::DbWeight::get().writes(2)), DispatchClass::Normal, Pays::No))]
        // pub fn commit_crv3_weights(
        //     origin: T::RuntimeOrigin,
        //     netuid: NetUid,
        //     commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
        //     reveal_round: u64,
        // ) -> DispatchResult {
        //     Self::do_commit_timelocked_weights(origin, netuid, commit, reveal_round, 4)
        // }

        /// Commits CRV3 timelock-encrypted weights for mechanism `mecid` (hotkey origin; payload size capped by `MAX_CRV3_COMMIT_SIZE_BYTES`).
        ///
        /// # Arguments
        /// * `origin`: The committing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `mecid`: The u8 mechanism identifier.
        ///
        /// * `commit`: The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * `reveal_round`: The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// # Errors
        /// * `CommitRevealV3Disabled`: Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`: Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        ///
        #[pallet::call_index(117)]
        #[pallet::weight((<T as Config>::WeightInfo::commit_crv3_mechanism_weights(), DispatchClass::Normal, Pays::No))]
        pub fn commit_crv3_mechanism_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
            reveal_round: u64,
        ) -> DispatchResult {
            Self::do_commit_timelocked_mechanism_weights(
                origin,
                netuid,
                mecid,
                commit,
                reveal_round,
                4,
            )
        }

        /// Batch `reveal_weights` across many commits; per-item failures emit batch events without reverting successes.
        ///
        /// # Arguments
        /// * `origin`: The signature of the revealing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `uids_list`: A list of uids for each set of weights being revealed.
        ///
        /// * `values_list`: A list of values for each set of weights being revealed.
        ///
        /// * `salts_list`: A list of salts used to generate the commit hashes.
        ///
        /// * `version_keys`: A list of network version keys.
        ///
        /// # Errors
        /// * `CommitRevealDisabled`: Attempting to reveal weights when the commit-reveal mechanism is disabled.
        ///
        /// * `NoWeightsCommitFound`: Attempting to reveal weights without an existing commit.
        ///
        /// * `ExpiredWeightCommit`: Attempting to reveal a weight commit that has expired.
        ///
        /// * `RevealTooEarly`: Attempting to reveal weights outside the valid reveal period.
        ///
        /// * `InvalidRevealCommitHashNotMatch`: The revealed hash does not match any committed hash.
        ///
        /// * `InvalidInputLengths`: The input vectors are of mismatched lengths.
        #[pallet::call_index(98)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::batch_reveal_weights(), DispatchClass::Normal, Pays::No))]
        pub fn batch_reveal_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            uids_list: Vec<Vec<u16>>,
            values_list: Vec<Vec<u16>>,
            salts_list: Vec<Vec<u16>>,
            version_keys: Vec<u64>,
        ) -> DispatchResult {
            Self::do_batch_reveal_weights(
                origin,
                netuid,
                uids_list,
                values_list,
                salts_list,
                version_keys,
            )
        }

        /// Decreases a delegate's take (hotkey origin); not rate-limited unlike `increase_take`.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The hotkey we are delegating (must be owned by the coldkey).
        ///
        /// * `netuid`: Subnet ID to decrease take for.
        ///
        /// * `take`: The new stake proportion that this hotkey takes from delegations.
        ///        The new value can be between 0 and 11_796 parts and should be strictly
        ///        lower than the previous value. If T is the new value (rational number),
        ///        the the parameter is calculated as [65535 * T]. For example, 1% would be
        ///        [0.01 * 65535] = [655.35] = 655
        ///
        /// # Events
        /// * `TakeDecreased`: On successfully setting a decreased take for this hotkey.
        ///
        /// # Errors
        /// * `NotRegistered`: The hotkey we are delegating is not registered on the network.
        ///
        /// * `NonAssociatedColdKey`: The hotkey we are delegating is not owned by the calling coldkey.
        ///
        /// * `DelegateTakeTooLow`: The delegate is setting a take which is not lower than the previous.
        ///
        #[pallet::call_index(65)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::decrease_take(), DispatchClass::Normal, Pays::Yes))]
        pub fn decrease_take(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            take: PerU16,
        ) -> DispatchResult {
            Self::do_decrease_take(origin, hotkey, take)
        }

        /// Increases a delegate's take (hotkey origin); rate-limited and bounded by max take.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The hotkey we are delegating (must be owned by the coldkey).
        ///
        /// * `take`: The new stake proportion that this hotkey takes from delegations.
        ///        The new value can be between 0 and 11_796 parts and should be strictly
        ///        greater than the previous value. T is the new value (rational number),
        ///        the the parameter is calculated as [65535 * T]. For example, 1% would be
        ///        [0.01 * 65535] = [655.35] = 655
        ///
        /// # Events
        /// * `TakeIncreased`: On successfully setting a increased take for this hotkey.
        ///
        /// # Errors
        /// * `NotRegistered`: The hotkey we are delegating is not registered on the network.
        ///
        /// * `NonAssociatedColdKey`: The hotkey we are delegating is not owned by the calling coldkey.
        ///
        /// * `DelegateTakeTooHigh`: The delegate is setting a take which is not greater than the previous.
        ///
        #[pallet::call_index(66)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::increase_take(), DispatchClass::Normal, Pays::Yes))]
        pub fn increase_take(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            take: PerU16,
        ) -> DispatchResult {
            Self::do_increase_take(origin, hotkey, take)
        }

        /// Stakes TAO from the signing coldkey onto `hotkey` for `netuid` (buys alpha via the subnet AMM).
        ///
        /// `amount_staked` is TAO (Rao). Coldkey may own the hotkey (self-stake) or delegate to another.
        ///
        /// # Note
        /// The coldkey account may own the hotkey, in which case they are
        /// delegating to themselves.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The associated hotkey account.
        ///
        /// * `netuid`: Subnetwork UID.
        ///
        /// * `amount_staked`: The amount of stake to be added to the hotkey staking account.
        ///
        /// # Events
        /// * `StakeAdded`: On the successfully adding stake to a global account.
        ///
        /// # Errors
        /// * `NotEnoughBalanceToStake`: Not enough balance on the coldkey to add onto the global account.
        ///
        /// * `NonAssociatedColdKey`: The calling coldkey is not associated with this hotkey.
        ///
        /// * `BalanceWithdrawalError`: Errors stemming from transaction pallet.
        ///
        #[pallet::call_index(2)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::add_stake())]
        pub fn add_stake(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            amount_staked: TaoBalance,
        ) -> DispatchResult {
            Self::do_add_stake(origin, hotkey, netuid, amount_staked).map(|_| ())
        }

        /// Unstakes alpha from `(coldkey, hotkey, netuid)` back to TAO on the signing coldkey (AMM sell).
        ///
        /// Only the coldkey associated with the hotkey may withdraw; `amount_unstaked` is alpha.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The associated hotkey account.
        ///
        /// * `netuid`: Subnetwork UID.
        ///
        /// * `amount_unstaked`: The amount of stake to be added to the hotkey staking account.
        ///
        /// # Events
        /// * `StakeRemoved`: On the successfully removing stake from the hotkey account.
        ///
        /// # Errors
        /// * `NotRegistered`: Thrown if the account we are attempting to unstake from is non existent.
        ///
        /// * `NonAssociatedColdKey`: Thrown if the coldkey does not own the hotkey we are unstaking from.
        ///
        /// * `NotEnoughStakeToWithdraw`: Thrown if there is not enough stake on the hotkey to withdwraw this amount.
        ///
        #[pallet::call_index(3)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::remove_stake())]
        pub fn remove_stake(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            amount_unstaked: AlphaBalance,
        ) -> DispatchResult {
            Self::do_remove_stake(origin, hotkey, netuid, amount_unstaked)
        }

        /// Serves or updates axon endpoint metadata for a registered neuron (hotkey origin); errors with `NotRegistered` if absent.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `version`: The bittensor version identifier.
        ///
        /// * `ip`: The endpoint ip information as a u128 encoded integer.
        ///
        /// * `port`: The endpoint port information as a u16 encoded integer.
        ///
        /// * `ip_type`: The endpoint ip version as a u8, 4 or 6.
        ///
        /// * `protocol`: UDP:1 or TCP:0.
        ///
        /// * `placeholder1`: Placeholder for further extra params.
        ///
        /// * `placeholder2`: Placeholder for further extra params.
        ///
        /// # Events
        /// * `AxonServed`: On successfully serving the axon info.
        ///
        /// # Errors
        /// * `MechanismDoesNotExist`: Attempting to set weights on a non-existent network.
        ///
        /// * `NotRegistered`: Attempting to set weights from a non registered account.
        ///
        /// * `InvalidIpType`: The ip type is not 4 or 6.
        ///
        /// * `InvalidIpAddress`: The numerically encoded ip address does not resolve to a proper ip.
        ///
        /// * `ServingRateLimitExceeded`: Attempting to set prometheus information withing the rate limit min.
        ///
        #[pallet::call_index(4)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::serve_axon(), DispatchClass::Normal, Pays::No))]
        pub fn serve_axon(
            origin: OriginFor<T>,
            netuid: NetUid,
            version: u32,
            ip: u128,
            port: u16,
            ip_type: u8,
            protocol: u8,
            placeholder1: u8,
            placeholder2: u8,
        ) -> DispatchResult {
            Self::do_serve_axon(
                origin,
                netuid,
                version,
                ip,
                port,
                ip_type,
                protocol,
                placeholder1,
                placeholder2,
                None,
            )
        }

        /// Like `serve_axon`, but also accepts an optional TLS certificate for inter-neuron communication (hotkey origin).
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `version`: The bittensor version identifier.
        ///
        /// * `ip`: The endpoint ip information as a u128 encoded integer.
        ///
        /// * `port`: The endpoint port information as a u16 encoded integer.
        ///
        /// * `ip_type`: The endpoint ip version as a u8, 4 or 6.
        ///
        /// * `protocol`: UDP:1 or TCP:0.
        ///
        /// * `placeholder1`: Placeholder for further extra params.
        ///
        /// * `placeholder2`: Placeholder for further extra params.
        ///
        /// * `certificate`: TLS certificate for inter neuron communitation.
        ///
        /// # Events
        /// * `AxonServed`: On successfully serving the axon info.
        ///
        /// # Errors
        /// * `MechanismDoesNotExist`: Attempting to set weights on a non-existent network.
        ///
        /// * `NotRegistered`: Attempting to set weights from a non registered account.
        ///
        /// * `InvalidIpType`: The ip type is not 4 or 6.
        ///
        /// * `InvalidIpAddress`: The numerically encoded ip address does not resolve to a proper ip.
        ///
        /// * `ServingRateLimitExceeded`: Attempting to set prometheus information withing the rate limit min.
        ///
        #[pallet::call_index(40)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::serve_axon_tls(), DispatchClass::Normal, Pays::No))]
        pub fn serve_axon_tls(
            origin: OriginFor<T>,
            netuid: NetUid,
            version: u32,
            ip: u128,
            port: u16,
            ip_type: u8,
            protocol: u8,
            placeholder1: u8,
            placeholder2: u8,
            certificate: Vec<u8>,
        ) -> DispatchResult {
            Self::do_serve_axon(
                origin,
                netuid,
                version,
                ip,
                port,
                ip_type,
                protocol,
                placeholder1,
                placeholder2,
                Some(certificate),
            )
        }

        /// Publishes Prometheus scrape endpoint metadata for a registered neuron (hotkey origin).
        ///
        /// # Arguments
        /// * `origin`: Hotkey of the neuron.
        /// * `netuid`: Subnet of the neuron.
        /// * `version`: Protocol/version identifier.
        /// * `ip`: Endpoint IP as a `u128`.
        /// * `port`: Scrape port.
        /// * `ip_type`: `4` or `6`.
        #[pallet::call_index(5)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::serve_prometheus(), DispatchClass::Normal, Pays::No))]
        pub fn serve_prometheus(
            origin: OriginFor<T>,
            netuid: NetUid,
            version: u32,
            ip: u128,
            port: u16,
            ip_type: u8,
        ) -> DispatchResult {
            Self::do_serve_prometheus(origin, netuid, version, ip, port, ip_type)
        }

        /// Legacy registration entrypoint; PoW args (`_block_number`/`_nonce`/`_work`/`_coldkey`) are unused and ignored.
        ///
        /// # Arguments
        /// * `origin`: The signature of the calling hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `block_number`: Block hash used to prove work done.
        ///
        /// * `nonce`: Positive integer nonce used in POW.
        ///
        /// * `work`: Vector encoded bytes representing work done.
        ///
        /// * `hotkey`: Hotkey to be registered to the network.
        ///
        /// * `coldkey`: Associated coldkey account.
        ///
        /// # Events
        /// * `NeuronRegistered`: On successfully registering a uid to a neuron slot on a subnetwork.
        ///
        /// # Errors
        /// * `MechanismDoesNotExist`: Attempting to register to a non existent network.
        ///
        /// * `TooManyRegistrationsThisBlock`: This registration exceeds the total allowed on this network this block.
        ///
        /// * `HotKeyAlreadyRegisteredInSubNet`: The hotkey is already registered on this network.
        ///
        /// * `InvalidWorkBlock`: The work has been performed on a stale, future, or non existent block.
        ///
        /// * `InvalidDifficulty`: The work does not match the difficulty.
        ///
        /// * `InvalidSeal`: The seal is incorrect.
        ///
        #[pallet::call_index(6)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register())]
        pub fn register(
            origin: OriginFor<T>,
            netuid: NetUid,
            _block_number: u64,
            _nonce: u64,
            _work: Vec<u8>,
            hotkey: T::AccountId,
            _coldkey: T::AccountId,
        ) -> DispatchResult {
            Self::do_register(origin, netuid, hotkey)
        }

        /// Registers `hotkey` on the root subnet; origin must be the owning coldkey.
        #[pallet::call_index(62)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::root_register())]
        pub fn root_register(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_root_register(origin, hotkey)
        }

        /// Registers `hotkey` on an existing subnet by paying the current burn (not PoW); coldkey origin.
        #[pallet::call_index(7)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::burned_register())]
        pub fn burned_register(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
        ) -> DispatchResult {
            Self::do_register(origin, netuid, hotkey)
        }

        /// Deprecated: swaps `hotkey` → `new_hotkey` on one subnet or all (`netuid = None`); coldkey origin, `keep_stake = false`.
        ///
        /// Prefer `swap_hotkey_v2`. Retained for call-index / client compatibility until removed after June 2026.
        ///
        /// # Arguments
        /// * `origin`: Coldkey that owns `hotkey`.
        /// * `hotkey`: Existing hotkey to replace.
        /// * `new_hotkey`: Replacement hotkey.
        /// * `netuid`: `Some` for one subnet; `None` for all subnets.
        #[deprecated(
            note = "Please use swap_hotkey_v2 instead. This extrinsic will be removed some time after June 2026."
        )]
        #[pallet::call_index(70)]
        #[pallet::weight((
            <T as crate::pallet::Config>::WeightInfo::swap_hotkey(),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        pub fn swap_hotkey(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            new_hotkey: T::AccountId,
            netuid: Option<NetUid>,
        ) -> DispatchResultWithPostInfo {
            Self::do_swap_hotkey(origin, &hotkey, &new_hotkey, netuid, false)
        }

        /// Swaps `hotkey` → `new_hotkey` on one subnet or all; coldkey origin.
        ///
        /// When `keep_stake` is true, stake stays on the old hotkey while other metadata moves
        /// (used when promoting a root key to a child key). Prefer this over deprecated `swap_hotkey`.
        ///
        /// # Arguments
        /// * `origin`: Coldkey that owns `hotkey`.
        /// * `hotkey`: Existing hotkey to replace.
        /// * `new_hotkey`: Replacement hotkey.
        /// * `netuid`: `Some` for one subnet; `None` for all subnets.
        /// * `keep_stake`: If true, leave stake on `hotkey` and transfer remaining metadata only.
        #[allow(unknown_lints, benchmarked_weight_not_plugged)]
        #[pallet::call_index(72)]
        #[pallet::weight((
            crate::Pallet::<T>::swap_hotkey_v2_dispatch_weight(netuid, *keep_stake),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        pub fn swap_hotkey_v2(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            new_hotkey: T::AccountId,
            netuid: Option<NetUid>,
            keep_stake: bool,
        ) -> DispatchResultWithPostInfo {
            Self::do_swap_hotkey(origin, &hotkey, &new_hotkey, netuid, keep_stake)
        }

        /// Root-only immediate coldkey swap (`old_coldkey` → `new_coldkey`) without announcement; optionally charges `swap_cost` TAO.
        ///
        /// Also clears any pending coldkey-swap announcement/dispute for `old_coldkey`.
        #[pallet::call_index(71)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::swap_coldkey())]
        pub fn swap_coldkey(
            origin: OriginFor<T>,
            old_coldkey: T::AccountId,
            new_coldkey: T::AccountId,
            swap_cost: TaoBalance,
        ) -> DispatchResult {
            ensure_root(origin)?;

            if !swap_cost.is_zero() {
                Self::charge_swap_cost(&old_coldkey, swap_cost)?;
            }
            Self::do_swap_coldkey(&old_coldkey, &new_coldkey)?;

            // We also clear any announcement or dispute for security reasons
            ColdkeySwapAnnouncements::<T>::remove(&old_coldkey);
            ColdkeySwapDisputes::<T>::remove(old_coldkey);

            Ok(())
        }

        /// Sets per-subnet childkey take for `hotkey` (`PerU16`, 65535 = 100%); coldkey origin, rate-limited.
        ///
        /// This function allows a coldkey to set the childkey take for a given hotkey.
        /// The childkey take determines the proportion of stake that the hotkey keeps for itself
        /// when distributing stake to its children.
        ///
        /// # Arguments
        /// * `origin`: The signature of the calling coldkey. Setting childkey take can only be done by the coldkey.
        ///
        /// * `hotkey`: The hotkey for which the childkey take will be set.
        ///
        /// * `take`: The new childkey take value. This is a ratio represented in parts per 65535,
        ///       where 65535 represents 100%.
        ///
        /// # Events
        /// * `ChildkeyTakeSet`: On successfully setting the childkey take for a hotkey.
        ///
        /// # Errors
        /// * `NonAssociatedColdKey`: The coldkey does not own the hotkey.
        /// * `InvalidChildkeyTake`: The provided take value is invalid (greater than the maximum allowed take).
        /// * `TxChildkeyTakeRateLimitExceeded`: The rate limit for changing childkey take has been exceeded.
        ///
        #[pallet::call_index(75)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_childkey_take())]
        pub fn set_childkey_take(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            take: PerU16,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;

            // Call the utility function to set the childkey take
            Self::do_set_childkey_take(coldkey, hotkey, netuid, take)
        }

        // ---- SUDO ONLY FUNCTIONS ------------------------------------------------------------

        /// Root-only: sets blocks between childkey-take updates (`tx_rate_limit`).
        ///
        /// This function can only be called by the root origin.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, must be root.
        /// * `tx_rate_limit`: The new rate limit in blocks.
        ///
        /// # Errors
        /// * `BadOrigin`: If the origin is not root.
        ///
        #[pallet::call_index(69)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::sudo_set_tx_childkey_take_rate_limit())]
        pub fn sudo_set_tx_childkey_take_rate_limit(
            origin: OriginFor<T>,
            tx_rate_limit: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::set_tx_childkey_take_rate_limit(tx_rate_limit);
            Ok(())
        }

        /// Root-only: sets the global minimum childkey take (`PerU16`).
        ///
        /// This function can only be called by the root origin.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, must be root.
        /// * `take`: The new minimum childkey take value.
        ///
        /// # Errors
        /// * `BadOrigin`: If the origin is not root.
        ///
        #[pallet::call_index(76)]
        #[pallet::weight(<T as Config>::WeightInfo::sudo_set_min_childkey_take())]
        pub fn sudo_set_min_childkey_take(origin: OriginFor<T>, take: PerU16) -> DispatchResult {
            ensure_root(origin)?;
            Self::set_min_childkey_take(take);
            Ok(())
        }

        /// Root-only: sets the global maximum childkey take (`PerU16`).
        ///
        /// This function can only be called by the root origin.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, must be root.
        /// * `take`: The new maximum childkey take value.
        ///
        /// # Errors
        /// * `BadOrigin`: If the origin is not root.
        ///
        #[pallet::call_index(77)]
        #[pallet::weight(<T as Config>::WeightInfo::sudo_set_max_childkey_take())]
        pub fn sudo_set_max_childkey_take(origin: OriginFor<T>, take: PerU16) -> DispatchResult {
            ensure_root(origin)?;
            Self::set_max_childkey_take(take);
            Ok(())
        }

        /// Creates a new subnet; signing coldkey becomes owner and pays the network lock cost, associating `hotkey`.
        #[pallet::call_index(59)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register_network())]
        pub fn register_network(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_register_network(origin, &hotkey, 1, None)
        }

        /// Test-only PoW faucet mint (`pow-faucet` feature); unavailable / errors with `FaucetDisabled` in production builds.
        #[pallet::call_index(60)]
        #[pallet::weight((Weight::from_parts(91_000_000, 0)
        .saturating_add(T::DbWeight::get().reads(27))
		.saturating_add(T::DbWeight::get().writes(22)), DispatchClass::Normal, Pays::No))]
        #[cfg(feature = "pow-faucet")]
        pub fn faucet(
            origin: OriginFor<T>,
            block_number: u64,
            nonce: u64,
            work: Vec<u8>,
        ) -> DispatchResult {
            if cfg!(feature = "pow-faucet") {
                return Self::do_faucet(origin, block_number, nonce, work);
            }

            Err(Error::<T>::FaucetDisabled.into())
        }

        /// Root-only dissolve of subnet `netuid`. The `_coldkey` argument is unused (kept for call-signature stability).
        #[pallet::call_index(61)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::dissolve_network())]
        pub fn dissolve_network(
            origin: OriginFor<T>,
            _coldkey: T::AccountId,
            netuid: NetUid,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_dissolve_network(netuid)
        }

        /// Sets the childkey proportion map for `hotkey` on `netuid` (coldkey origin); root subnet disallowed.
        ///
        /// This function allows a coldkey to set a single child for a given hotkey on a specified network.
        /// The proportion of the hotkey's stake to be allocated to the child is also specified.
        ///
        /// # Arguments
        /// * `origin`: The signature of the calling coldkey. Setting a hotkey child can only be done by the coldkey.
        ///
        /// * `hotkey`: The hotkey which will be assigned the child.
        ///
        /// * `child`: The child which will be assigned to the hotkey.
        ///
        /// * `netuid`: The u16 network identifier where the childkey will exist.
        ///
        /// * `proportion`: Proportion of the hotkey's stake to be given to the child, the value must be u64 normalized.
        ///
        /// # Events
        /// * `ChildAddedSingular`: On successfully registering a child to a hotkey.
        ///
        /// # Errors
        /// * `MechanismDoesNotExist`: Attempting to register to a non-existent network.
        /// * `RegistrationNotPermittedOnRootSubnet`: Attempting to register a child on the root network.
        /// * `NonAssociatedColdKey`: The coldkey does not own the hotkey or the child is the same as the hotkey.
        /// * `HotKeyAccountNotExists`: The hotkey account does not exist.
        ///
        /// # Note
        /// 1. **Signature Verification**: Ensures that the caller has signed the transaction, verifying the coldkey.
        /// 2. **Root Network Check**: Ensures that the delegation is not on the root network, as child hotkeys are not valid on the root.
        /// 3. **Network Existence Check**: Ensures that the specified network exists.
        /// 4. **Ownership Verification**: Ensures that the coldkey owns the hotkey.
        /// 5. **Hotkey Account Existence Check**: Ensures that the hotkey account already exists.
        /// 6. **Child-Hotkey Distinction**: Ensures that the child is not the same as the hotkey.
        /// 7. **Old Children Cleanup**: Removes the hotkey from the parent list of its old children.
        /// 8. **New Children Assignment**: Assigns the new child to the hotkey and updates the parent list for the new child.
        // TODO: Benchmark this call
        #[pallet::call_index(67)]
        #[pallet::weight((<T as Config>::WeightInfo::set_children(children.len() as u32), DispatchClass::Normal, Pays::Yes))]
        pub fn set_children(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            children: Vec<(u64, T::AccountId)>,
        ) -> DispatchResultWithPostInfo {
            Self::do_schedule_children(origin, hotkey, netuid, children)?;
            Ok(().into())
        }

        /// Deprecated: always returns `Error::Deprecated`. Use `announce_coldkey_swap` / `swap_coldkey_announced`.
        #[pallet::call_index(73)]
        #[pallet::weight(<T as Config>::WeightInfo::schedule_swap_coldkey())]
        #[deprecated(note = "Deprecated, please migrate to `announce_coldkey_swap`/`coldkey_swap`")]
        pub fn schedule_swap_coldkey(
            _origin: OriginFor<T>,
            _new_coldkey: T::AccountId,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// Sets coldkey chain-identity fields (name, url, github, image, discord, description, additional).
        ///
        /// Signed by the coldkey; distinct from axon/prometheus serving and from `set_subnet_identity`.
        ///
        /// # Arguments
        /// * `origin`: Coldkey whose identity is updated.
        /// * `name` / `url` / `github_repo` / `image` / `discord` / `description` / `additional`: UTF-8 byte fields.
        #[pallet::call_index(68)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_identity())]
        pub fn set_identity(
            origin: OriginFor<T>,
            name: Vec<u8>,
            url: Vec<u8>,
            github_repo: Vec<u8>,
            image: Vec<u8>,
            discord: Vec<u8>,
            description: Vec<u8>,
            additional: Vec<u8>,
        ) -> DispatchResult {
            Self::do_set_identity(
                origin,
                name,
                url,
                github_repo,
                image,
                discord,
                description,
                additional,
            )
        }

        /// Sets subnet-owner identity metadata for `netuid` (name, contacts, urls); coldkey must own the subnet.
        /// # Arguments
        /// * `origin`: The signature of the calling coldkey, which must be the owner of the subnet.
        ///
        /// * `netuid`: The unique network identifier of the subnet.
        ///
        /// * `subnet_name`: The name of the subnet.
        ///
        /// * `github_repo`: The GitHub repository associated with the subnet identity.
        ///
        /// * `subnet_contact`: The contact information for the subnet.
        #[pallet::call_index(78)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_subnet_identity())]
        pub fn set_subnet_identity(
            origin: OriginFor<T>,
            netuid: NetUid,
            subnet_name: Vec<u8>,
            github_repo: Vec<u8>,
            subnet_contact: Vec<u8>,
            subnet_url: Vec<u8>,
            discord: Vec<u8>,
            description: Vec<u8>,
            logo_url: Vec<u8>,
            additional: Vec<u8>,
        ) -> DispatchResult {
            Self::do_set_subnet_identity(
                origin,
                netuid,
                subnet_name,
                github_repo,
                subnet_contact,
                subnet_url,
                discord,
                description,
                logo_url,
                additional,
            )
        }

        /// Creates a new subnet like `register_network`, optionally attaching `SubnetIdentityOfV3` at registration.
        #[pallet::call_index(79)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register_network_with_identity())]
        pub fn register_network_with_identity(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            identity: Option<SubnetIdentityOfV3>,
        ) -> DispatchResult {
            Self::do_register_network(origin, &hotkey, 1, identity)
        }

        /// Unstakes all positions for `hotkey` across every subnet onto the signing coldkey (TAO proceeds).
        ///
        /// # Arguments
        /// * `origin`: Coldkey that owns `hotkey`.
        /// * `hotkey`: Hotkey whose stake is fully withdrawn.
        ///
        /// # Events
        /// * `StakeRemoved`: On successfully removing stake from the hotkey account.
        ///
        /// # Errors
        /// * `NotRegistered` / `NonAssociatedColdKey` / `NotEnoughStakeToWithdraw` / `TxRateLimitExceeded`.
        #[pallet::call_index(83)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::unstake_all())]
        pub fn unstake_all(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_unstake_all(origin, hotkey)
        }

        /// Unstakes all non-root alpha for `hotkey` across subnets onto the signing coldkey; root stake is left untouched.
        ///
        /// # Arguments
        /// * `origin`: Coldkey that owns `hotkey`.
        /// * `hotkey`: Hotkey whose alpha stake is fully withdrawn.
        ///
        /// # Events
        /// * `StakeRemoved`: On successfully removing stake from the hotkey account.
        ///
        /// # Errors
        /// * `NotRegistered` / `NonAssociatedColdKey` / `NotEnoughStakeToWithdraw` / `TxRateLimitExceeded`.
        #[pallet::call_index(84)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::unstake_all_alpha())]
        pub fn unstake_all_alpha(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_unstake_all_alpha(origin, hotkey)
        }

        /// Moves alpha stake between hotkeys and/or subnets while keeping the same coldkey ownership.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `origin_hotkey`: The hotkey account to move stake from.
        ///
        /// * `destination_hotkey`: The hotkey account to move stake to.
        ///
        /// * `origin_netuid`: The subnet ID to move stake from.
        ///
        /// * `destination_netuid`: The subnet ID to move stake to.
        ///
        /// * `alpha_amount`: The alpha stake amount to move.
        ///
        #[pallet::call_index(85)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::move_stake())]
        pub fn move_stake(
            origin: OriginFor<T>,
            origin_hotkey: T::AccountId,
            destination_hotkey: T::AccountId,
            origin_netuid: NetUid,
            destination_netuid: NetUid,
            alpha_amount: AlphaBalance,
        ) -> DispatchResult {
            Self::do_move_stake(
                origin,
                origin_hotkey,
                destination_hotkey,
                origin_netuid,
                destination_netuid,
                alpha_amount,
            )
        }

        /// Transfers alpha stake to another coldkey on the same hotkey, optionally across subnets.
        ///
        /// # Arguments
        /// * `origin`: The origin of the transaction, which must be signed by the `origin_coldkey`.
        /// * `destination_coldkey`: The coldkey to which the stake is transferred.
        /// * `hotkey`: The hotkey associated with the stake.
        /// * `origin_netuid`: The network/subnet ID to move stake from.
        /// * `destination_netuid`: The network/subnet ID to move stake to (for cross-subnet transfer).
        /// * `alpha_amount`: The amount of stake to transfer.
        ///
        /// # Errors
        /// * `BadOrigin`: The transaction is not signed.
        /// * `SubnetNotExists`: Either `origin_netuid` or `destination_netuid` does not exist.
        /// * `SubtokenDisabled`: The subtoken is disabled on the origin or destination subnet.
        /// * `HotKeyAccountNotExists`: The `hotkey` account does not exist.
        /// * `NotEnoughStakeToWithdraw`: The `(origin_coldkey, hotkey, origin_netuid)` position has less stake than `alpha_amount`.
        /// * `InsufficientLiquidity`: The swap simulation on the origin subnet fails.
        /// * `AmountTooLow`: The TAO-equivalent of the transfer is below the minimum stake requirement.
        /// * `TransferDisallowed`: Transfers are disabled on the origin or destination subnet.
        /// * `StakeUnavailable`: The remaining stake would not cover the locked amount on the origin subnet.
        ///
        /// # Events
        /// May emit a `StakeTransferred` event on success.
        #[pallet::call_index(86)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::transfer_stake())]
        pub fn transfer_stake(
            origin: OriginFor<T>,
            destination_coldkey: T::AccountId,
            hotkey: T::AccountId,
            origin_netuid: NetUid,
            destination_netuid: NetUid,
            alpha_amount: AlphaBalance,
        ) -> DispatchResult {
            Self::do_transfer_stake(
                origin,
                destination_coldkey,
                hotkey,
                origin_netuid,
                destination_netuid,
                alpha_amount,
            )
        }

        /// Swaps alpha stake from `origin_netuid` to `destination_netuid` for the same coldkey/hotkey (AMM path).
        ///
        /// # Arguments
        /// * `origin`: Coldkey that owns `hotkey`.
        /// * `hotkey`: Hotkey whose stake is swapped.
        /// * `origin_netuid` / `destination_netuid`: Distinct subnets.
        /// * `alpha_amount`: Alpha to sell on origin and rebuy on destination.
        ///
        /// # Errors
        /// * `BadOrigin` / `SameNetuid` / `SubnetNotExists` / `SubtokenDisabled` / `HotKeyAccountNotExists` /
        ///   `NotEnoughStakeToWithdraw` / `InsufficientLiquidity` / `AmountTooLow` / `StakeUnavailable`.
        ///
        /// # Events
        /// May emit a `StakeSwapped` event on success.
        #[pallet::call_index(87)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::swap_stake())]
        pub fn swap_stake(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            origin_netuid: NetUid,
            destination_netuid: NetUid,
            alpha_amount: AlphaBalance,
        ) -> DispatchResult {
            Self::do_swap_stake(
                origin,
                hotkey,
                origin_netuid,
                destination_netuid,
                alpha_amount,
            )
        }

        /// Like `add_stake`, but with alpha price limit (`limit_price` RAO/alpha) and optional partial fill.
        ///
        /// In case if slippage occurs and the price shall move beyond the limit
        /// price, the staking order may execute only partially or not execute
        /// at all.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The associated hotkey account.
        ///
        /// * `netuid`: Subnetwork UID.
        ///
        /// * `amount_staked`: The amount of stake to be added to the hotkey staking account.
        ///
        /// * `limit_price`: The limit price expressed in units of RAO per one Alpha.
        ///
        /// * `allow_partial`: Allows partial execution of the amount. If set to false, this becomes
        ///       fill or kill type of order.
        ///
        /// # Events
        /// * `StakeAdded`: On the successfully adding stake to a global account.
        ///
        /// # Errors
        /// * `NotEnoughBalanceToStake`: Not enough balance on the coldkey to add onto the global account.
        ///
        /// * `NonAssociatedColdKey`: The calling coldkey is not associated with this hotkey.
        ///
        /// * `BalanceWithdrawalError`: Errors stemming from transaction pallet.
        ///
        #[pallet::call_index(88)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::add_stake_limit())]
        pub fn add_stake_limit(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            amount_staked: TaoBalance,
            limit_price: TaoBalance,
            allow_partial: bool,
        ) -> DispatchResult {
            Self::do_add_stake_limit(
                origin,
                hotkey,
                netuid,
                amount_staked,
                limit_price,
                allow_partial,
            )
            .map(|_| ())
        }

        /// Like `remove_stake`, but with alpha price floor (`limit_price` RAO/alpha) and optional partial fill.
        ///
        /// In case if slippage occurs and the price shall move beyond the limit
        /// price, the staking order may execute only partially or not execute
        /// at all.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The associated hotkey account.
        ///
        /// * `netuid`: Subnetwork UID.
        ///
        /// * `amount_unstaked`: The amount of stake to be added to the hotkey staking account.
        ///
        /// * `limit_price`: The limit price expressed in units of RAO per one Alpha.
        ///
        /// * `allow_partial`: Allows partial execution of the amount. If set to false, this becomes
        ///       fill or kill type of order.
        ///
        /// # Events
        /// * `StakeRemoved`: On the successfully removing stake from the hotkey account.
        ///
        /// # Errors
        /// * `NotRegistered`: Thrown if the account we are attempting to unstake from is non existent.
        ///
        /// * `NonAssociatedColdKey`: Thrown if the coldkey does not own the hotkey we are unstaking from.
        ///
        /// * `NotEnoughStakeToWithdraw`: Thrown if there is not enough stake on the hotkey to withdwraw this amount.
        ///
        #[pallet::call_index(89)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::remove_stake_limit())]
        pub fn remove_stake_limit(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            amount_unstaked: AlphaBalance,
            limit_price: TaoBalance,
            allow_partial: bool,
        ) -> DispatchResult {
            Self::do_remove_stake_limit(
                origin,
                hotkey,
                netuid,
                amount_unstaked,
                limit_price,
                allow_partial,
            )
        }

        /// Like `swap_stake`, with `limit_price` (RAO/alpha) and optional partial fill on the AMM legs.
        ///
        /// # Arguments
        /// * `origin`: Coldkey that owns `hotkey`.
        /// * `hotkey`: Hotkey whose stake is swapped.
        /// * `origin_netuid` / `destination_netuid`: Distinct subnets.
        /// * `alpha_amount`: Alpha to sell on origin and rebuy on destination.
        /// * `limit_price`: Limit price in RAO per alpha.
        /// * `allow_partial`: If false, fill-or-kill when the limit would be crossed.
        ///
        /// # Errors
        /// * `BadOrigin` / `SameNetuid` / `SubnetNotExists` / `SubtokenDisabled` / `HotKeyAccountNotExists` /
        ///   `NotEnoughStakeToWithdraw` / `InsufficientLiquidity` / `AmountTooLow` / `SlippageTooHigh` /
        ///   `StakeUnavailable`.
        ///
        /// # Events
        /// May emit a `StakeSwapped` event on success.
        #[pallet::call_index(90)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::swap_stake_limit())]
        pub fn swap_stake_limit(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            origin_netuid: NetUid,
            destination_netuid: NetUid,
            alpha_amount: AlphaBalance,
            limit_price: TaoBalance,
            allow_partial: bool,
        ) -> DispatchResult {
            Self::do_swap_stake_limit(
                origin,
                hotkey,
                origin_netuid,
                destination_netuid,
                alpha_amount,
                limit_price,
                allow_partial,
            )
        }

        /// Associates `hotkey` with the signing coldkey if unbound; no-op / errors if already owned elsewhere.
        ///
        /// # Arguments
        /// * `origin`: The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
        /// * `hotkey`: The hotkey to associate with the coldkey.
        ///
        /// # Note
        /// Will charge based on the weight even if the hotkey is already associated with a coldkey.
        #[pallet::call_index(91)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::try_associate_hotkey())]
        pub fn try_associate_hotkey(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;

            Self::do_try_associate_hotkey(&coldkey, &hotkey)?;

            Ok(())
        }

        /// Subnet-owner starts emissions for `netuid` (sets first emission block); owner-signed.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, which must be signed by the subnet owner.
        /// * `netuid`: The unique identifier of the subnet on which the call is being initiated.
        ///
        /// # Events
        /// Emits a `FirstEmissionBlockNumberSet` event on success.
        #[pallet::call_index(92)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::start_call())]
        pub fn start_call(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            Self::do_start_call(origin, netuid)?;
            Ok(())
        }

        /// Associates an EVM address with a registered hotkey after ECDSA recovery of the expected signed payload.
        ///
        /// The signature will be checked to see if the recovered public key matches the `evm_key` provided.
        ///
        /// The EVM key is expected to sign the message according to this formula to produce the signature:
        /// ```text
        /// keccak_256(hotkey ++ keccak_256(block_number))
        /// ```
        ///
        /// # Arguments
        /// * `origin`: The origin of the transaction, which must be signed by the `hotkey`.
        /// * `netuid`: The netuid that the `hotkey` belongs to.
        /// * `evm_key`: The EVM key to associate with the `hotkey`.
        /// * `block_number`: The block number used in the `signature`.
        /// * `signature`: A signed message by the `evm_key` containing the `hotkey` and the hashed `block_number`.
        ///
        /// # Errors
        /// * `BadOrigin`: The transaction is not signed.
        /// * `SubnetNotExists`: The subnet identified by `netuid` does not exist.
        /// * `NonAssociatedColdKey`: The hotkey is not associated with a coldkey.
        /// * `HotKeyNotRegisteredInSubNet`: The hotkey is not registered on the subnet identified by `netuid`.
        /// * `EvmKeyAssociateRateLimitExceeded`: The association rate limit has not elapsed since the last association.
        /// * `InvalidIdentity`: The public key cannot be recovered from the signature.
        /// * `UnableToRecoverPublicKey`: The recovered public key cannot be parsed.
        /// * `InvalidRecoveredPublicKey`: The EVM key recovered from the signature does not match the given `evm_key`.
        /// * `EvmKeyAssociationLimitExceeded`: The EVM key is already associated with the maximum number of UIDs.
        ///
        /// # Events
        /// May emit a `EvmKeyAssociated` event on success
        #[pallet::call_index(93)]
        #[pallet::weight((
            <T as crate::pallet::Config>::WeightInfo::associate_evm_key(),
            DispatchClass::Normal,
            Pays::No
        ))]
        pub fn associate_evm_key(
            origin: OriginFor<T>,
            netuid: NetUid,
            evm_key: H160,
            block_number: u64,
            signature: Signature,
        ) -> DispatchResult {
            Self::do_associate_evm_key(origin, netuid, evm_key, block_number, signature)
        }

        /// Recycles alpha from `(coldkey, hotkey)` on `netuid`, decreasing `AlphaOut` (coldkey origin).
        ///
        /// # Arguments
        /// * `origin`: The origin of the call (must be signed by the coldkey)
        /// * `hotkey`: The hotkey account
        /// * `amount`: The amount of alpha to recycle
        /// * `netuid`: The subnet ID
        ///
        /// # Events
        /// Emits a `TokensRecycled` event on success.
        #[pallet::call_index(101)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::recycle_alpha())]
        pub fn recycle_alpha(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            amount: AlphaBalance,
            netuid: NetUid,
        ) -> DispatchResult {
            Self::do_recycle_alpha(origin, hotkey, amount, netuid).map(|_| ())
        }

        /// Burns alpha from `(coldkey, hotkey)` on `netuid` without decreasing `AlphaOut` (coldkey origin).
        ///
        /// # Arguments
        /// * `origin`: The origin of the call (must be signed by the coldkey)
        /// * `hotkey`: The hotkey account
        /// * `amount`: The amount of alpha to burn
        /// * `netuid`: The subnet ID
        ///
        /// # Events
        /// Emits a `TokensBurned` event on success.
        #[pallet::call_index(102)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::burn_alpha())]
        pub fn burn_alpha(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            amount: AlphaBalance,
            netuid: NetUid,
        ) -> DispatchResult {
            Self::do_burn_alpha(origin, hotkey, amount, netuid).map(|_| ())
        }

        /// Root-only: sets `PendingChildKeyCooldown` (blocks) before a pending childkey assignment takes effect.
        #[pallet::call_index(109)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_pending_childkey_cooldown())]
        pub fn set_pending_childkey_cooldown(
            origin: OriginFor<T>,
            cooldown: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            PendingChildKeyCooldown::<T>::put(cooldown);
            Ok(())
        }

        /// Removes the full `(coldkey, hotkey, netuid)` position, optionally with a sell price floor; `None` limit matches full `remove_stake`.
        #[pallet::call_index(103)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::remove_stake_full_limit())]
        pub fn remove_stake_full_limit(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            limit_price: Option<TaoBalance>,
        ) -> DispatchResult {
            Self::do_remove_stake_full_limit(origin, hotkey, netuid, limit_price)
        }

        /// Registers a leased subnet; origin pays lease terms and designates beneficiary/hotkey per lease config.
        ///
        /// The crowdloan's contributions are used to compute the share of the emissions that the contributors
        /// will receive as dividends.
        ///
        /// The leftover cap is refunded to the contributors and the beneficiary.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `emissions_share`: The share of the emissions that the contributors will receive as dividends.
        ///
        /// * `end_block`: The block at which the lease will end. If not defined, the lease is perpetual.
        #[pallet::call_index(110)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register_leased_network(T::MaxContributors::get()))]
        pub fn register_leased_network(
            origin: OriginFor<T>,
            emissions_share: Percent,
            end_block: Option<BlockNumberFor<T>>,
        ) -> DispatchResultWithPostInfo {
            Self::do_register_leased_network(origin, emissions_share, end_block)
        }

        /// Terminates an active subnet lease early when caller permissions and lease terms allow.
        ///
        /// The beneficiary can terminate the lease after the end block has passed and get the subnet ownership.
        /// The subnet is transferred to the beneficiary and the lease is removed from storage.
        ///
        /// **The hotkey must be owned by the beneficiary coldkey.**
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `lease_id`: The ID of the lease to terminate.
        ///
        /// * `hotkey`: The hotkey of the beneficiary to mark as subnet owner hotkey.
        #[pallet::call_index(111)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::terminate_lease(T::MaxContributors::get()))]
        pub fn terminate_lease(
            origin: OriginFor<T>,
            lease_id: LeaseId,
            hotkey: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            Self::do_terminate_lease(origin, lease_id, hotkey)
        }

        /// Sets `TokenSymbol` for `netuid` (sudo or subnet owner); symbol must exist and be unused.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, which must be the subnet owner or root.
        /// * `netuid`: The unique identifier of the subnet on which the symbol is being set.
        /// * `symbol`: The symbol to set for the subnet.
        ///
        /// # Errors
        /// * `BadOrigin`: The transaction is not signed by the subnet owner or root.
        /// * `SubnetNotExists`: The subnet identified by `netuid` does not exist.
        /// * `SymbolDoesNotExist`: The symbol is not one of the recognized symbols.
        /// * `SymbolAlreadyInUse`: The symbol is already in use by another subnet.
        ///
        /// # Events
        /// Emits a `SymbolUpdated` event on success.
        #[pallet::call_index(112)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::update_symbol())]
        pub fn update_symbol(
            origin: OriginFor<T>,
            netuid: NetUid,
            symbol: Vec<u8>,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;
            ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

            Self::ensure_symbol_exists(&symbol)?;
            Self::ensure_symbol_available(&symbol)?;

            TokenSymbol::<T>::insert(netuid, symbol.clone());

            Self::deposit_event(Event::SymbolUpdated { netuid, symbol });
            Ok(())
        }

        /// Commits CRV3 timelock-encrypted weights for later reveal (hotkey origin; size capped by `MAX_CRV3_COMMIT_SIZE_BYTES`).
        ///
        /// # Arguments
        /// * `origin`: The committing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `commit`: The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * `reveal_round`: The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// * `commit_reveal_version`: The client (bittensor-drand) version.
        #[pallet::call_index(113)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::commit_timelocked_weights(), DispatchClass::Normal, Pays::No))]
        pub fn commit_timelocked_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
            reveal_round: u64,
            commit_reveal_version: u16,
        ) -> DispatchResult {
            Self::do_commit_timelocked_weights(
                origin,
                netuid,
                commit,
                reveal_round,
                commit_reveal_version,
            )
        }

        /// Sets which `hotkey` receives auto-staked emissions for the signing coldkey on `netuid`.
        ///
        /// The caller selects a hotkey where all future rewards
        /// will be automatically staked.
        ///
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// * `hotkey`: The hotkey account to designate as the autostake destination.
        #[pallet::call_index(114)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_coldkey_auto_stake_hotkey())]
        pub fn set_coldkey_auto_stake_hotkey(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
            ensure!(
                Uids::<T>::contains_key(netuid, &hotkey),
                Error::<T>::HotKeyNotRegisteredInSubNet
            );

            let current_hotkey = AutoStakeDestination::<T>::get(coldkey.clone(), netuid);
            if let Some(current_hotkey) = current_hotkey {
                ensure!(
                    current_hotkey != hotkey,
                    Error::<T>::SameAutoStakeHotkeyAlreadySet
                );

                // Remove the coldkey from the old hotkey (if present)
                AutoStakeDestinationColdkeys::<T>::mutate(current_hotkey.clone(), netuid, |v| {
                    v.retain(|c| c != &coldkey);
                });
            }

            // Add the coldkey to the new hotkey (if not already present)
            AutoStakeDestination::<T>::insert(coldkey.clone(), netuid, hotkey.clone());
            AutoStakeDestinationColdkeys::<T>::mutate(hotkey.clone(), netuid, |v| {
                if !v.contains(&coldkey) {
                    v.push(coldkey.clone());
                }
            });

            Self::deposit_event(Event::AutoStakeDestinationSet {
                coldkey,
                netuid,
                hotkey,
            });

            Ok(())
        }

        /// Commits CRV3 timelock-encrypted weights for mechanism `mecid` (hotkey origin; size capped by `MAX_CRV3_COMMIT_SIZE_BYTES`).
        ///
        /// # Arguments
        /// * `origin`: The committing hotkey.
        ///
        /// * `netuid`: The u16 network identifier.
        ///
        /// * `mecid`: The u8 mechanism identifier.
        ///
        /// * `commit`: The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * `reveal_round`: The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// * `commit_reveal_version`: The client (bittensor-drand) version.
        #[pallet::call_index(118)]
        #[pallet::weight((<T as Config>::WeightInfo::commit_timelocked_mechanism_weights(), DispatchClass::Normal, Pays::No))]
        pub fn commit_timelocked_mechanism_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
            reveal_round: u64,
            commit_reveal_version: u16,
        ) -> DispatchResult {
            Self::do_commit_timelocked_mechanism_weights(
                origin,
                netuid,
                mecid,
                commit,
                reveal_round,
                commit_reveal_version,
            )
        }

        /// Root-only dissolve of subnet `netuid` (same effect as `dissolve_network`, without unused coldkey arg).
        #[pallet::call_index(120)]
        #[pallet::weight(<T as Config>::WeightInfo::root_dissolve_network())]
        pub fn root_dissolve_network(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_dissolve_network(netuid)
        }

        /// Claims root emissions for the signing coldkey on the given subnet set (non-empty, <= `MAX_SUBNET_CLAIMS`).
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// # Events
        /// * `RootClaimed`: On the successfully claiming the root emissions for a coldkey.
        ///
        /// # Errors
        /// * `InvalidSubnetNumber`: The subnet set is empty or exceeds the maximum number of claims.
        ///
        #[pallet::call_index(121)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::claim_root())]
        pub fn claim_root(
            origin: OriginFor<T>,
            subnets: BTreeSet<NetUid>,
        ) -> DispatchResultWithPostInfo {
            let coldkey: T::AccountId = ensure_signed(origin)?;

            ensure!(!subnets.is_empty(), Error::<T>::InvalidSubnetNumber);
            ensure!(
                subnets.len() <= MAX_SUBNET_CLAIMS,
                Error::<T>::InvalidSubnetNumber
            );

            Self::maybe_add_coldkey_index(&coldkey);

            let weight = Self::do_root_claim(coldkey, Some(subnets))?;
            Ok((Some(weight), Pays::Yes).into())
        }

        /// Sets how the signing coldkey claims root emissions (`RootClaimTypeEnum`, including keep-subnet filters).
        /// # Arguments
        /// * `origin`: The signature of the caller's coldkey.
        ///
        /// # Events
        /// * `RootClaimTypeSet`: On the successfully setting the root claim type for the coldkey.
        ///
        #[pallet::call_index(122)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_root_claim_type())]
        pub fn set_root_claim_type(
            origin: OriginFor<T>,
            new_root_claim_type: RootClaimTypeEnum,
        ) -> DispatchResult {
            let coldkey: T::AccountId = ensure_signed(origin)?;

            if let RootClaimTypeEnum::KeepSubnets { subnets } = &new_root_claim_type {
                ensure!(!subnets.is_empty(), Error::<T>::InvalidSubnetNumber);
            }

            Self::maybe_add_coldkey_index(&coldkey);

            Self::change_root_claim_type(&coldkey, new_root_claim_type);
            Ok(())
        }

        /// Root-only: sets how many coldkeys auto-claim root emissions per block; `0` disables (max `MAX_NUM_ROOT_CLAIMS`).
        #[pallet::call_index(123)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::sudo_set_num_root_claims())]
        pub fn sudo_set_num_root_claims(origin: OriginFor<T>, new_value: u64) -> DispatchResult {
            ensure_root(origin)?;

            ensure!(
                new_value <= MAX_NUM_ROOT_CLAIMS,
                Error::<T>::InvalidNumRootClaim
            );

            NumRootClaim::<T>::set(new_value);

            Ok(())
        }

        /// Sets per-subnet `RootClaimableThreshold` (sudo or subnet owner); claims only above this threshold.
        #[pallet::call_index(124)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::sudo_set_root_claim_threshold())]
        pub fn sudo_set_root_claim_threshold(
            origin: OriginFor<T>,
            netuid: NetUid,
            new_value: u64,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;
            ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

            ensure!(
                new_value <= I96F32::from(MAX_ROOT_CLAIM_THRESHOLD),
                Error::<T>::InvalidRootClaimThreshold
            );

            RootClaimableThreshold::<T>::set(netuid, new_value.into());

            Ok(())
        }

        /// Announces a pending coldkey swap by committing `BlakeTwo256` of the new coldkey; starts the dispute window.
        ///
        /// This is required before the coldkey swap can be performed
        /// after the delay period.
        ///
        /// It can be reannounced after a delay of `ColdkeySwapReannouncementDelay` following
        /// the first valid execution block of the original announcement.
        ///
        /// The dispatch origin of this call must be the original coldkey that made the announcement.
        ///
        /// * `new_coldkey_hash`: The hash of the new coldkey using BlakeTwo256.
        ///
        /// The `ColdkeySwapAnnounced` event is emitted on successful announcement.
        ///
        #[pallet::call_index(125)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::announce_coldkey_swap())]
        pub fn announce_coldkey_swap(
            origin: OriginFor<T>,
            new_coldkey_hash: T::Hash,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let now = <frame_system::Pallet<T>>::block_number();

            if let Some((when, _)) = ColdkeySwapAnnouncements::<T>::get(who.clone()) {
                let reannouncement_delay = ColdkeySwapReannouncementDelay::<T>::get();
                let min_when = when.saturating_add(reannouncement_delay);
                ensure!(now >= min_when, Error::<T>::ColdkeySwapReannouncedTooEarly);
            } else {
                // Only charge the swap cost on the first announcement
                let swap_cost = Self::get_key_swap_cost();
                Self::charge_swap_cost(&who, swap_cost)?;
            }

            let delay = ColdkeySwapAnnouncementDelay::<T>::get();
            let when = now.saturating_add(delay);
            ColdkeySwapAnnouncements::<T>::insert(who.clone(), (when, new_coldkey_hash.clone()));

            Self::deposit_event(Event::ColdkeySwapAnnounced {
                who,
                new_coldkey_hash,
            });
            Ok(())
        }

        /// Completes an announced coldkey swap after the delay if undisputed; reveals the new coldkey matching the hash.
        ///
        /// The dispatch origin of this call must be the original coldkey that made the announcement.
        ///
        /// * `new_coldkey`: The new coldkey to swap to. The BlakeTwo256 hash of the new coldkey must be
        ///   the same as the announced coldkey hash.
        ///
        /// The `ColdkeySwapped` event is emitted on successful swap.
        #[pallet::call_index(126)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::swap_coldkey_announced())]
        pub fn swap_coldkey_announced(
            origin: OriginFor<T>,
            new_coldkey: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let (when, new_coldkey_hash) = ColdkeySwapAnnouncements::<T>::take(who.clone())
                .ok_or(Error::<T>::ColdkeySwapAnnouncementNotFound)?;

            ensure!(
                new_coldkey_hash == T::Hashing::hash_of(&new_coldkey),
                Error::<T>::AnnouncedColdkeyHashDoesNotMatch
            );

            let now = <frame_system::Pallet<T>>::block_number();
            ensure!(now >= when, Error::<T>::ColdkeySwapTooEarly);

            Self::do_swap_coldkey(&who, &new_coldkey)?;

            Ok(())
        }

        /// Disputes a pending coldkey-swap announcement for the signing coldkey, blocking completion until reset.
        ///
        /// This will prevent any further actions on the coldkey swap
        /// until triumvirate step in to resolve the issue.
        ///
        /// * `coldkey`: The coldkey to dispute the swap for.
        ///
        #[pallet::call_index(127)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::dispute_coldkey_swap())]
        pub fn dispute_coldkey_swap(origin: OriginFor<T>) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;

            ensure!(
                ColdkeySwapAnnouncements::<T>::contains_key(&coldkey),
                Error::<T>::ColdkeySwapAnnouncementNotFound
            );
            ensure!(
                !ColdkeySwapDisputes::<T>::contains_key(&coldkey),
                Error::<T>::ColdkeySwapAlreadyDisputed
            );

            let now = <frame_system::Pallet<T>>::block_number();
            ColdkeySwapDisputes::<T>::insert(&coldkey, now);

            Self::deposit_event(Event::ColdkeySwapDisputed { coldkey });
            Ok(())
        }

        /// Clears announcement and dispute state for the signing coldkey so a new swap can be announced.
        ///
        /// The dispatch origin of this call must be root.
        ///
        /// * `coldkey`: The coldkey to reset the swap for.
        ///
        #[pallet::call_index(128)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::reset_coldkey_swap())]
        pub fn reset_coldkey_swap(origin: OriginFor<T>, coldkey: T::AccountId) -> DispatchResult {
            ensure_root(origin)?;

            ColdkeySwapAnnouncements::<T>::remove(&coldkey);
            ColdkeySwapDisputes::<T>::remove(&coldkey);

            Self::deposit_event(Event::ColdkeySwapReset { who: coldkey });
            Ok(())
        }

        /// Enables voting-power tracking on `netuid` (sudo or owner); required before EMA voting-power features apply.
        ///
        /// This function can be called by the subnet owner or root.
        /// When enabled, voting power EMA is updated every epoch for all validators.
        /// Voting power starts at 0 and increases over epochs.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, must be subnet owner or root.
        /// * `netuid`: The subnet to enable voting power tracking for.
        ///
        /// # Errors
        /// * `SubnetNotExist`: If the subnet does not exist.
        /// * `NotSubnetOwner`: If the caller is not the subnet owner or root.
        #[pallet::call_index(129)]
        #[pallet::weight(<T as Config>::WeightInfo::enable_voting_power_tracking())]
        pub fn enable_voting_power_tracking(
            origin: OriginFor<T>,
            netuid: NetUid,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;
            Self::do_enable_voting_power_tracking(netuid)
        }

        /// Schedules disable of voting-power tracking on `netuid` (sudo or owner); not an instantaneous clear.
        ///
        /// This function can be called by the subnet owner or root.
        /// Voting power tracking will continue for 14 days (grace period) after this call,
        /// then automatically disable and clear all VotingPower entries for the subnet.
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, must be subnet owner or root.
        /// * `netuid`: The subnet to schedule disabling voting power tracking for.
        ///
        /// # Errors
        /// * `SubnetNotExist`: If the subnet does not exist.
        /// * `NotSubnetOwner`: If the caller is not the subnet owner or root.
        /// * `VotingPowerTrackingNotEnabled`: If voting power tracking is not enabled.
        #[pallet::call_index(130)]
        #[pallet::weight(<T as Config>::WeightInfo::disable_voting_power_tracking())]
        pub fn disable_voting_power_tracking(
            origin: OriginFor<T>,
            netuid: NetUid,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;
            Self::do_disable_voting_power_tracking(netuid)
        }

        /// Root-only: sets voting-power EMA alpha for `netuid` (`u64` with 1e18 = 1.0; must be <= 1e18).
        ///
        /// This function can only be called by root (sudo).
        /// Higher alpha = faster response to stake changes.
        /// Alpha is stored as u64 with 18 decimal precision (1.0 = 10^18).
        ///
        /// # Arguments
        /// * `origin`: The origin of the call, must be root.
        /// * `netuid`: The subnet to set the alpha for.
        /// * `alpha`: The new alpha value (u64 with 18 decimal precision).
        ///
        /// # Errors
        /// * `BadOrigin`: If the origin is not root.
        /// * `SubnetNotExist`: If the subnet does not exist.
        /// * `InvalidVotingPowerEmaAlpha`: If alpha is greater than 10^18 (1.0).
        #[pallet::call_index(131)]
        #[pallet::weight(<T as Config>::WeightInfo::sudo_set_voting_power_ema_alpha())]
        pub fn sudo_set_voting_power_ema_alpha(
            origin: OriginFor<T>,
            netuid: NetUid,
            alpha: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_set_voting_power_ema_alpha(netuid, alpha)
        }

        /// Subnet buyback: buy alpha with TAO (optional price `limit`) then immediately `burn_alpha` the acquired amount.
        #[pallet::call_index(132)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::add_stake_burn())]
        pub fn add_stake_burn(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            amount: TaoBalance,
            limit: Option<TaoBalance>,
        ) -> DispatchResult {
            Self::do_add_stake_burn(origin, hotkey, netuid, amount, limit)
        }

        /// Clears a coldkey swap announcement after the reannouncement delay if
        /// it has not been disputed.
        ///
        /// The `ColdkeySwapCleared` event is emitted on successful clear.
        #[pallet::call_index(133)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::clear_coldkey_swap_announcement())]
        pub fn clear_coldkey_swap_announcement(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let now = <frame_system::Pallet<T>>::block_number();

            let Some((when, _)) = ColdkeySwapAnnouncements::<T>::get(who.clone()) else {
                return Err(Error::<T>::ColdkeySwapAnnouncementNotFound.into());
            };
            let delay = ColdkeySwapReannouncementDelay::<T>::get();
            let min_when = when.saturating_add(delay);
            ensure!(now >= min_when, Error::<T>::ColdkeySwapClearTooEarly);

            ColdkeySwapAnnouncements::<T>::remove(&who);

            Self::deposit_event(Event::ColdkeySwapCleared { who });
            Ok(())
        }

        /// Registers `hotkey` via burn only when this block's burn price is <= `limit_price` (same units as `Burn`).
        ///
        /// Fill-or-kill relative to burn; coldkey origin. Distinct from creating a subnet.
        #[pallet::call_index(134)]
        #[pallet::weight((<T as Config>::WeightInfo::register_limit(), DispatchClass::Normal, Pays::Yes))]
        pub fn register_limit(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            limit_price: u64,
        ) -> DispatchResult {
            Self::do_register_limit(origin, netuid, hotkey, limit_price)
        }

        /// Coldkey owning a root-registered `hotkey` toggles auto parent-delegation to new subnet owner hotkeys.
        #[pallet::call_index(135)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::set_auto_parent_delegation_enabled(), DispatchClass::Normal, Pays::Yes))]
        pub fn set_auto_parent_delegation_enabled(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            enabled: bool,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;

            ensure!(
                Self::coldkey_owns_hotkey(&coldkey, &hotkey),
                Error::<T>::NonAssociatedColdKey
            );

            ensure!(
                Self::is_hotkey_registered_on_network(NetUid::ROOT, &hotkey),
                Error::<T>::HotKeyNotRegisteredInSubNet
            );

            AutoParentDelegationEnabled::<T>::insert(&hotkey, enabled);

            Self::deposit_event(Event::AutoParentDelegationEnabledSet { hotkey, enabled });
            Ok(())
        }

        /// Locks stake on a subnet to a specific hotkey, building conviction over time.
        ///
        /// If no lock exists for (coldkey, subnet), a new one is created.
        /// If a lock exists, the destination hotkey must match the existing lock's hotkey.
        /// Top-up adds to the locked amount after rolling the lock state forward.
        ///
        /// # Arguments
        /// * `origin`: Must be signed by the coldkey.
        /// * `hotkey`: The hotkey to lock stake to.
        /// * `netuid`: The subnet on which to lock.
        /// * `amount`: The alpha amount to lock.
        #[pallet::call_index(136)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::lock_stake())]
        pub fn lock_stake(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            amount: AlphaBalance,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            Self::do_lock_stake(&coldkey, netuid, &hotkey, amount)
        }

        /// Moves an existing lock for a coldkey on a subnet from one hotkey to another.
        ///
        /// The lock is rolled forward to the current block before switching the
        /// associated hotkey, preserving the decayed locked mass. The conviction is
        /// reset to zero.
        ///
        /// # Arguments
        /// * `origin`: Must be signed by the coldkey that owns the lock.
        /// * `destination_hotkey`: The hotkey the lock should target after the move.
        /// * `netuid`: The subnet on which the lock exists.
        /// # Errors
        /// * `Error::<T>::NoExistingLock`: If no lock exists for the given coldkey and subnet.
        #[pallet::call_index(137)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::move_lock())]
        pub fn move_lock(
            origin: OriginFor<T>,
            destination_hotkey: T::AccountId,
            netuid: NetUid,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            Self::do_move_lock(&coldkey, &destination_hotkey, netuid)
        }

        /// Sets or clears the caller's perpetual lock flag for a subnet.
        ///
        /// Locks decay by default. When enabled, the caller's individual lock
        /// does not unlock through locked-mass decay. Passing `false` returns
        /// the caller's lock to normal decay.
        #[pallet::call_index(138)]
        #[pallet::weight(<T as Config>::WeightInfo::set_perpetual_lock())]
        pub fn set_perpetual_lock(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            Self::do_set_perpetual_lock(&coldkey, netuid, enabled)
        }

        /// Deprecated compatibility entry point retained for call-index stability.
        /// This call charges a fee, returns success, and does not modify state.
        /// Use `AdminUtils::sudo_set_tempo` to change subnet tempo.
        #[deprecated(note = "Use `AdminUtils::sudo_set_tempo` instead")]
        #[pallet::call_index(139)]
        #[pallet::weight((
            <T as crate::pallet::Config>::WeightInfo::set_tempo(),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        pub fn set_tempo(_origin: OriginFor<T>, _netuid: NetUid, _tempo: u16) -> DispatchResult {
            Ok(())
        }

        /// Deprecated compatibility entry point retained for call-index stability.
        /// This call charges a fee, returns success, and does not modify state.
        /// Use `AdminUtils::sudo_set_activity_cutoff_factor` to change the factor.
        #[deprecated(note = "Use `AdminUtils::sudo_set_activity_cutoff_factor` instead")]
        #[pallet::call_index(140)]
        #[pallet::weight((
            <T as crate::pallet::Config>::WeightInfo::set_activity_cutoff_factor(),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        pub fn set_activity_cutoff_factor(
            _origin: OriginFor<T>,
            _netuid: NetUid,
            _factor_milli: u32,
        ) -> DispatchResult {
            Ok(())
        }

        /// Subnet-owner schedules an epoch after `AdminFreezeWindow` blocks; rate-limited like other owner hyperparam updates.
        #[pallet::call_index(141)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::trigger_epoch())]
        pub fn trigger_epoch(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            Self::do_trigger_epoch(origin, netuid)
        }

        /// Sets or clears whether the caller rejects incoming locked alpha.
        ///
        /// Coldkeys reject locked alpha by default. Passing `false` opts the
        /// caller into receiving locked alpha from stake transfers or coldkey
        /// swaps.
        #[pallet::call_index(142)]
        #[pallet::weight((<T as Config>::WeightInfo::set_reject_locked_alpha(), DispatchClass::Normal, Pays::Yes))]
        pub fn set_reject_locked_alpha(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            Self::set_accept_locked_alpha(&coldkey, !enabled);
            Self::deposit_event(Event::RejectLockedAlphaUpdated { coldkey, enabled });
            Ok(())
        }

        /// Transfers alpha stake to another coldkey and hotkey in one call, optionally across subnets.
        ///
        /// This is `transfer_stake` generalized to a destination hotkey: it transfers
        /// ownership of the position and re-delegates it in one atomic call. Use
        /// `transfer_stake` when the hotkey stays the same, and `move_stake` when only
        /// the hotkey changes (ownership stays with the signing coldkey).
        ///
        /// # Arguments
        /// * `origin`: The origin of the transaction, which must be signed by the `origin_coldkey`.
        /// * `destination_coldkey`: The coldkey to which the stake is transferred.
        /// * `origin_hotkey`: The hotkey the stake currently sits on.
        /// * `destination_hotkey`: The hotkey the stake lands on.
        /// * `origin_netuid`: The network/subnet ID to move stake from.
        /// * `destination_netuid`: The network/subnet ID to move stake to (for cross-subnet transfer).
        /// * `alpha_amount`: The amount of stake to transfer.
        ///
        /// # Errors
        /// * `BadOrigin`: The transaction is not signed.
        /// * `SubnetNotExists`: Either `origin_netuid` or `destination_netuid` does not exist.
        /// * `SubtokenDisabled`: The subtoken is disabled on the origin or destination subnet.
        /// * `HotKeyAccountNotExists`: The `origin_hotkey` or `destination_hotkey` account does not exist.
        /// * `NotEnoughStakeToWithdraw`: The `(origin_coldkey, origin_hotkey, origin_netuid)` position has less stake than `alpha_amount`.
        /// * `InsufficientLiquidity`: The swap simulation on the origin subnet fails.
        /// * `AmountTooLow`: The TAO-equivalent of the transfer is below the minimum stake requirement.
        /// * `TransferDisallowed`: Transfers are disabled on the origin or destination subnet.
        /// * `StakeUnavailable`: The remaining stake would not cover the locked amount on the origin subnet.
        ///
        /// # Events
        /// May emit a `StakeAndHotkeyTransferred` event on success.
        #[pallet::call_index(143)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::transfer_stake_and_hotkey())]
        pub fn transfer_stake_and_hotkey(
            origin: OriginFor<T>,
            destination_coldkey: T::AccountId,
            origin_hotkey: T::AccountId,
            destination_hotkey: T::AccountId,
            origin_netuid: NetUid,
            destination_netuid: NetUid,
            alpha_amount: AlphaBalance,
        ) -> DispatchResult {
            Self::do_transfer_stake_and_hotkey(
                origin,
                destination_coldkey,
                origin_hotkey,
                destination_hotkey,
                origin_netuid,
                destination_netuid,
                alpha_amount,
            )
        }

        /// Locks additional miner collateral (in alpha) on the signer's own
        /// hotkey.
        ///
        /// Prefers free alpha already staked on that `(hotkey, coldkey)`
        /// position and only buys the shortfall with TAO. Used to top up
        /// registration collateral voluntarily — for example to meet a
        /// validator-published per-machine requirement on resource subnets.
        /// The lock is released back through earned emission exactly like
        /// registration collateral (it is the same lock), keeps the existing
        /// drain-ratio snapshot, and is credited against the collateral
        /// requirement on re-registration.
        ///
        /// # Arguments
        /// * `origin`: Signed by the coldkey that owns `hotkey`.
        /// * `netuid`: The subnet to lock collateral on.
        /// * `hotkey`: The miner hotkey the collateral attaches to.
        /// * `alpha`: Alpha of collateral to add. Fully covered by free
        ///   stake when possible; otherwise the unpaid remainder is bought
        ///   with TAO (that remainder must meet the staking minimum).
        /// * `limit_price`: Worst alpha price (RAO per alpha) accepted for any
        ///   TAO→alpha buy of the shortfall. Fill-or-kill: the buy fails with
        ///   `SlippageTooHigh` instead of executing above this price. Pass the
        ///   current spot (or spot × (1 + tolerance)) — never the swap max.
        ///
        /// # Errors
        /// * `RegistrationNotPermittedOnRootSubnet`: `netuid` is the root network.
        /// * `SubnetNotExists`: The subnet does not exist.
        /// * `HotKeyAccountNotExists`: The hotkey account does not exist.
        /// * `NonAssociatedColdKey`: The signer does not own `hotkey`.
        /// * `AmountTooLow`: `alpha` is zero, or the buy remainder is below
        ///   the minimum stake.
        /// * `NotEnoughBalanceToStake`: The coldkey cannot cover the buy remainder.
        /// * `SlippageTooHigh`: The shortfall buy would clear above `limit_price`.
        ///
        /// # Events
        /// Emits `CollateralLocked` on success.
        #[pallet::call_index(144)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::add_collateral())]
        pub fn add_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            alpha: AlphaBalance,
            limit_price: TaoBalance,
        ) -> DispatchResult {
            Self::do_add_collateral(origin, netuid, hotkey, alpha, limit_price)
        }

        /// Sets the self-maintaining collateral floor for the signer's hotkey
        /// on a subnet.
        ///
        /// The drain never releases the lock below the floor, and while the
        /// lock is under it, earned emission is captured into the lock until
        /// the floor is met — so a miner tracking a validator-published
        /// collateral requirement does not need to keep re-locking drained
        /// funds. Zero clears the floor and restores pure drain behavior.
        ///
        /// # Arguments
        /// * `origin`: Signed by the coldkey that owns `hotkey`.
        /// * `netuid`: The subnet the floor applies to.
        /// * `hotkey`: The miner hotkey the floor applies to.
        /// * `min_locked`: The floor, in alpha; zero clears it.
        ///
        /// # Errors
        /// * `RegistrationNotPermittedOnRootSubnet`: `netuid` is the root network.
        /// * `SubnetNotExists`: The subnet does not exist.
        /// * `HotKeyAccountNotExists`: The hotkey account does not exist.
        /// * `NonAssociatedColdKey`: The signer does not own `hotkey`.
        ///
        /// # Events
        /// Emits `MinCollateralSet` on success.
        #[pallet::call_index(145)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_min_collateral())]
        pub fn set_min_collateral(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            min_locked: AlphaBalance,
        ) -> DispatchResult {
            Self::do_set_min_collateral(origin, netuid, hotkey, min_locked)
        }
    }
}
