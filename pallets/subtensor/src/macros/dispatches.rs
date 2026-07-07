#![allow(clippy::crate_in_macro_def)]
use frame_support::pallet_macros::pallet_section;

/// A [`pallet_section`] that defines the errors for a pallet.
/// This can later be imported into the pallet using [`import_section`].
#[pallet_section]
mod dispatches {
    use crate::weights::WeightInfo;
    use frame_support::traits::schedule::v3::Anon as ScheduleAnon;
    use frame_system::pallet_prelude::BlockNumberFor;
    use sp_core::ecdsa::Signature;
    use sp_runtime::{Percent, Saturating, traits::Hash};

    use crate::MAX_CRV3_COMMIT_SIZE_BYTES;
    use crate::MAX_NUM_ROOT_CLAIMS;
    use crate::MAX_ROOT_CLAIM_THRESHOLD;
    use crate::MAX_SUBNET_CLAIMS;

    /// Dispatchable functions allow users to interact with the pallet and invoke state changes.
    /// These functions materialize as "extrinsics", which are often compared to transactions.
    /// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// --- Sets the caller weights for the incentive mechanism. The call can be
        /// made from the hotkey account so is potentially insecure, however, the damage
        /// of changing weights is minimal if caught early. This function includes all the
        /// checks that the passed weights meet the requirements. Stored as u16s they represent
        /// rational values in the range [0,1] which sum to 1 and can be interpreted as
        /// probabilities. The specific weights determine how inflation propagates outward
        /// from this peer.
        ///
        /// Note: The 16 bit integers weights should represent 1.0 as the max u16.
        /// However, the function normalizes all integers to u16_max anyway. This means that if the sum of all
        /// elements is larger or smaller than the amount of elements * u16_max, all elements
        /// will be corrected for this deviation.
        ///
        /// # Args:
        /// * `origin`: (<T as frame_system::Config>Origin):
        ///     - The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuid` (u16):
        /// 	- The network uid we are setting these weights on.
        ///
        /// * `dests` (Vec<u16>):
        /// 	- The edge endpoint for the weight, i.e. j for w_ij.
        ///
        /// * 'weights' (Vec<u16>):
        /// 	- The u16 integer encoded weights. Interpreted as rational
        ///     values in the range [0,1]. They must sum to in32::MAX.
        ///
        /// * 'version_key' ( u64 ):
        /// 	- The network version key to check if the validator is up to date.
        ///
        /// # Event:
        /// * WeightsSet;
        /// 	- On successfully setting the weights on chain.
        ///
        /// # Raises:
        /// * 'MechanismDoesNotExist':
        /// 	- Attempting to set weights on a non-existent network.
        ///
        /// * 'NotRegistered':
        /// 	- Attempting to set weights from a non registered account.
        ///
        /// * 'WeightVecNotEqualSize':
        /// 	- Attempting to set weights with uids not of same length.
        ///
        /// * 'DuplicateUids':
        /// 	- Attempting to set weights with duplicate uids.
        ///
        ///     * 'UidsLengthExceedUidsInSubNet':
        /// 	- Attempting to set weights above the max allowed uids.
        ///
        /// * 'UidVecContainInvalidOne':
        /// 	- Attempting to set weights with invalid uids.
        ///
        /// * 'WeightVecLengthIsLow':
        /// 	- Attempting to set weights with fewer weights than min.
        ///
        /// * 'MaxWeightExceeded':
        /// 	- Attempting to set weights with max value exceeding limit.
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

        /// --- Sets the caller weights for the incentive mechanism for mechanisms. The call
        /// can be made from the hotkey account so is potentially insecure, however, the damage
        /// of changing weights is minimal if caught early. This function includes all the
        /// checks that the passed weights meet the requirements. Stored as u16s they represent
        /// rational values in the range [0,1] which sum to 1 and can be interpreted as
        /// probabilities. The specific weights determine how inflation propagates outward
        /// from this peer.
        ///
        /// Note: The 16 bit integers weights should represent 1.0 as the max u16.
        /// However, the function normalizes all integers to u16_max anyway. This means that if the sum of all
        /// elements is larger or smaller than the amount of elements * u16_max, all elements
        /// will be corrected for this deviation.
        ///
        /// # Args:
        /// * `origin`: (<T as frame_system::Config>Origin):
        ///     - The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuid` (u16):
        /// 	- The network uid we are setting these weights on.
        ///
        /// * `mecid` (`u8`):
        ///   - The u8 mechnism identifier.
        ///
        /// * `dests` (Vec<u16>):
        /// 	- The edge endpoint for the weight, i.e. j for w_ij.
        ///
        /// * 'weights' (Vec<u16>):
        /// 	- The u16 integer encoded weights. Interpreted as rational
        ///     values in the range [0,1]. They must sum to in32::MAX.
        ///
        /// * 'version_key' ( u64 ):
        /// 	- The network version key to check if the validator is up to date.
        ///
        /// # Event:
        /// * WeightsSet;
        /// 	- On successfully setting the weights on chain.
        ///
        /// # Raises:
        /// * 'MechanismDoesNotExist':
        /// 	- Attempting to set weights on a non-existent network.
        ///
        /// * 'NotRegistered':
        /// 	- Attempting to set weights from a non registered account.
        ///
        /// * 'WeightVecNotEqualSize':
        /// 	- Attempting to set weights with uids not of same length.
        ///
        /// * 'DuplicateUids':
        /// 	- Attempting to set weights with duplicate uids.
        ///
        ///     * 'UidsLengthExceedUidsInSubNet':
        /// 	- Attempting to set weights above the max allowed uids.
        ///
        /// * 'UidVecContainInvalidOne':
        /// 	- Attempting to set weights with invalid uids.
        ///
        /// * 'WeightVecLengthIsLow':
        /// 	- Attempting to set weights with fewer weights than min.
        ///
        /// * 'MaxWeightExceeded':
        /// 	- Attempting to set weights with max value exceeding limit.
        #[pallet::call_index(119)]
        #[pallet::weight((Weight::from_parts(15_540_000_000, 0)
        .saturating_add(T::DbWeight::get().reads(4111))
        .saturating_add(T::DbWeight::get().writes(2)), DispatchClass::Normal, Pays::No))]
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

        /// --- Allows a hotkey to set weights for multiple netuids as a batch.
        ///
        /// # Args:
        /// * `origin`: (<T as frame_system::Config>Origin):
        ///     - The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuids` (Vec<Compact<u16>>):
        /// 	- The network uids we are setting these weights on.
        ///
        /// * `weights` (Vec<Vec<(Compact<u16>, Compact<u16>)>):
        /// 	- The weights to set for each network. [(uid, weight), ...]
        ///
        /// * `version_keys` (Vec<Compact<u64>>):
        /// 	- The network version keys to check if the validator is up to date.
        ///
        /// # Event:
        /// * WeightsSet;
        /// 	- On successfully setting the weights on chain.
        /// * BatchWeightsCompleted;
        /// 	- On success of the batch.
        /// * BatchCompletedWithErrors;
        /// 	- On failure of any of the weights in the batch.
        /// * BatchWeightItemFailed;
        /// 	- On failure for each failed item in the batch.
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

        /// ---- Used to commit a hash of your weight values to later be revealed.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The signature of the committing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `commit_hash` (`H256`):
        ///   - The hash representing the committed weights.
        ///
        /// # Raises:
        /// * `CommitRevealDisabled`:
        ///   - Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`:
        ///   - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
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

        /// ---- Used to commit a hash of your weight values to later be revealed for mechanisms.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The signature of the committing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `mecid` (`u8`):
        ///   - The u8 mechanism identifier.
        ///
        /// * `commit_hash` (`H256`):
        ///   - The hash representing the committed weights.
        ///
        /// # Raises:
        /// * `CommitRevealDisabled`:
        ///   - Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`:
        ///   - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        ///
        #[pallet::call_index(115)]
        #[pallet::weight((Weight::from_parts(55_130_000, 0)
		.saturating_add(T::DbWeight::get().reads(7))
		.saturating_add(T::DbWeight::get().writes(2)), DispatchClass::Normal, Pays::No))]
        pub fn commit_mechanism_weights(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            commit_hash: H256,
        ) -> DispatchResult {
            Self::do_commit_mechanism_weights(origin, netuid, mecid, commit_hash)
        }

        /// --- Allows a hotkey to commit weight hashes for multiple netuids as a batch.
        ///
        /// # Args:
        /// * `origin`: (<T as frame_system::Config>Origin):
        ///     - The caller, a hotkey who wishes to set their weights.
        ///
        /// * `netuids` (Vec<Compact<u16>>):
        /// 	- The network uids we are setting these weights on.
        ///
        /// * `commit_hashes` (Vec<H256>):
        /// 	- The commit hashes to commit.
        ///
        /// # Event:
        /// * WeightsSet;
        /// 	- On successfully setting the weights on chain.
        /// * BatchWeightsCompleted;
        /// 	- On success of the batch.
        /// * BatchCompletedWithErrors;
        /// 	- On failure of any of the weights in the batch.
        /// * BatchWeightItemFailed;
        /// 	- On failure for each failed item in the batch.
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

        /// ---- Used to reveal the weights for a previously committed hash.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The signature of the revealing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `uids` (`Vec<u16>`):
        ///   - The uids for the weights being revealed.
        ///
        /// * `values` (`Vec<u16>`):
        ///   - The values of the weights being revealed.
        ///
        /// * `salt` (`Vec<u16>`):
        ///   - The salt used to generate the commit hash.
        ///
        /// * `version_key` (`u64`):
        ///   - The network version key.
        ///
        /// # Raises:
        /// * `CommitRevealDisabled`:
        ///   - Attempting to reveal weights when the commit-reveal mechanism is disabled.
        ///
        /// * `NoWeightsCommitFound`:
        ///   - Attempting to reveal weights without an existing commit.
        ///
        /// * `ExpiredWeightCommit`:
        ///   - Attempting to reveal a weight commit that has expired.
        ///
        /// * `RevealTooEarly`:
        ///   - Attempting to reveal weights outside the valid reveal period.
        ///
        /// * `InvalidRevealCommitHashNotMatch`:
        ///   - The revealed hash does not match any committed hash.
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

        /// ---- Used to reveal the weights for a previously committed hash for mechanisms.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The signature of the revealing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `mecid` (`u8`):
        ///   - The u8 mechanism identifier.
        ///
        /// * `uids` (`Vec<u16>`):
        ///   - The uids for the weights being revealed.
        ///
        /// * `values` (`Vec<u16>`):
        ///   - The values of the weights being revealed.
        ///
        /// * `salt` (`Vec<u16>`):
        ///   - The salt used to generate the commit hash.
        ///
        /// * `version_key` (`u64`):
        ///   - The network version key.
        ///
        /// # Raises:
        /// * `CommitRevealDisabled`:
        ///   - Attempting to reveal weights when the commit-reveal mechanism is disabled.
        ///
        /// * `NoWeightsCommitFound`:
        ///   - Attempting to reveal weights without an existing commit.
        ///
        /// * `ExpiredWeightCommit`:
        ///   - Attempting to reveal a weight commit that has expired.
        ///
        /// * `RevealTooEarly`:
        ///   - Attempting to reveal weights outside the valid reveal period.
        ///
        /// * `InvalidRevealCommitHashNotMatch`:
        ///   - The revealed hash does not match any committed hash.
        ///
        #[pallet::call_index(116)]
        #[pallet::weight((Weight::from_parts(122_000_000, 0)
		.saturating_add(T::DbWeight::get().reads(16))
		.saturating_add(T::DbWeight::get().writes(2)), DispatchClass::Normal, Pays::No))]
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

        /// ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The committing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `commit` (`Vec<u8>`):
        ///   - The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * reveal_round (`u64`):
        ///    - The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// # Raises:
        /// * `CommitRevealV3Disabled`:
        ///   - Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`:
        ///   - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        ///
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

        /// ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed for mechanisms.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The committing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `mecid` (`u8`):
        ///   - The u8 mechanism identifier.
        ///
        /// * `commit` (`Vec<u8>`):
        ///   - The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * reveal_round (`u64`):
        ///    - The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// # Raises:
        /// * `CommitRevealV3Disabled`:
        ///   - Attempting to commit when the commit-reveal mechanism is disabled.
        ///
        /// * `TooManyUnrevealedCommits`:
        ///   - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
        ///
        #[pallet::call_index(117)]
        #[pallet::weight((Weight::from_parts(77_750_000, 0)
		.saturating_add(T::DbWeight::get().reads(7_u64))
		.saturating_add(T::DbWeight::get().writes(2)), DispatchClass::Normal, Pays::No))]
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

        /// ---- The implementation for batch revealing committed weights.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The signature of the revealing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `uids_list` (`Vec<Vec<u16>>`):
        ///   - A list of uids for each set of weights being revealed.
        ///
        /// * `values_list` (`Vec<Vec<u16>>`):
        ///   - A list of values for each set of weights being revealed.
        ///
        /// * `salts_list` (`Vec<Vec<u16>>`):
        ///   - A list of salts used to generate the commit hashes.
        ///
        /// * `version_keys` (`Vec<u64>`):
        ///   - A list of network version keys.
        ///
        /// # Raises:
        /// * `CommitRevealDisabled`:
        ///   - Attempting to reveal weights when the commit-reveal mechanism is disabled.
        ///
        /// * `NoWeightsCommitFound`:
        ///   - Attempting to reveal weights without an existing commit.
        ///
        /// * `ExpiredWeightCommit`:
        ///   - Attempting to reveal a weight commit that has expired.
        ///
        /// * `RevealTooEarly`:
        ///   - Attempting to reveal weights outside the valid reveal period.
        ///
        /// * `InvalidRevealCommitHashNotMatch`:
        ///   - The revealed hash does not match any committed hash.
        ///
        /// * `InvalidInputLengths`:
        ///   - The input vectors are of mismatched lengths.
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

        /// --- Allows delegates to decrease its take value.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>::Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        /// * 'hotkey' (T::AccountId):
        /// 	- The hotkey we are delegating (must be owned by the coldkey.)
        ///
        /// * 'netuid' (u16):
        /// 	- Subnet ID to decrease take for
        ///
        /// * 'take' (u16):
        /// 	- The new stake proportion that this hotkey takes from delegations.
        ///        The new value can be between 0 and 11_796 and should be strictly
        ///        lower than the previous value. It T is the new value (rational number),
        ///        the the parameter is calculated as [65535 * T]. For example, 1% would be
        ///        [0.01 * 65535] = [655.35] = 655
        ///
        /// # Event:
        /// * TakeDecreased;
        /// 	- On successfully setting a decreased take for this hotkey.
        ///
        /// # Raises:
        /// * 'NotRegistered':
        /// 	- The hotkey we are delegating is not registered on the network.
        ///
        /// * 'NonAssociatedColdKey':
        /// 	- The hotkey we are delegating is not owned by the calling coldkey.
        ///
        /// * 'DelegateTakeTooLow':
        /// 	- The delegate is setting a take which is not lower than the previous.
        ///
        #[pallet::call_index(65)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::decrease_take(), DispatchClass::Normal, Pays::No))]
        pub fn decrease_take(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            take: u16,
        ) -> DispatchResult {
            Self::do_decrease_take(origin, hotkey, take)
        }

        /// --- Allows delegates to increase its take value. This call is rate-limited.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>::Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        /// * 'hotkey' (T::AccountId):
        /// 	- The hotkey we are delegating (must be owned by the coldkey.)
        ///
        /// * 'take' (u16):
        /// 	- The new stake proportion that this hotkey takes from delegations.
        ///        The new value can be between 0 and 11_796 and should be strictly
        ///        greater than the previous value. T is the new value (rational number),
        ///        the the parameter is calculated as [65535 * T]. For example, 1% would be
        ///        [0.01 * 65535] = [655.35] = 655
        ///
        /// # Event:
        /// * TakeIncreased;
        /// 	- On successfully setting a increased take for this hotkey.
        ///
        /// # Raises:
        /// * 'NotRegistered':
        /// 	- The hotkey we are delegating is not registered on the network.
        ///
        /// * 'NonAssociatedColdKey':
        /// 	- The hotkey we are delegating is not owned by the calling coldkey.
        ///
        /// * 'DelegateTakeTooHigh':
        /// 	- The delegate is setting a take which is not greater than the previous.
        ///
        #[pallet::call_index(66)]
        #[pallet::weight((<T as crate::pallet::Config>::WeightInfo::increase_take(), DispatchClass::Normal, Pays::No))]
        pub fn increase_take(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            take: u16,
        ) -> DispatchResult {
            Self::do_increase_take(origin, hotkey, take)
        }

        /// --- Adds stake to a hotkey. The call is made from a coldkey account.
        /// This delegates stake to the hotkey.
        ///
        /// Note: the coldkey account may own the hotkey, in which case they are
        /// delegating to themselves.
        ///
        /// # Args:
        ///  * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        ///  * 'hotkey' (T::AccountId):
        /// 	- The associated hotkey account.
        ///
        /// * 'netuid' (u16):
        ///     - Subnetwork UID
        ///
        ///  * 'amount_staked' (u64):
        /// 	- The amount of stake to be added to the hotkey staking account.
        ///
        /// # Event:
        ///  * StakeAdded;
        /// 	- On the successfully adding stake to a global account.
        ///
        /// # Raises:
        ///  * 'NotEnoughBalanceToStake':
        /// 	- Not enough balance on the coldkey to add onto the global account.
        ///
        ///  * 'NonAssociatedColdKey':
        /// 	- The calling coldkey is not associated with this hotkey.
        ///
        ///  * 'BalanceWithdrawalError':
        ///  	- Errors stemming from transaction pallet.
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

        /// Remove stake from the staking account. The call must be made
        /// from the coldkey account attached to the neuron metadata. Only this key
        /// has permission to make staking and unstaking requests.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        /// * 'hotkey' (T::AccountId):
        /// 	- The associated hotkey account.
        ///
        /// * 'netuid' (u16):
        ///     - Subnetwork UID
        ///
        /// * 'amount_unstaked' (u64):
        /// 	- The amount of stake to be added to the hotkey staking account.
        ///
        /// # Event:
        /// * StakeRemoved;
        /// 	- On the successfully removing stake from the hotkey account.
        ///
        /// # Raises:
        /// * 'NotRegistered':
        /// 	- Thrown if the account we are attempting to unstake from is non existent.
        ///
        /// * 'NonAssociatedColdKey':
        /// 	- Thrown if the coldkey does not own the hotkey we are unstaking from.
        ///
        /// * 'NotEnoughStakeToWithdraw':
        /// 	- Thrown if there is not enough stake on the hotkey to withdwraw this amount.
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

        /// Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is
        /// already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller.
        ///
        /// * 'netuid' (u16):
        /// 	- The u16 network identifier.
        ///
        /// * 'version' (u64):
        /// 	- The bittensor version identifier.
        ///
        /// * 'ip' (u64):
        /// 	- The endpoint ip information as a u128 encoded integer.
        ///
        /// * 'port' (u16):
        /// 	- The endpoint port information as a u16 encoded integer.
        ///
        /// * 'ip_type' (u8):
        /// 	- The endpoint ip version as a u8, 4 or 6.
        ///
        /// * 'protocol' (u8):
        /// 	- UDP:1 or TCP:0
        ///
        /// * 'placeholder1' (u8):
        /// 	- Placeholder for further extra params.
        ///
        /// * 'placeholder2' (u8):
        /// 	- Placeholder for further extra params.
        ///
        /// # Event:
        /// * AxonServed;
        /// 	- On successfully serving the axon info.
        ///
        /// # Raises:
        /// * 'MechanismDoesNotExist':
        /// 	- Attempting to set weights on a non-existent network.
        ///
        /// * 'NotRegistered':
        /// 	- Attempting to set weights from a non registered account.
        ///
        /// * 'InvalidIpType':
        /// 	- The ip type is not 4 or 6.
        ///
        /// * 'InvalidIpAddress':
        /// 	- The numerically encoded ip address does not resolve to a proper ip.
        ///
        /// * 'ServingRateLimitExceeded':
        /// 	- Attempting to set prometheus information withing the rate limit min.
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

        /// Same as `serve_axon` but takes a certificate as an extra optional argument.
        /// Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is
        /// already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller.
        ///
        /// * 'netuid' (u16):
        /// 	- The u16 network identifier.
        ///
        /// * 'version' (u64):
        /// 	- The bittensor version identifier.
        ///
        /// * 'ip' (u64):
        /// 	- The endpoint ip information as a u128 encoded integer.
        ///
        /// * 'port' (u16):
        /// 	- The endpoint port information as a u16 encoded integer.
        ///
        /// * 'ip_type' (u8):
        /// 	- The endpoint ip version as a u8, 4 or 6.
        ///
        /// * 'protocol' (u8):
        /// 	- UDP:1 or TCP:0
        ///
        /// * 'placeholder1' (u8):
        /// 	- Placeholder for further extra params.
        ///
        /// * 'placeholder2' (u8):
        /// 	- Placeholder for further extra params.
        ///
        /// * 'certificate' (Vec<u8>):
        ///     - TLS certificate for inter neuron communitation.
        ///
        /// # Event:
        /// * AxonServed;
        /// 	- On successfully serving the axon info.
        ///
        /// # Raises:
        /// * 'MechanismDoesNotExist':
        /// 	- Attempting to set weights on a non-existent network.
        ///
        /// * 'NotRegistered':
        /// 	- Attempting to set weights from a non registered account.
        ///
        /// * 'InvalidIpType':
        /// 	- The ip type is not 4 or 6.
        ///
        /// * 'InvalidIpAddress':
        /// 	- The numerically encoded ip address does not resolve to a proper ip.
        ///
        /// * 'ServingRateLimitExceeded':
        /// 	- Attempting to set prometheus information withing the rate limit min.
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

        /// ---- Set prometheus information for the neuron.
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the calling hotkey.
        ///
        /// * 'netuid' (u16):
        /// 	- The u16 network identifier.
        ///
        /// * 'version' (u16):
        /// 	-  The bittensor version identifier.
        ///
        /// * 'ip' (u128):
        /// 	- The prometheus ip information as a u128 encoded integer.
        ///
        /// * 'port' (u16):
        /// 	- The prometheus port information as a u16 encoded integer.
        ///
        /// * 'ip_type' (u8):
        /// 	- The ip type v4 or v6.
        ///
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

        /// ---- Registers a new neuron to the subnetwork.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the calling hotkey.
        ///
        /// * 'netuid' (u16):
        /// 	- The u16 network identifier.
        ///
        /// * 'block_number' ( u64 ):
        /// 	- Block hash used to prove work done.
        ///
        /// * 'nonce' ( u64 ):
        /// 	- Positive integer nonce used in POW.
        ///
        /// * 'work' ( Vec<u8> ):
        /// 	- Vector encoded bytes representing work done.
        ///
        /// * 'hotkey' ( T::AccountId ):
        /// 	- Hotkey to be registered to the network.
        ///
        /// * 'coldkey' ( T::AccountId ):
        /// 	- Associated coldkey account.
        ///
        /// # Event:
        /// * NeuronRegistered;
        /// 	- On successfully registering a uid to a neuron slot on a subnetwork.
        ///
        /// # Raises:
        /// * 'MechanismDoesNotExist':
        /// 	- Attempting to register to a non existent network.
        ///
        /// * 'TooManyRegistrationsThisBlock':
        /// 	- This registration exceeds the total allowed on this network this block.
        ///
        /// * 'HotKeyAlreadyRegisteredInSubNet':
        /// 	- The hotkey is already registered on this network.
        ///
        /// * 'InvalidWorkBlock':
        /// 	- The work has been performed on a stale, future, or non existent block.
        ///
        /// * 'InvalidDifficulty':
        /// 	- The work does not match the difficulty.
        ///
        /// * 'InvalidSeal':
        /// 	- The seal is incorrect.
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

        /// Register the hotkey to root network
        #[pallet::call_index(62)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::root_register())]
        pub fn root_register(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_root_register(origin, hotkey)
        }

        /// User register a new subnetwork via burning token
        #[pallet::call_index(7)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::burned_register())]
        pub fn burned_register(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
        ) -> DispatchResult {
            Self::do_register(origin, netuid, hotkey)
        }

        /// ---- The extrinsic for user to change its hotkey in subnet or all subnets.
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction (must be signed by the coldkey).
        /// * `hotkey` - The old hotkey to be swapped.
        /// * `new_hotkey` - The new hotkey to replace the old one.
        /// * `netuid` - Optional subnet ID. If `Some`, swap only on that subnet; if `None`, swap on all subnets.
        ///   is transferred to the new hotkey.
        #[deprecated(
            note = "Please use swap_hotkey_v2 instead. This extrinsic will be removed some time after June 2026."
        )]
        #[pallet::call_index(70)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::swap_hotkey())]
        pub fn swap_hotkey(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            new_hotkey: T::AccountId,
            netuid: Option<NetUid>,
        ) -> DispatchResultWithPostInfo {
            Self::do_swap_hotkey(origin, &hotkey, &new_hotkey, netuid, false)
        }

        /// ---- The extrinsic for user to change its hotkey in subnet or all subnets. This extrinsic is
        /// similar to swap_hotkey, but with keep_stake parameter bo be able to keep the stake when swapping
        /// a root key to a child key
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction (must be signed by the coldkey).
        /// * `hotkey` - The old hotkey to be swapped.
        /// * `new_hotkey` - The new hotkey to replace the old one.
        /// * `netuid` - Optional subnet ID. If `Some`, swap only on that subnet; if `None`, swap on all subnets.
        /// * `keep_stake` - If `true`, stake remains on the old hotkey and the rest metadata
        ///   is transferred to the new hotkey.
        #[pallet::call_index(72)]
        #[pallet::weight((Weight::from_parts(275_300_000, 0)
        .saturating_add(T::DbWeight::get().reads(52_u64))
        .saturating_add(T::DbWeight::get().writes(35_u64)), DispatchClass::Normal, Pays::Yes))]
        pub fn swap_hotkey_v2(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            new_hotkey: T::AccountId,
            netuid: Option<NetUid>,
            keep_stake: bool,
        ) -> DispatchResultWithPostInfo {
            Self::do_swap_hotkey(origin, &hotkey, &new_hotkey, netuid, keep_stake)
        }

        /// Performs an arbitrary coldkey swap for any coldkey.
        ///
        /// Only callable by root as it doesn't require an announcement and can be used to swap any coldkey.
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

        /// Sets the childkey take for a given hotkey.
        ///
        /// This function allows a coldkey to set the childkey take for a given hotkey.
        /// The childkey take determines the proportion of stake that the hotkey keeps for itself
        /// when distributing stake to its children.
        ///
        /// # Arguments:
        /// * `origin` (<T as frame_system::Config>::RuntimeOrigin):
        ///     - The signature of the calling coldkey. Setting childkey take can only be done by the coldkey.
        ///
        /// * `hotkey` (T::AccountId):
        ///     - The hotkey for which the childkey take will be set.
        ///
        /// * `take` (u16):
        ///     - The new childkey take value. This is a percentage represented as a value between 0 and 10000,
        ///       where 10000 represents 100%.
        ///
        /// # Events:
        /// * `ChildkeyTakeSet`:
        ///     - On successfully setting the childkey take for a hotkey.
        ///
        /// # Errors:
        /// * `NonAssociatedColdKey`:
        ///     - The coldkey does not own the hotkey.
        /// * `InvalidChildkeyTake`:
        ///     - The provided take value is invalid (greater than the maximum allowed take).
        /// * `TxChildkeyTakeRateLimitExceeded`:
        ///     - The rate limit for changing childkey take has been exceeded.
        ///
        #[pallet::call_index(75)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_childkey_take())]
        pub fn set_childkey_take(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            take: u16,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;

            // Call the utility function to set the childkey take
            Self::do_set_childkey_take(coldkey, hotkey, netuid, take)
        }

        // ---- SUDO ONLY FUNCTIONS ------------------------------------------------------------

        /// Sets the transaction rate limit for changing childkey take.
        ///
        /// This function can only be called by the root origin.
        ///
        /// # Arguments:
        /// * `origin` - The origin of the call, must be root.
        /// * `tx_rate_limit` - The new rate limit in blocks.
        ///
        /// # Errors:
        /// * `BadOrigin` - If the origin is not root.
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

        /// Sets the minimum allowed childkey take.
        ///
        /// This function can only be called by the root origin.
        ///
        /// # Arguments:
        /// * `origin` - The origin of the call, must be root.
        /// * `take` - The new minimum childkey take value.
        ///
        /// # Errors:
        /// * `BadOrigin` - If the origin is not root.
        ///
        #[pallet::call_index(76)]
        #[pallet::weight(Weight::from_parts(6_000, 0)
        .saturating_add(T::DbWeight::get().reads(1))
        .saturating_add(T::DbWeight::get().writes(1)))]
        pub fn sudo_set_min_childkey_take(origin: OriginFor<T>, take: u16) -> DispatchResult {
            ensure_root(origin)?;
            Self::set_min_childkey_take(take);
            Ok(())
        }

        /// Sets the maximum allowed childkey take.
        ///
        /// This function can only be called by the root origin.
        ///
        /// # Arguments:
        /// * `origin` - The origin of the call, must be root.
        /// * `take` - The new maximum childkey take value.
        ///
        /// # Errors:
        /// * `BadOrigin` - If the origin is not root.
        ///
        #[pallet::call_index(77)]
        #[pallet::weight(Weight::from_parts(6_000, 0)
        .saturating_add(T::DbWeight::get().reads(1))
        .saturating_add(T::DbWeight::get().writes(1)))]
        pub fn sudo_set_max_childkey_take(origin: OriginFor<T>, take: u16) -> DispatchResult {
            ensure_root(origin)?;
            Self::set_max_childkey_take(take);
            Ok(())
        }

        /// User register a new subnetwork
        #[pallet::call_index(59)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register_network())]
        pub fn register_network(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_register_network(origin, &hotkey, 1, None)
        }

        /// Facility extrinsic for user to get taken from faucet
        /// It is only available when pow-faucet feature enabled
        /// Just deployed in testnet and devnet for testing purpose
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

        /// Remove a user's subnetwork
        /// The caller must be the owner of the network
        #[pallet::call_index(61)]
        #[pallet::weight(Weight::from_parts(119_000_000, 0)
		.saturating_add(T::DbWeight::get().reads(6))
		.saturating_add(T::DbWeight::get().writes(31)))]
        pub fn dissolve_network(
            origin: OriginFor<T>,
            _coldkey: T::AccountId,
            netuid: NetUid,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_dissolve_network(netuid)
        }

        /// Set a single child for a given hotkey on a specified network.
        ///
        /// This function allows a coldkey to set a single child for a given hotkey on a specified network.
        /// The proportion of the hotkey's stake to be allocated to the child is also specified.
        ///
        /// # Arguments:
        /// * `origin` (<T as frame_system::Config>::RuntimeOrigin):
        ///     - The signature of the calling coldkey. Setting a hotkey child can only be done by the coldkey.
        ///
        /// * `hotkey` (T::AccountId):
        ///     - The hotkey which will be assigned the child.
        ///
        /// * `child` (T::AccountId):
        ///     - The child which will be assigned to the hotkey.
        ///
        /// * `netuid` (u16):
        ///     - The u16 network identifier where the childkey will exist.
        ///
        /// * `proportion` (u64):
        ///     - Proportion of the hotkey's stake to be given to the child, the value must be u64 normalized.
        ///
        /// # Events:
        /// * `ChildAddedSingular`:
        ///     - On successfully registering a child to a hotkey.
        ///
        /// # Errors:
        /// * `MechanismDoesNotExist`:
        ///     - Attempting to register to a non-existent network.
        /// * `RegistrationNotPermittedOnRootSubnet`:
        ///     - Attempting to register a child on the root network.
        /// * `NonAssociatedColdKey`:
        ///     - The coldkey does not own the hotkey or the child is the same as the hotkey.
        /// * `HotKeyAccountNotExists`:
        ///     - The hotkey account does not exist.
        ///
        /// # Detailed Explanation of Checks:
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
        #[pallet::weight((Weight::from_parts(119_000_000, 0)
		.saturating_add(T::DbWeight::get().reads(6))
		.saturating_add(T::DbWeight::get().writes(31)), DispatchClass::Normal, Pays::Yes))]
        pub fn set_children(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            netuid: NetUid,
            children: Vec<(u64, T::AccountId)>,
        ) -> DispatchResultWithPostInfo {
            Self::do_schedule_children(origin, hotkey, netuid, children)?;
            Ok(().into())
        }

        /// Schedules a coldkey swap operation to be executed at a future block.
        ///
        /// WARNING: This function is deprecated, please migrate to `announce_coldkey_swap`/`coldkey_swap`
        #[pallet::call_index(73)]
        #[pallet::weight(T::DbWeight::get().reads(5))]
        #[deprecated(note = "Deprecated, please migrate to `announce_coldkey_swap`/`coldkey_swap`")]
        pub fn schedule_swap_coldkey(
            _origin: OriginFor<T>,
            _new_coldkey: T::AccountId,
        ) -> DispatchResult {
            Err(Error::<T>::Deprecated.into())
        }

        /// ---- Set prometheus information for the neuron.
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the calling hotkey.
        ///
        /// * 'netuid' (u16):
        /// 	- The u16 network identifier.
        ///
        /// * 'version' (u16):
        /// 	-  The bittensor version identifier.
        ///
        /// * 'ip' (u128):
        /// 	- The prometheus ip information as a u128 encoded integer.
        ///
        /// * 'port' (u16):
        /// 	- The prometheus port information as a u16 encoded integer.
        ///
        /// * 'ip_type' (u8):
        /// 	- The ip type v4 or v6.
        ///
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

        /// ---- Set the identity information for a subnet.
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the calling coldkey, which must be the owner of the subnet.
        ///
        /// * `netuid` (u16):
        ///     - The unique network identifier of the subnet.
        ///
        /// * `subnet_name` (Vec<u8>):
        ///     - The name of the subnet.
        ///
        /// * `github_repo` (Vec<u8>):
        ///     - The GitHub repository associated with the subnet identity.
        ///
        /// * `subnet_contact` (Vec<u8>):
        ///     - The contact information for the subnet.
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

        /// User register a new subnetwork
        #[pallet::call_index(79)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register_network_with_identity())]
        pub fn register_network_with_identity(
            origin: OriginFor<T>,
            hotkey: T::AccountId,
            identity: Option<SubnetIdentityOfV3>,
        ) -> DispatchResult {
            Self::do_register_network(origin, &hotkey, 1, identity)
        }

        /// ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
        ///
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the caller's coldkey.
        ///
        /// * `hotkey` (T::AccountId):
        ///     - The associated hotkey account.
        ///
        /// # Event:
        /// * StakeRemoved;
        ///     - On the successfully removing stake from the hotkey account.
        ///
        /// # Raises:
        /// * `NotRegistered`:
        ///     - Thrown if the account we are attempting to unstake from is non existent.
        ///
        /// * `NonAssociatedColdKey`:
        ///     - Thrown if the coldkey does not own the hotkey we are unstaking from.
        ///
        /// * `NotEnoughStakeToWithdraw`:
        ///     - Thrown if there is not enough stake on the hotkey to withdraw this amount.
        ///
        /// * `TxRateLimitExceeded`:
        ///     - Thrown if key has hit transaction rate limit
        #[pallet::call_index(83)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::unstake_all())]
        pub fn unstake_all(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_unstake_all(origin, hotkey)
        }

        /// ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
        ///
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the caller's coldkey.
        ///
        /// * `hotkey` (T::AccountId):
        ///     - The associated hotkey account.
        ///
        /// # Event:
        /// * StakeRemoved;
        ///     - On the successfully removing stake from the hotkey account.
        ///
        /// # Raises:
        /// * `NotRegistered`:
        ///     - Thrown if the account we are attempting to unstake from is non existent.
        ///
        /// * `NonAssociatedColdKey`:
        ///     - Thrown if the coldkey does not own the hotkey we are unstaking from.
        ///
        /// * `NotEnoughStakeToWithdraw`:
        ///     - Thrown if there is not enough stake on the hotkey to withdraw this amount.
        ///
        /// * `TxRateLimitExceeded`:
        ///     - Thrown if key has hit transaction rate limit
        #[pallet::call_index(84)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::unstake_all_alpha())]
        pub fn unstake_all_alpha(origin: OriginFor<T>, hotkey: T::AccountId) -> DispatchResult {
            Self::do_unstake_all_alpha(origin, hotkey)
        }

        /// ---- The implementation for the extrinsic move_stake: Moves specified amount of stake from a hotkey to another across subnets.
        ///
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the caller's coldkey.
        ///
        /// * `origin_hotkey` (T::AccountId):
        ///     - The hotkey account to move stake from.
        ///
        /// * `destination_hotkey` (T::AccountId):
        ///     - The hotkey account to move stake to.
        ///
        /// * `origin_netuid` (T::AccountId):
        ///     - The subnet ID to move stake from.
        ///
        /// * `destination_netuid` (T::AccountId):
        ///     - The subnet ID to move stake to.
        ///
        /// * `alpha_amount` (T::AccountId):
        ///     - The alpha stake amount to move.
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

        /// Transfers a specified amount of stake from one coldkey to another, optionally across subnets,
        /// while keeping the same hotkey.
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction, which must be signed by the `origin_coldkey`.
        /// * `destination_coldkey` - The coldkey to which the stake is transferred.
        /// * `hotkey` - The hotkey associated with the stake.
        /// * `origin_netuid` - The network/subnet ID to move stake from.
        /// * `destination_netuid` - The network/subnet ID to move stake to (for cross-subnet transfer).
        /// * `alpha_amount` - The amount of stake to transfer.
        ///
        /// # Errors
        /// Returns an error if:
        /// * The origin is not signed by the correct coldkey.
        /// * Either subnet does not exist.
        /// * The hotkey does not exist.
        /// * There is insufficient stake on `(origin_coldkey, hotkey, origin_netuid)`.
        /// * The transfer amount is below the minimum stake requirement.
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

        /// Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
        /// * `hotkey` - The hotkey whose stake is being swapped.
        /// * `origin_netuid` - The network/subnet ID from which stake is removed.
        /// * `destination_netuid` - The network/subnet ID to which stake is added.
        /// * `alpha_amount` - The amount of stake to swap.
        ///
        /// # Errors
        /// Returns an error if:
        /// * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails).
        /// * Either `origin_netuid` or `destination_netuid` does not exist.
        /// * The hotkey does not exist.
        /// * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`.
        /// * The swap amount is below the minimum stake requirement.
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

        /// --- Adds stake to a hotkey on a subnet with a price limit.
        /// This extrinsic allows to specify the limit price for alpha token
        /// at which or better (lower) the staking should execute.
        ///
        /// In case if slippage occurs and the price shall move beyond the limit
        /// price, the staking order may execute only partially or not execute
        /// at all.
        ///
        /// # Args:
        ///  * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        ///  * 'hotkey' (T::AccountId):
        /// 	- The associated hotkey account.
        ///
        /// * 'netuid' (u16):
        ///     - Subnetwork UID
        ///
        ///  * 'amount_staked' (u64):
        /// 	- The amount of stake to be added to the hotkey staking account.
        ///
        ///  * 'limit_price' (u64):
        /// 	- The limit price expressed in units of RAO per one Alpha.
        ///
        ///  * 'allow_partial' (bool):
        /// 	- Allows partial execution of the amount. If set to false, this becomes
        ///       fill or kill type or order.
        ///
        /// # Event:
        ///  * StakeAdded;
        /// 	- On the successfully adding stake to a global account.
        ///
        /// # Raises:
        ///  * 'NotEnoughBalanceToStake':
        /// 	- Not enough balance on the coldkey to add onto the global account.
        ///
        ///  * 'NonAssociatedColdKey':
        /// 	- The calling coldkey is not associated with this hotkey.
        ///
        ///  * 'BalanceWithdrawalError':
        ///  	- Errors stemming from transaction pallet.
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

        /// --- Removes stake from a hotkey on a subnet with a price limit.
        /// This extrinsic allows to specify the limit price for alpha token
        /// at which or better (higher) the staking should execute.
        ///
        /// In case if slippage occurs and the price shall move beyond the limit
        /// price, the staking order may execute only partially or not execute
        /// at all.
        ///
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        /// * 'hotkey' (T::AccountId):
        /// 	- The associated hotkey account.
        ///
        /// * 'netuid' (u16):
        ///     - Subnetwork UID
        ///
        /// * 'amount_unstaked' (u64):
        /// 	- The amount of stake to be added to the hotkey staking account.
        ///
        ///  * 'limit_price' (u64):
        ///     - The limit price expressed in units of RAO per one Alpha.
        ///
        ///  * 'allow_partial' (bool):
        ///     - Allows partial execution of the amount. If set to false, this becomes
        ///       fill or kill type or order.
        ///
        /// # Event:
        /// * StakeRemoved;
        /// 	- On the successfully removing stake from the hotkey account.
        ///
        /// # Raises:
        /// * 'NotRegistered':
        /// 	- Thrown if the account we are attempting to unstake from is non existent.
        ///
        /// * 'NonAssociatedColdKey':
        /// 	- Thrown if the coldkey does not own the hotkey we are unstaking from.
        ///
        /// * 'NotEnoughStakeToWithdraw':
        /// 	- Thrown if there is not enough stake on the hotkey to withdwraw this amount.
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

        /// Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
        /// * `hotkey` - The hotkey whose stake is being swapped.
        /// * `origin_netuid` - The network/subnet ID from which stake is removed.
        /// * `destination_netuid` - The network/subnet ID to which stake is added.
        /// * `alpha_amount` - The amount of stake to swap.
        /// * `limit_price` - The limit price expressed in units of RAO per one Alpha.
        /// * `allow_partial` - Allows partial execution of the amount. If set to false, this becomes fill or kill type or order.
        ///
        /// # Errors
        /// Returns an error if:
        /// * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails).
        /// * Either `origin_netuid` or `destination_netuid` does not exist.
        /// * The hotkey does not exist.
        /// * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`.
        /// * The swap amount is below the minimum stake requirement.
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

        /// Attempts to associate a hotkey with a coldkey.
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
        /// * `hotkey` - The hotkey to associate with the coldkey.
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

        /// Initiates a call on a subnet.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be signed by the subnet owner.
        /// * `netuid` - The unique identifier of the subnet on which the call is being initiated.
        ///
        /// # Events
        /// Emits a `FirstEmissionBlockNumberSet` event on success.
        #[pallet::call_index(92)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::start_call())]
        pub fn start_call(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            Self::do_start_call(origin, netuid)?;
            Ok(())
        }

        /// Attempts to associate a hotkey with an EVM key.
        ///
        /// The signature will be checked to see if the recovered public key matches the `evm_key` provided.
        ///
        /// The EVM key is expected to sign the message according to this formula to produce the signature:
        /// ```text
        /// keccak_256(hotkey ++ keccak_256(block_number))
        /// ```
        ///
        /// # Arguments
        /// * `origin` - The origin of the transaction, which must be signed by the `hotkey`.
        /// * `netuid` - The netuid that the `hotkey` belongs to.
        /// * `evm_key` - The EVM key to associate with the `hotkey`.
        /// * `block_number` - The block number used in the `signature`.
        /// * `signature` - A signed message by the `evm_key` containing the `hotkey` and the hashed `block_number`.
        ///
        /// # Errors
        /// Returns an error if:
        /// * The transaction is not signed.
        /// * The hotkey does not belong to the subnet identified by the netuid.
        /// * The EVM key cannot be recovered from the signature.
        /// * The EVM key recovered from the signature does not match the given EVM key.
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

        /// Recycles alpha from a cold/hot key pair, reducing AlphaOut on a subnet
        ///
        /// # Arguments
        /// * `origin` - The origin of the call (must be signed by the coldkey)
        /// * `hotkey` - The hotkey account
        /// * `amount` - The amount of alpha to recycle
        /// * `netuid` - The subnet ID
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

        /// Burns alpha from a cold/hot key pair without reducing `AlphaOut`
        ///
        /// # Arguments
        /// * `origin` - The origin of the call (must be signed by the coldkey)
        /// * `hotkey` - The hotkey account
        /// * `amount` - The amount of alpha to burn
        /// * `netuid` - The subnet ID
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

        /// Sets the pending childkey cooldown (in blocks). Root only.
        #[pallet::call_index(109)]
        #[pallet::weight(Weight::from_parts(3_000_000, 0)
			.saturating_add(T::DbWeight::get().writes(1_u64)))]
        pub fn set_pending_childkey_cooldown(
            origin: OriginFor<T>,
            cooldown: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            PendingChildKeyCooldown::<T>::put(cooldown);
            Ok(())
        }

        /// Removes all stake from a hotkey on a subnet with a price limit.
        /// This extrinsic allows to specify the limit price for alpha token
        /// at which or better (higher) the staking should execute.
        /// Without limit_price it remove all the stake similar to `remove_stake` extrinsic
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

        /// Register a new leased network.
        ///
        /// The crowdloan's contributions are used to compute the share of the emissions that the contributors
        /// will receive as dividends.
        ///
        /// The leftover cap is refunded to the contributors and the beneficiary.
        ///
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the caller's coldkey.
        ///
        /// * `emissions_share` (Percent):
        ///     - The share of the emissions that the contributors will receive as dividends.
        ///
        /// * `end_block` (Option<BlockNumberFor<T>>):
        ///     - The block at which the lease will end. If not defined, the lease is perpetual.
        #[pallet::call_index(110)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::register_leased_network(T::MaxContributors::get()))]
        pub fn register_leased_network(
            origin: OriginFor<T>,
            emissions_share: Percent,
            end_block: Option<BlockNumberFor<T>>,
        ) -> DispatchResultWithPostInfo {
            Self::do_register_leased_network(origin, emissions_share, end_block)
        }

        /// Terminate a lease.
        ///
        /// The beneficiary can terminate the lease after the end block has passed and get the subnet ownership.
        /// The subnet is transferred to the beneficiary and the lease is removed from storage.
        ///
        /// **The hotkey must be owned by the beneficiary coldkey.**
        ///
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the caller's coldkey.
        ///
        /// * `lease_id` (LeaseId):
        ///     - The ID of the lease to terminate.
        ///
        /// * `hotkey` (T::AccountId):
        ///     - The hotkey of the beneficiary to mark as subnet owner hotkey.
        #[pallet::call_index(111)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::terminate_lease(T::MaxContributors::get()))]
        pub fn terminate_lease(
            origin: OriginFor<T>,
            lease_id: LeaseId,
            hotkey: T::AccountId,
        ) -> DispatchResultWithPostInfo {
            Self::do_terminate_lease(origin, lease_id, hotkey)
        }

        /// Updates the symbol for a subnet.
        ///
        /// # Arguments
        /// * `origin` - The origin of the call, which must be the subnet owner or root.
        /// * `netuid` - The unique identifier of the subnet on which the symbol is being set.
        /// * `symbol` - The symbol to set for the subnet.
        ///
        /// # Errors
        /// Returns an error if:
        /// * The transaction is not signed by the subnet owner.
        /// * The symbol does not exist.
        /// * The symbol is already in use by another subnet.
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

            Self::ensure_symbol_exists(&symbol)?;
            Self::ensure_symbol_available(&symbol)?;

            TokenSymbol::<T>::insert(netuid, symbol.clone());

            Self::deposit_event(Event::SymbolUpdated { netuid, symbol });
            Ok(())
        }

        /// ---- Used to commit timelock encrypted commit-reveal weight values to later be revealed.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The committing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `commit` (`Vec<u8>`):
        ///   - The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * reveal_round (`u64`):
        ///    - The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// * commit_reveal_version (`u16`):
        ///     - The client (bittensor-drand) version
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

        /// Set the autostake destination hotkey for a coldkey.
        ///
        /// The caller selects a hotkey where all future rewards
        /// will be automatically staked.
        ///
        /// # Args:
        /// * `origin` - (<T as frame_system::Config>::Origin):
        ///     - The signature of the caller's coldkey.
        ///
        /// * `hotkey` (T::AccountId):
        ///     - The hotkey account to designate as the autostake destination.
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

        /// ---- Used to commit timelock encrypted commit-reveal weight values to later be revealed for
        /// a mechanism.
        ///
        /// # Args:
        /// * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
        ///   - The committing hotkey.
        ///
        /// * `netuid` (`u16`):
        ///   - The u16 network identifier.
        ///
        /// * `mecid` (`u8`):
        ///   - The u8 mechanism identifier.
        ///
        /// * `commit` (`Vec<u8>`):
        ///   - The encrypted compressed commit.
        ///     The steps for this are:
        ///     1. Instantiate [`WeightsTlockPayload`]
        ///     2. Serialize it using the `parity_scale_codec::Encode` trait
        ///     3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
        ///        to produce a [`TLECiphertext<TinyBLS381>`] type.
        ///     4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
        ///
        /// * reveal_round (`u64`):
        ///    - The drand reveal round which will be avaliable during epoch `n+1` from the current
        ///      epoch.
        ///
        /// * commit_reveal_version (`u16`):
        ///     - The client (bittensor-drand) version
        #[pallet::call_index(118)]
        #[pallet::weight((Weight::from_parts(84_020_000, 0)
		.saturating_add(T::DbWeight::get().reads(9_u64))
		.saturating_add(T::DbWeight::get().writes(2)), DispatchClass::Normal, Pays::No))]
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

        /// Remove a subnetwork
        /// The caller must be root
        #[pallet::call_index(120)]
        #[pallet::weight(Weight::from_parts(119_000_000, 0)
		.saturating_add(T::DbWeight::get().reads(6))
		.saturating_add(T::DbWeight::get().writes(31)))]
        pub fn root_dissolve_network(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_dissolve_network(netuid)
        }

        /// --- Claims the root emissions for a coldkey.
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        /// # Event:
        /// * RootClaimed;
        /// 	- On the successfully claiming the root emissions for a coldkey.
        ///
        /// # Raises:
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

        /// --- Sets the root claim type for the coldkey.
        /// # Args:
        /// * 'origin': (<T as frame_system::Config>Origin):
        /// 	- The signature of the caller's coldkey.
        ///
        /// # Event:
        /// * RootClaimTypeSet;
        /// 	- On the successfully setting the root claim type for the coldkey.
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

        /// --- Sets root claim number (sudo extrinsic). Zero disables auto-claim.
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

        /// --- Sets root claim threshold for subnet (sudo or owner origin).
        #[pallet::call_index(124)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::sudo_set_root_claim_threshold())]
        pub fn sudo_set_root_claim_threshold(
            origin: OriginFor<T>,
            netuid: NetUid,
            new_value: u64,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;

            ensure!(
                new_value <= I96F32::from(MAX_ROOT_CLAIM_THRESHOLD),
                Error::<T>::InvalidRootClaimThreshold
            );

            RootClaimableThreshold::<T>::set(netuid, new_value.into());

            Ok(())
        }

        /// Announces a coldkey swap using BlakeTwo256 hash of the new coldkey.
        ///
        /// This is required before the coldkey swap can be performed
        /// after the delay period.
        ///
        /// It can be reannounced after a delay of `ColdkeySwapReannouncementDelay` following
        /// the first valid execution block of the original announcement.
        ///
        /// The dispatch origin of this call must be the original coldkey that made the announcement.
        ///
        /// - `new_coldkey_hash`: The hash of the new coldkey using BlakeTwo256.
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

        /// Performs a coldkey swap if an announcement has been made.
        ///
        /// The dispatch origin of this call must be the original coldkey that made the announcement.
        ///
        /// - `new_coldkey`: The new coldkey to swap to. The BlakeTwo256 hash of the new coldkey must be
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

        /// Dispute a coldkey swap.
        ///
        /// This will prevent any further actions on the coldkey swap
        /// until triumvirate step in to resolve the issue.
        ///
        /// - `coldkey`: The coldkey to dispute the swap for.
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

        /// Reset a coldkey swap by clearing the announcement and dispute status.
        ///
        /// The dispatch origin of this call must be root.
        ///
        /// - `coldkey`: The coldkey to reset the swap for.
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

        /// Enables voting power tracking for a subnet.
        ///
        /// This function can be called by the subnet owner or root.
        /// When enabled, voting power EMA is updated every epoch for all validators.
        /// Voting power starts at 0 and increases over epochs.
        ///
        /// # Arguments:
        /// * `origin` - The origin of the call, must be subnet owner or root.
        /// * `netuid` - The subnet to enable voting power tracking for.
        ///
        /// # Errors:
        /// * `SubnetNotExist` - If the subnet does not exist.
        /// * `NotSubnetOwner` - If the caller is not the subnet owner or root.
        #[pallet::call_index(129)]
        #[pallet::weight(Weight::from_parts(10_000, 0)
        .saturating_add(T::DbWeight::get().reads(2))
        .saturating_add(T::DbWeight::get().writes(2)))]
        pub fn enable_voting_power_tracking(
            origin: OriginFor<T>,
            netuid: NetUid,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;
            Self::do_enable_voting_power_tracking(netuid)
        }

        /// Schedules disabling of voting power tracking for a subnet.
        ///
        /// This function can be called by the subnet owner or root.
        /// Voting power tracking will continue for 14 days (grace period) after this call,
        /// then automatically disable and clear all VotingPower entries for the subnet.
        ///
        /// # Arguments:
        /// * `origin` - The origin of the call, must be subnet owner or root.
        /// * `netuid` - The subnet to schedule disabling voting power tracking for.
        ///
        /// # Errors:
        /// * `SubnetNotExist` - If the subnet does not exist.
        /// * `NotSubnetOwner` - If the caller is not the subnet owner or root.
        /// * `VotingPowerTrackingNotEnabled` - If voting power tracking is not enabled.
        #[pallet::call_index(130)]
        #[pallet::weight(Weight::from_parts(10_000, 0)
        .saturating_add(T::DbWeight::get().reads(2))
        .saturating_add(T::DbWeight::get().writes(1)))]
        pub fn disable_voting_power_tracking(
            origin: OriginFor<T>,
            netuid: NetUid,
        ) -> DispatchResult {
            Self::ensure_subnet_owner_or_root(origin, netuid)?;
            Self::do_disable_voting_power_tracking(netuid)
        }

        /// Sets the EMA alpha value for voting power calculation on a subnet.
        ///
        /// This function can only be called by root (sudo).
        /// Higher alpha = faster response to stake changes.
        /// Alpha is stored as u64 with 18 decimal precision (1.0 = 10^18).
        ///
        /// # Arguments:
        /// * `origin` - The origin of the call, must be root.
        /// * `netuid` - The subnet to set the alpha for.
        /// * `alpha` - The new alpha value (u64 with 18 decimal precision).
        ///
        /// # Errors:
        /// * `BadOrigin` - If the origin is not root.
        /// * `SubnetNotExist` - If the subnet does not exist.
        /// * `InvalidVotingPowerEmaAlpha` - If alpha is greater than 10^18 (1.0).
        #[pallet::call_index(131)]
        #[pallet::weight(Weight::from_parts(6_000, 0)
        .saturating_add(T::DbWeight::get().reads(1))
        .saturating_add(T::DbWeight::get().writes(1)))]
        pub fn sudo_set_voting_power_ema_alpha(
            origin: OriginFor<T>,
            netuid: NetUid,
            alpha: u64,
        ) -> DispatchResult {
            ensure_root(origin)?;
            Self::do_set_voting_power_ema_alpha(netuid, alpha)
        }

        /// --- The extrinsic is a combination of add_stake(add_stake_limit) and burn_alpha. We buy
        /// alpha token first and immediately burn the acquired amount of alpha (aka Subnet buyback).
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
        #[pallet::weight(Weight::from_parts(17_890_000, 0)
        .saturating_add(T::DbWeight::get().reads(2))
        .saturating_add(T::DbWeight::get().writes(1)))]
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

        /// User register a new subnetwork via burning token, but only if the
        /// on-chain burn price for this block is <= `limit_price`.
        ///
        /// `limit_price` is expressed in the same TaoCurrency/u64 units as `Burn`.
        #[pallet::call_index(134)]
        #[pallet::weight((
            Weight::from_parts(354_200_000, 0)
                .saturating_add(T::DbWeight::get().reads(47_u64))
                .saturating_add(T::DbWeight::get().writes(40_u64)),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        pub fn register_limit(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
            limit_price: u64,
        ) -> DispatchResult {
            Self::do_register_limit(origin, netuid, hotkey, limit_price)
        }

        /// --- Allows a root validator to toggle auto parent delegation
        /// for new subnets owner hotkey
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
        /// * `origin` - Must be signed by the coldkey.
        /// * `hotkey` - The hotkey to lock stake to.
        /// * `netuid` - The subnet on which to lock.
        /// * `amount` - The alpha amount to lock.
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
        /// * `origin` - Must be signed by the coldkey that owns the lock.
        /// * `destination_hotkey` - The hotkey the lock should target after the move.
        /// * `netuid` - The subnet on which the lock exists.
        /// # Errors:
        /// * `Error::<T>::NoExistingLock` - If no lock exists for the given coldkey and subnet.
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
        #[pallet::weight(<T as frame_system::Config>::DbWeight::get().reads_writes(4, 3))]
        pub fn set_perpetual_lock(
            origin: OriginFor<T>,
            netuid: NetUid,
            enabled: bool,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            Self::do_set_perpetual_lock(&coldkey, netuid, enabled)
        }

        /// Owner-side `set_tempo`. Validates `[MinTempo, MaxTempo]`, applies a fixed
        /// `MinTempo`-block cooldown via `TransactionType::TempoUpdate`, respects the admin
        /// freeze window, and resets the cycle (`LastEpochBlock = current_block`) on success.
        #[pallet::call_index(139)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_tempo())]
        pub fn set_tempo(origin: OriginFor<T>, netuid: NetUid, tempo: u16) -> DispatchResult {
            Self::do_set_tempo(origin, netuid, tempo)
        }

        /// `set_activity_cutoff_factor`. Per-mille (1/1000) units; `cutoff_blocks
        /// = (factor × tempo) / 1000`. Validates `[MinActivityCutoffFactorMilli,
        /// MaxActivityCutoffFactorMilli]`. Callable by the subnet owner (rate-limited
        /// via `OwnerHyperparamUpdate`, respects the admin freeze window) or by root
        /// (bypasses both).
        #[pallet::call_index(140)]
        #[pallet::weight(<T as crate::pallet::Config>::WeightInfo::set_activity_cutoff_factor())]
        pub fn set_activity_cutoff_factor(
            origin: OriginFor<T>,
            netuid: NetUid,
            factor_milli: u32,
        ) -> DispatchResult {
            Self::do_set_activity_cutoff_factor(origin, netuid, factor_milli)
        }

        /// Owner-side `trigger_epoch`. Schedules an epoch to fire after `AdminFreezeWindow`
        /// blocks. Rate-limited via the existing `OwnerHyperparamUpdate` pattern.
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
        #[pallet::weight((
            <T as frame_system::Config>::DbWeight::get().reads_writes(1, 1),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        pub fn set_reject_locked_alpha(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            Self::set_accept_locked_alpha(&coldkey, !enabled);
            Self::deposit_event(Event::RejectLockedAlphaUpdated { coldkey, enabled });
            Ok(())
        }
    }
}
