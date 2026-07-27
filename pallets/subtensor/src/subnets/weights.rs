use super::*;
use crate::epoch::math::*;
use crate::{Error, MAX_COMMIT_REVEAL_PEROIDS, MIN_COMMIT_REVEAL_PEROIDS};
use codec::Compact;
use frame_support::dispatch::DispatchResult;
use safe_math::*;
use sp_core::{ConstU32, H256};
use sp_runtime::{
    BoundedVec,
    traits::{BlakeTwo256, Hash},
};
use sp_std::{collections::vec_deque::VecDeque, vec};
use subtensor_runtime_common::{MechId, NetUid, NetUidStorageIndex};

impl<T: Config> Pallet<T> {
    /// ---- The implementation for committing weight hashes.
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
    ///   - Raised if commit-reveal is disabled for the specified network.
    ///
    /// * `HotKeyNotRegisteredInSubNet`:
    ///   - Raised if the hotkey is not registered on the specified network.
    ///
    /// * `CommittingWeightsTooFast`:
    ///   - Raised if the hotkey's commit rate exceeds the permitted limit.
    ///
    /// * `TooManyUnrevealedCommits`:
    ///   - Raised if the hotkey has reached the maximum number of unrevealed commits.
    ///
    /// # Events:
    /// * `WeightsCommitted`:
    ///   - Emitted upon successfully storing the weight hash.
    pub fn do_commit_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        commit_hash: H256,
    ) -> DispatchResult {
        Self::internal_commit_weights(origin, netuid, MechId::MAIN, commit_hash)
    }

    pub fn do_commit_mechanism_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        commit_hash: H256,
    ) -> DispatchResult {
        Self::internal_commit_weights(origin, netuid, mecid, commit_hash)
    }

    fn internal_commit_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        commit_hash: H256,
    ) -> DispatchResult {
        // Ensure netuid and mecid exist
        Self::ensure_mechanism_exists(netuid, mecid)?;

        // Calculate subnet storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // 1. Verify the caller's signature (hotkey).
        let who = ensure_signed(origin)?;

        log::debug!("do_commit_weights(hotkey: {who:?}, netuid: {netuid:?})");

        // 2. Ensure commit-reveal is enabled.
        ensure!(
            Self::get_commit_reveal_weights_enabled(netuid),
            Error::<T>::CommitRevealDisabled
        );

        // 3. Ensure the hotkey is registered on the network.
        ensure!(
            Self::is_hotkey_registered_on_network(netuid, &who),
            Error::<T>::HotKeyNotRegisteredInSubNet
        );

        // 4. Check that the commit rate does not exceed the allowed frequency.
        let commit_block = Self::get_current_block_as_u64();
        let neuron_uid = Self::get_uid_for_net_and_hotkey(netuid, &who)?;
        ensure!(
            // Rate limiting should happen per sub-subnet, so use netuid_index here
            Self::check_rate_limit(netuid_index, neuron_uid, commit_block),
            Error::<T>::CommittingWeightsTooFast
        );

        // 5. Resolve the epoch this commit belongs to under the stateful counter.
        let commit_epoch = Self::current_epoch_with_lookahead(netuid);

        // 6. Retrieve or initialize the VecDeque of commits for the hotkey.
        WeightCommits::<T>::try_mutate(netuid_index, &who, |maybe_commits| -> DispatchResult {
            // Tuple shape `(hash, commit_epoch, commit_block, _)`. `commit_epoch`
            // drives reveal-window timing; `commit_block` is kept for the epoch's
            // commit-reveal weight column-mask. The 4th field is a legacy
            // reveal-block bound, now unused and left at 0.
            let mut commits: VecDeque<(H256, u64, u64, u64)> =
                maybe_commits.take().unwrap_or_default();

            // 7. Remove any expired commits from the front of the queue.
            while let Some((_, commit_epoch_existing, _, _)) = commits.front() {
                if Self::is_commit_expired(netuid, *commit_epoch_existing) {
                    commits.pop_front();
                } else {
                    break;
                }
            }

            // 8. Verify that the number of unrevealed commits is within the allowed limit.
            ensure!(commits.len() < 10, Error::<T>::TooManyUnrevealedCommits);

            // 9. Append the new commit, tagged with its epoch and block.
            commits.push_back((commit_hash, commit_epoch, commit_block, 0));

            // 10. Store the updated commits queue back to storage.
            *maybe_commits = Some(commits);

            // 11. Emit the WeightsCommitted event
            Self::deposit_event(Event::WeightsCommitted(
                who.clone(),
                netuid_index,
                commit_hash,
            ));

            // 12. Update the last commit block for the hotkey's UID.
            Self::set_last_update_for_uid(netuid_index, neuron_uid, commit_block);

            // 13. Return success.
            Ok(())
        })
    }

    /// ---- The implementation for the extrinsic batch_commit_weights.
    ///
    /// This call runs a batch of commit weights calls, continuing on errors.
    ///
    /// # Args:
    ///  * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///    - The signature of the calling hotkey.
    ///
    ///  * 'netuids' ( Vec<Compact<u16>> ):
    ///    - The u16 network identifiers.
    ///
    ///  * 'commit_hashes' ( Vec<H256> ):
    ///    - The commit hashes to be committed, one hash for each netuid in the batch.
    ///
    /// # Event:
    ///  * WeightsCommitted;
    ///    - On successfully storing the weight hashes.
    ///  * BatchCompletedWithErrors;
    ///    - Emitted when at least on of the weight commits has an error.
    ///  * BatchWeightItemFailed;
    ///    - Emitted for each error within the batch.
    ///  * BatchWeightsCompleted
    ///    - Emitted when the batch of weights is completed.
    ///  * InputLengthsUnequal;
    ///    - Emitted when the lengths of the input vectors are not equal.
    ///
    pub fn do_batch_commit_weights(
        origin: OriginFor<T>,
        netuids: Vec<Compact<NetUid>>,
        commit_hashes: Vec<H256>,
    ) -> dispatch::DispatchResult {
        // --- 1. Check the caller's signature. This is the hotkey of a registered account.
        let hotkey = ensure_signed(origin.clone())?;
        log::debug!(
            "do_batch_commit_weights( origin:{hotkey:?}, netuids:{netuids:?}, hashes:{commit_hashes:?} )"
        );

        ensure!(
            netuids.len() == commit_hashes.len(),
            Error::<T>::InputLengthsUnequal
        );

        let results: Vec<(NetUid, dispatch::DispatchResult)> = netuids
            .iter()
            .zip(commit_hashes.iter())
            .map(|(&netuid, &commit_hash)| {
                let origin_cloned = origin.clone();
                let netuid: NetUid = netuid.into();
                (
                    netuid,
                    Self::do_commit_weights(origin_cloned, netuid, commit_hash),
                )
            })
            .collect();

        let mut completed_with_errors: bool = false;
        for (netuid, result) in results {
            if let Some(err) = result.err() {
                if !completed_with_errors {
                    Self::deposit_event(Event::BatchCompletedWithErrors());
                    completed_with_errors = true;
                }
                Self::deposit_event(Event::BatchWeightItemFailed(netuid, err));
            }
        }

        // --- 19. Emit the tracking event.
        log::debug!("BatchWeightsCompleted( netuids:{netuids:?}, hotkey:{hotkey:?} )");
        Self::deposit_event(Event::BatchWeightsCompleted(netuids, hotkey));

        // --- 20. Return ok.
        Ok(())
    }

    /// ---- Commits a timelocked, encrypted weight payload (Commit-Reveal v3).
    ///
    /// # Args
    /// * `origin` (`<T as frame_system::Config>::RuntimeOrigin`):
    ///   The signed origin of the committing hotkey.
    /// * `netuid` (`NetUid` = `u16`):
    ///   Unique identifier for the subnet on which the commit is made.
    /// * `commit` (`BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>`):
    ///   The encrypted weight payload, produced as follows:
    ///   1. Build a [`WeightsPayload`] structure.
    ///   2. SCALE-encode it (`parity_scale_codec::Encode`).
    ///   3. Encrypt it following the steps
    ///      [here](https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336) to
    ///      produce a [`TLECiphertext<TinyBLS381>`].
    ///   4. Compress & serialise.
    /// * `reveal_round` (`u64`):
    ///   DRAND round whose output becomes known during epoch `n + 1`; the payload
    ///   must be revealed in that epoch.
    /// * `commit_reveal_version` (`u16`):
    ///   Version tag that **must** match [`get_commit_reveal_weights_version`] for
    ///   the call to succeed. Used to gate runtime upgrades.
    ///
    /// # Behaviour
    /// 1. Verifies the caller’s signature and registration on `netuid`.
    /// 2. Ensures commit-reveal is enabled **and** the supplied
    ///    `commit_reveal_version` is current.
    /// 3. Enforces per-neuron rate-limiting via [`Pallet::check_rate_limit`].
    /// 4. Rejects the call when the hotkey already has ≥ 10 unrevealed commits in
    ///    the current epoch.
    /// 5. Appends `(hotkey, commit_block, commit, reveal_round)` to
    ///    `TimelockedWeightCommits[netuid][epoch]`.
    /// 6. Emits `TimelockedWeightsCommitted` with the Blake2 hash of `commit`.
    /// 7. Updates `LastUpdateForUid` so subsequent rate-limit checks include this
    ///    commit.
    ///
    /// # Raises
    /// * `CommitRevealDisabled` – Commit-reveal is disabled on `netuid`.
    /// * `IncorrectCommitRevealVersion` – Provided version ≠ runtime version.
    /// * `HotKeyNotRegisteredInSubNet` – Caller’s hotkey is not registered.
    /// * `CommittingWeightsTooFast` – Caller exceeds commit-rate limit.
    /// * `TooManyUnrevealedCommits` – Caller already has 10 unrevealed commits.
    ///
    /// # Events
    /// * `TimelockedWeightsCommitted(hotkey, netuid, commit_hash, reveal_round)` – Fired after the commit is successfully stored.
    pub fn do_commit_timelocked_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
        reveal_round: u64,
        commit_reveal_version: u16,
    ) -> DispatchResult {
        Self::internal_commit_timelocked_weights(
            origin,
            netuid,
            MechId::MAIN,
            commit,
            reveal_round,
            commit_reveal_version,
        )
    }

    pub fn do_commit_timelocked_mechanism_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
        reveal_round: u64,
        commit_reveal_version: u16,
    ) -> DispatchResult {
        Self::internal_commit_timelocked_weights(
            origin,
            netuid,
            mecid,
            commit,
            reveal_round,
            commit_reveal_version,
        )
    }

    pub fn internal_commit_timelocked_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        commit: BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
        reveal_round: u64,
        commit_reveal_version: u16,
    ) -> DispatchResult {
        // Ensure netuid and mecid exist
        Self::ensure_mechanism_exists(netuid, mecid)?;

        // Calculate netuid storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // 1. Verify the caller's signature (hotkey).
        let who = ensure_signed(origin)?;

        log::debug!("do_commit_v3_weights(hotkey: {who:?}, netuid: {netuid:?})");

        // 2. Ensure commit-reveal is enabled.
        ensure!(
            Self::get_commit_reveal_weights_enabled(netuid),
            Error::<T>::CommitRevealDisabled
        );

        // 3. Ensure correct client version
        ensure!(
            commit_reveal_version == Self::get_commit_reveal_weights_version(),
            Error::<T>::IncorrectCommitRevealVersion
        );

        // 4. Ensure the hotkey is registered on the network.
        ensure!(
            Self::is_hotkey_registered_on_network(netuid, &who),
            Error::<T>::HotKeyNotRegisteredInSubNet
        );

        // 5. Check that the commit rate does not exceed the allowed frequency.
        let commit_block = Self::get_current_block_as_u64();
        let neuron_uid = Self::get_uid_for_net_and_hotkey(netuid, &who)?;
        ensure!(
            Self::check_rate_limit(netuid_index, neuron_uid, commit_block),
            Error::<T>::CommittingWeightsTooFast
        );

        // 6. Retrieve or initialize the VecDeque of commits for the hotkey.
        let cur_block = Self::get_current_block_as_u64();
        // Key the commit by the epoch it belongs to under the stateful counter.
        let cur_epoch = Self::current_epoch_with_lookahead(netuid);

        TimelockedWeightCommits::<T>::try_mutate(
            netuid_index,
            cur_epoch,
            |commits| -> DispatchResult {
                // 7. Verify that the number of unrevealed commits is within the allowed limit.

                let unrevealed_commits_for_who = commits
                    .iter()
                    .filter(|(account, _, _, _)| account == &who)
                    .count();
                ensure!(
                    unrevealed_commits_for_who < 10,
                    Error::<T>::TooManyUnrevealedCommits
                );

                // 8. Append the new commit with calculated reveal blocks.
                // Hash the commit before it is moved, for the event
                let commit_hash = BlakeTwo256::hash(&commit);
                commits.push_back((who.clone(), cur_block, commit, reveal_round));

                // 9. Emit the WeightsCommitted event
                Self::deposit_event(Event::TimelockedWeightsCommitted(
                    who.clone(),
                    netuid_index,
                    commit_hash,
                    reveal_round,
                ));

                // 10. Update the last commit block for the hotkey's UID.
                Self::set_last_update_for_uid(netuid_index, neuron_uid, commit_block);

                // 11. Return success.
                Ok(())
            },
        )
    }

    /// ---- The implementation for revealing committed weights.
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
    pub fn do_reveal_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        uids: Vec<u16>,
        values: Vec<u16>,
        salt: Vec<u16>,
        version_key: u64,
    ) -> DispatchResult {
        Self::internal_reveal_weights(
            origin,
            netuid,
            MechId::MAIN,
            uids,
            values,
            salt,
            version_key,
        )
    }

    pub fn do_reveal_mechanism_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        uids: Vec<u16>,
        values: Vec<u16>,
        salt: Vec<u16>,
        version_key: u64,
    ) -> DispatchResult {
        Self::internal_reveal_weights(origin, netuid, mecid, uids, values, salt, version_key)
    }

    fn internal_reveal_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        uids: Vec<u16>,
        values: Vec<u16>,
        salt: Vec<u16>,
        version_key: u64,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Calculate netuid storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // --- 1. Check the caller's signature (hotkey).
        let who = ensure_signed(origin.clone())?;

        log::debug!("do_reveal_weights( hotkey:{who:?} netuid:{netuid:?})");

        // --- 2. Ensure commit-reveal is enabled for the network.
        ensure!(
            Self::get_commit_reveal_weights_enabled(netuid),
            Error::<T>::CommitRevealDisabled
        );

        // --- 3. Mutate the WeightCommits to retrieve existing commits for the user.
        WeightCommits::<T>::try_mutate_exists(
            netuid_index,
            &who,
            |maybe_commits| -> DispatchResult {
                let commits = maybe_commits
                    .as_mut()
                    .ok_or(Error::<T>::NoWeightsCommitFound)?;

                // --- 4. Remove any expired commits from the front of the queue, collecting their hashes.
                let mut expired_hashes = Vec::new();
                while let Some((hash, commit_block, _, _)) = commits.front() {
                    if Self::is_commit_expired(netuid, *commit_block) {
                        // Collect the expired commit hash
                        expired_hashes.push(*hash);
                        commits.pop_front();
                    } else {
                        break;
                    }
                }

                // --- 5. Hash the provided data.
                let provided_hash: H256 =
                    Self::get_commit_hash(&who, netuid_index, &uids, &values, &salt, version_key);

                // --- 6. After removing expired commits, check if any commits are left.
                if commits.is_empty() {
                    // Check if provided_hash matches any expired commits
                    if expired_hashes.contains(&provided_hash) {
                        return Err(Error::<T>::ExpiredWeightCommit.into());
                    } else {
                        return Err(Error::<T>::NoWeightsCommitFound.into());
                    }
                }

                // --- 7. Search for the provided_hash in the non-expired commits.
                if let Some(position) = commits
                    .iter()
                    .position(|(hash, _, _, _)| *hash == provided_hash)
                {
                    // --- 8. Get the commit block for the commit being revealed.
                    let (_, commit_block, _, _) = commits
                        .get(position)
                        .ok_or(Error::<T>::NoWeightsCommitFound)?;

                    // --- 9. Ensure the commit is ready to be revealed in the current block range.
                    ensure!(
                        Self::is_reveal_block_range(netuid, *commit_block),
                        Error::<T>::RevealTooEarly
                    );

                    // --- 10. Remove all commits up to and including the one being revealed.
                    for _ in 0..=position {
                        commits.pop_front();
                    }

                    // --- 11. If the queue is now empty, remove the storage entry for the user.
                    if commits.is_empty() {
                        *maybe_commits = None;
                    }

                    // --- 12. Proceed to set the revealed weights.
                    Self::do_set_mechanism_weights(
                        origin,
                        netuid,
                        mecid,
                        uids.clone(),
                        values.clone(),
                        version_key,
                    )?;

                    // --- 13. Emit the WeightsRevealed event.
                    Self::deposit_event(Event::WeightsRevealed(
                        who.clone(),
                        netuid_index,
                        provided_hash,
                    ));

                    // --- 14. Return ok.
                    Ok(())
                } else {
                    // --- 15. The provided_hash does not match any non-expired commits.
                    if expired_hashes.contains(&provided_hash) {
                        Err(Error::<T>::ExpiredWeightCommit.into())
                    } else {
                        Err(Error::<T>::InvalidRevealCommitHashNotMatch.into())
                    }
                }
            },
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
    /// * `InputLengthsUnequal`:
    ///   - The input vectors are of mismatched lengths.
    pub fn do_batch_reveal_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        uids_list: Vec<Vec<u16>>,
        values_list: Vec<Vec<u16>>,
        salts_list: Vec<Vec<u16>>,
        version_keys: Vec<u64>,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        // Calculate netuid storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, MechId::MAIN);

        // --- 1. Check that the input lists are of the same length.
        let num_reveals = uids_list.len();
        ensure!(
            num_reveals == values_list.len()
                && num_reveals == salts_list.len()
                && num_reveals == version_keys.len(),
            Error::<T>::InputLengthsUnequal
        );

        // --- 2. Check the caller's signature (hotkey).
        let who = ensure_signed(origin.clone())?;

        log::debug!("do_batch_reveal_weights( hotkey:{who:?} netuid:{netuid:?})");

        // --- 3. Ensure commit-reveal is enabled for the network.
        ensure!(
            Self::get_commit_reveal_weights_enabled(netuid),
            Error::<T>::CommitRevealDisabled
        );

        // --- 4. Mutate the WeightCommits to retrieve existing commits for the user.
        WeightCommits::<T>::try_mutate_exists(
            netuid_index,
            &who,
            |maybe_commits| -> DispatchResult {
                let commits = maybe_commits
                    .as_mut()
                    .ok_or(Error::<T>::NoWeightsCommitFound)?;

                // --- 5. Remove any expired commits from the front of the queue, collecting their hashes.
                let mut expired_hashes = Vec::new();
                while let Some((hash, commit_block, _, _)) = commits.front() {
                    if Self::is_commit_expired(netuid, *commit_block) {
                        // Collect the expired commit hash
                        expired_hashes.push(*hash);
                        commits.pop_front();
                    } else {
                        break;
                    }
                }

                // --- 6. Prepare to collect all provided hashes and their corresponding reveals.
                let mut provided_hashes = Vec::new();
                let mut reveals = Vec::new();
                let mut revealed_hashes: Vec<H256> = Vec::with_capacity(num_reveals);

                for ((uids, values), (salt, version_key)) in uids_list
                    .into_iter()
                    .zip(values_list)
                    .zip(salts_list.into_iter().zip(version_keys))
                {
                    // --- 6a. Hash the provided data.
                    let provided_hash: H256 = BlakeTwo256::hash_of(&(
                        who.clone(),
                        netuid,
                        uids.clone(),
                        values.clone(),
                        salt.clone(),
                        version_key,
                    ));
                    provided_hashes.push(provided_hash);
                    reveals.push((uids, values, version_key, provided_hash));
                }

                // --- 7. Validate all reveals first to ensure atomicity.
                for (_uids, _values, _version_key, provided_hash) in &reveals {
                    // --- 7a. Check if the provided_hash is in the non-expired commits.
                    if !commits
                        .iter()
                        .any(|(hash, _, _, _)| *hash == *provided_hash)
                    {
                        // --- 7b. If not found, check if it matches any expired commits.
                        if expired_hashes.contains(provided_hash) {
                            return Err(Error::<T>::ExpiredWeightCommit.into());
                        } else {
                            return Err(Error::<T>::InvalidRevealCommitHashNotMatch.into());
                        }
                    }

                    // --- 7c. Find the commit corresponding to the provided_hash.
                    let commit = commits
                        .iter()
                        .find(|(hash, _, _, _)| *hash == *provided_hash)
                        .ok_or(Error::<T>::NoWeightsCommitFound)?;

                    // --- 7d. Check if the commit is within the reveal window.
                    ensure!(
                        Self::is_reveal_block_range(netuid, commit.1),
                        Error::<T>::RevealTooEarly
                    );
                }

                // --- 8. All reveals are valid. Proceed to remove and process each reveal.
                for (uids, values, version_key, provided_hash) in reveals {
                    // --- 8a. Find the position of the provided_hash.
                    if let Some(position) = commits
                        .iter()
                        .position(|(hash, _, _, _)| *hash == provided_hash)
                    {
                        // --- 8b. Remove the commit from the queue.
                        commits.remove(position);

                        // --- 8c. Proceed to set the revealed weights.
                        Self::do_set_weights(origin.clone(), netuid, uids, values, version_key)?;

                        // --- 8d. Collect the revealed hash.
                        revealed_hashes.push(provided_hash);
                    } else if expired_hashes.contains(&provided_hash) {
                        return Err(Error::<T>::ExpiredWeightCommit.into());
                    } else {
                        return Err(Error::<T>::InvalidRevealCommitHashNotMatch.into());
                    }
                }

                // --- 9. If the queue is now empty, remove the storage entry for the user.
                if commits.is_empty() {
                    *maybe_commits = None;
                }

                // --- 10. Emit the WeightsBatchRevealed event with all revealed hashes.
                Self::deposit_event(Event::WeightsBatchRevealed(
                    who.clone(),
                    netuid,
                    revealed_hashes,
                ));

                // --- 11. Return ok.
                Ok(())
            },
        )
    }

    fn internal_set_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        uids: Vec<u16>,
        values: Vec<u16>,
        version_key: u64,
    ) -> dispatch::DispatchResult {
        // Calculate subnet storage index
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // --- 1. Check the caller's signature. This is the hotkey of a registered account.
        let hotkey = ensure_signed(origin)?;
        log::debug!(
            "do_set_weights( origin:{hotkey:?} netuid:{netuid:?}, uids:{uids:?}, values:{values:?})"
        );

        // --- Check that the netuid is not the root network.
        ensure!(!netuid.is_root(), Error::<T>::CanNotSetRootNetworkWeights);

        // --- 2. Check that the length of uid list and value list are equal for this network.
        ensure!(
            Self::uids_match_values(&uids, &values),
            Error::<T>::WeightVecNotEqualSize
        );

        // --- 3. Check to see if this is a valid network and sub-subnet.
        Self::ensure_mechanism_exists(netuid, mecid)?;

        // --- 4. Check to see if the number of uids is within the max allowed uids for this network.
        ensure!(
            Self::check_len_uids_within_allowed(netuid, &uids),
            Error::<T>::UidsLengthExceedUidsInSubNet
        );

        // --- 5. Check to see if the hotkey is registered to the passed network.
        ensure!(
            Self::is_hotkey_registered_on_network(netuid, &hotkey),
            Error::<T>::HotKeyNotRegisteredInSubNet
        );

        // --- 6. Check to see if the hotkey has enough stake to set weights.
        ensure!(
            Self::check_weights_min_stake(&hotkey, netuid),
            Error::<T>::NotEnoughStakeToSetWeights
        );

        // --- 7. Ensure version_key is up-to-date.
        ensure!(
            Self::check_version_key(netuid, version_key),
            Error::<T>::IncorrectWeightVersionKey
        );

        // --- 9. Ensure the uid is not setting weights faster than the weights_set_rate_limit.
        let neuron_uid = Self::get_uid_for_net_and_hotkey(netuid, &hotkey)?;
        let current_block: u64 = Self::get_current_block_as_u64();
        if !Self::get_commit_reveal_weights_enabled(netuid) {
            ensure!(
                // Rate limit should apply per sub-subnet, so use netuid_index here
                Self::check_rate_limit(netuid_index, neuron_uid, current_block),
                Error::<T>::SettingWeightsTooFast
            );
        }
        // --- 10. Check that the neuron uid is an allowed validator permitted to set non-self weights.
        ensure!(
            Self::check_validator_permit(netuid, neuron_uid, &uids, &values),
            Error::<T>::NeuronNoValidatorPermit
        );

        // --- 11. Ensure the passed uids contain no duplicates.
        ensure!(!Self::has_duplicate_uids(&uids), Error::<T>::DuplicateUids);

        // --- 12. Ensure that the passed uids are valid for the network.
        ensure!(
            !Self::contains_invalid_uids(netuid, &uids),
            Error::<T>::UidVecContainInvalidOne
        );

        // --- 13. Ensure that the weights have the required length.
        ensure!(
            Self::check_length(netuid, neuron_uid, &uids, &values),
            Error::<T>::WeightVecLengthIsLow
        );

        // --- 14. Max-upscale the weights.
        let max_upscaled_weights: Vec<u16> = vec_u16_max_upscale_to_u16(&values);

        // --- 15. Ensure the weights are max weight limited
        ensure!(
            Self::max_weight_limited(netuid, neuron_uid, &uids, &max_upscaled_weights),
            Error::<T>::MaxWeightExceeded
        );

        // --- 16. Zip weights for sinking to storage map.
        let mut zipped_weights: Vec<(u16, u16)> = vec![];
        for (uid, val) in uids.iter().zip(max_upscaled_weights.iter()) {
            zipped_weights.push((*uid, *val))
        }

        // --- 17. Set weights under netuid_index (sub-subnet), uid double map entry.
        Weights::<T>::insert(netuid_index, neuron_uid, zipped_weights);

        // --- 18. Set the activity for the weights on this network.
        if !Self::get_commit_reveal_weights_enabled(netuid) {
            Self::set_last_update_for_uid(netuid_index, neuron_uid, current_block);
        }

        // --- 19. Emit the tracking event.
        log::debug!("WeightsSet( netuid:{netuid_index:?}, neuron_uid:{neuron_uid:?} )");
        Self::deposit_event(Event::WeightsSet(netuid_index, neuron_uid));

        // --- 20. Return ok.
        Ok(())
    }

    /// ---- The implementation for the extrinsic set_weights.
    ///
    /// # Args:
    ///  * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///    - The signature of the calling hotkey.
    ///
    ///  * 'netuid' (u16):
    ///    - The u16 network identifier.
    ///
    ///  * 'uids' ( Vec<u16> ):
    ///    - The uids of the weights to be set on the chain.
    ///
    ///  * 'values' ( Vec<u16> ):
    ///    - The values of the weights to set on the chain.
    ///
    ///  * 'version_key' ( u64 ):
    ///    - The network version key.
    ///
    /// # Event:
    ///  * WeightsSet;
    ///    - On successfully setting the weights on chain.
    ///
    /// # Raises:
    ///  * 'MechanismDoesNotExist':
    ///    - Attempting to set weights on a non-existent network.
    ///
    ///  * 'NotRegistered':
    ///    - Attempting to set weights from a non registered account.
    ///
    ///  * 'IncorrectWeightVersionKey':
    ///    - Attempting to set weights without having an up-to-date version_key.
    ///
    ///  * 'SettingWeightsTooFast':
    ///    - Attempting to set weights faster than the weights_set_rate_limit.
    ///
    ///  * 'NeuronNoValidatorPermit':
    ///    - Attempting to set non-self weights without a validator permit.
    ///
    ///  * 'WeightVecNotEqualSize':
    ///    - Attempting to set weights with uids not of same length.
    ///
    ///  * 'DuplicateUids':
    ///    - Attempting to set weights with duplicate uids.
    ///
    /// * 'UidsLengthExceedUidsInSubNet':
    ///    - Attempting to set weights above the max allowed uids.
    ///
    /// * 'UidVecContainInvalidOne':
    ///    - Attempting to set weights with invalid uids.
    ///
    /// * 'WeightVecLengthIsLow':
    ///    - Attempting to set weights with fewer weights than min.
    ///
    /// * 'MaxWeightExceeded':
    ///    - Attempting to set weights with max value exceeding limit.
    ///
    pub fn do_set_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        uids: Vec<u16>,
        values: Vec<u16>,
        version_key: u64,
    ) -> dispatch::DispatchResult {
        Self::internal_set_weights(origin, netuid, MechId::MAIN, uids, values, version_key)
    }

    /// ---- The implementation for the extrinsic set_weights.
    ///
    /// # Args:
    ///  * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///    - The signature of the calling hotkey.
    ///
    ///  * 'netuid' (u16):
    ///    - The u16 network identifier.
    ///
    ///  * 'mecid' (u8):
    ///    - The u8 identifier of sub-subnet.
    ///
    ///  * 'uids' ( Vec<u16> ):
    ///    - The uids of the weights to be set on the chain.
    ///
    ///  * 'values' ( Vec<u16> ):
    ///    - The values of the weights to set on the chain.
    ///
    ///  * 'version_key' ( u64 ):
    ///    - The network version key.
    ///
    /// # Event:
    ///  * WeightsSet;
    ///    - On successfully setting the weights on chain.
    ///
    /// # Raises:
    ///  * 'MechanismDoesNotExist':
    ///    - Attempting to set weights on a non-existent network.
    ///
    ///  * 'NotRegistered':
    ///    - Attempting to set weights from a non registered account.
    ///
    ///  * 'IncorrectWeightVersionKey':
    ///    - Attempting to set weights without having an up-to-date version_key.
    ///
    ///  * 'SettingWeightsTooFast':
    ///    - Attempting to set weights faster than the weights_set_rate_limit.
    ///
    ///  * 'NeuronNoValidatorPermit':
    ///    - Attempting to set non-self weights without a validator permit.
    ///
    ///  * 'WeightVecNotEqualSize':
    ///    - Attempting to set weights with uids not of same length.
    ///
    ///  * 'DuplicateUids':
    ///    - Attempting to set weights with duplicate uids.
    ///
    /// * 'UidsLengthExceedUidsInSubNet':
    ///    - Attempting to set weights above the max allowed uids.
    ///
    /// * 'UidVecContainInvalidOne':
    ///    - Attempting to set weights with invalid uids.
    ///
    /// * 'WeightVecLengthIsLow':
    ///    - Attempting to set weights with fewer weights than min.
    ///
    /// * 'MaxWeightExceeded':
    ///    - Attempting to set weights with max value exceeding limit.
    ///
    pub fn do_set_mechanism_weights(
        origin: OriginFor<T>,
        netuid: NetUid,
        mecid: MechId,
        uids: Vec<u16>,
        values: Vec<u16>,
        version_key: u64,
    ) -> dispatch::DispatchResult {
        Self::internal_set_weights(origin, netuid, mecid, uids, values, version_key)
    }

    /// ---- The implementation for the extrinsic batch_set_weights.
    ///
    /// This call runs a batch of set weights calls, continuing on errors.
    ///
    /// # Args:
    ///  * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///    - The signature of the calling hotkey.
    ///
    ///  * 'netuids' ( Vec<Compact<u16>> ):
    ///    - The u16 network identifiers.
    ///
    ///  * 'weights' ( Vec<Vec<(Compact<u16>, Compact<u16>)>> ):
    ///    - Tuples of (uid, value) of the weights to be set on the chain,
    ///      one Vec for each netuid in the batch.
    ///
    ///  * 'version_keys' ( Vec<Compact<u64>> ):
    ///    - The network version key, one u64 for each netuid in the batch.
    ///
    /// # Event:
    ///  * WeightsSet;
    ///    - On successfully setting the weights on chain.
    ///  * BatchCompletedWithErrors;
    ///    - Emitted when at least on of the weight sets has an error.
    ///  * BatchWeightItemFailed;
    ///    - Emitted for each error within the batch.
    ///  * BatchWeightsCompleted;
    ///    - Emitted when the batch of weights is completed.
    ///  * InputLengthsUnequal;
    ///    - Emitted when the lengths of the input vectors are not equal.
    ///
    pub fn do_batch_set_weights(
        origin: OriginFor<T>,
        netuids: Vec<Compact<NetUid>>,
        weights: Vec<Vec<(Compact<u16>, Compact<u16>)>>,
        version_keys: Vec<Compact<u64>>,
    ) -> dispatch::DispatchResult {
        // --- 1. Check the caller's signature. This is the hotkey of a registered account.
        let hotkey = ensure_signed(origin.clone())?;
        log::debug!(
            "do_batch_set_weights( origin:{hotkey:?} netuids:{netuids:?}, weights:{weights:?}"
        );

        ensure!(
            netuids.len() == weights.len() && netuids.len() == version_keys.len(),
            Error::<T>::InputLengthsUnequal
        );

        let results: Vec<(NetUid, dispatch::DispatchResult)> = netuids
            .iter()
            .zip(weights.iter())
            .zip(version_keys.iter())
            .map(|((&netuid, w), &version_key)| {
                let origin_cloned = origin.clone();
                let netuid: NetUid = netuid.into();

                if Self::get_commit_reveal_weights_enabled(netuid) {
                    return (netuid, Err(Error::<T>::CommitRevealEnabled.into()));
                }

                let uids = w.iter().map(|(u, _)| (*u).into()).collect::<Vec<u16>>();

                let values = w.iter().map(|(_, v)| (*v).into()).collect::<Vec<u16>>();

                (
                    netuid,
                    Self::do_set_weights(origin_cloned, netuid, uids, values, version_key.into()),
                )
            })
            .collect();

        let mut completed_with_errors: bool = false;
        for (netuid, result) in results {
            if let Some(err) = result.err() {
                if !completed_with_errors {
                    Self::deposit_event(Event::BatchCompletedWithErrors());
                    completed_with_errors = true;
                }
                Self::deposit_event(Event::BatchWeightItemFailed(netuid, err));
            }
        }

        // --- 19. Emit the tracking event.
        log::debug!("BatchWeightsSet( netuids:{netuids:?}, hotkey:{hotkey:?} )");
        Self::deposit_event(Event::BatchWeightsCompleted(netuids, hotkey));

        // --- 20. Return ok.
        Ok(())
    }

    // ==========================
    // ==== Helper functions ====
    // ==========================

    /// Returns true if version_key is up-to-date.
    ///
    pub fn check_version_key(netuid: NetUid, version_key: u64) -> bool {
        let network_version_key: u64 = WeightsVersionKey::<T>::get(netuid);
        log::debug!(
            "check_version_key( network_version_key:{network_version_key:?}, version_key:{version_key:?} )"
        );
        network_version_key == 0 || version_key >= network_version_key
    }

    /// Checks if the neuron has set weights within the weights_set_rate_limit.
    ///
    pub fn check_rate_limit(
        netuid_index: NetUidStorageIndex,
        neuron_uid: u16,
        current_block: u64,
    ) -> bool {
        let maybe_netuid_and_subid = Self::get_netuid_and_subid(netuid_index);
        if let Ok((netuid, _)) = maybe_netuid_and_subid
            && Self::is_uid_exist_on_network(netuid, neuron_uid)
        {
            // --- 1. Ensure that the diff between current and last_set weights is greater than limit.
            let last_set_weights: u64 = Self::get_last_update_for_uid(netuid_index, neuron_uid);
            if last_set_weights == 0 {
                return true;
            } // (Storage default) Never set weights.
            return current_block.saturating_sub(last_set_weights)
                >= Self::get_weights_set_rate_limit(netuid);
        }

        // --- 3. Non registered peers cant pass. Neither can non-existing mecid
        false
    }

    /// Checks for any invalid uids on this network.
    pub fn contains_invalid_uids(netuid: NetUid, uids: &[u16]) -> bool {
        for uid in uids {
            if !Self::is_uid_exist_on_network(netuid, *uid) {
                log::debug!(
                    "contains_invalid_uids( netuid:{netuid:?}, uid:{uids:?} does not exist on network. )"
                );
                return true;
            }
        }
        false
    }

    /// Returns true if the passed uids have the same length of the passed values.
    pub fn uids_match_values(uids: &[u16], values: &[u16]) -> bool {
        uids.len() == values.len()
    }

    /// Returns true if the items contain duplicates.
    pub fn has_duplicate_uids(items: &[u16]) -> bool {
        let mut parsed: Vec<u16> = Vec::new();
        for item in items {
            if parsed.contains(item) {
                return true;
            }
            parsed.push(*item);
        }
        false
    }

    /// Returns True if setting self-weight or has validator permit.
    pub fn check_validator_permit(netuid: NetUid, uid: u16, uids: &[u16], weights: &[u16]) -> bool {
        // Check self weight. Allowed to set single value for self weight.
        if Self::is_self_weight(uid, uids, weights) {
            return true;
        }

        // Allow the subnet owner hotkey to act as a validator regardless of permit state.
        if let Some(owner_uid) = Self::get_owner_uid(netuid)
            && owner_uid == uid
        {
            return true;
        }

        // Check if uid has validator permit.
        Self::get_validator_permit_for_uid(netuid, uid)
    }
    /// Returns True if the uids and weights are have a valid length for uid on network.
    pub fn check_length(netuid: NetUid, uid: u16, uids: &[u16], weights: &[u16]) -> bool {
        let subnet_n: usize = Self::get_subnetwork_n(netuid) as usize;
        let min_allowed_length: usize = Self::get_min_allowed_weights(netuid) as usize;
        let min_allowed: usize = {
            if subnet_n < min_allowed_length {
                subnet_n
            } else {
                min_allowed_length
            }
        };

        // Check self weight. Allowed to set single value for self weight.
        // Or check that this is the root netuid.
        if !netuid.is_root() && Self::is_self_weight(uid, uids, weights) {
            return true;
        }
        // Check if number of weights exceeds min.
        if weights.len() >= min_allowed {
            return true;
        }
        // To few weights.
        false
    }

    #[allow(clippy::arithmetic_side_effects)]
    /// Returns normalized the passed positive integer weights so that they sum to u16 max value.
    pub fn normalize_weights(mut weights: Vec<u16>) -> Vec<u16> {
        let sum: u64 = weights.iter().map(|x| *x as u64).sum();
        if sum == 0 {
            return weights;
        }
        weights.iter_mut().for_each(|x| {
            *x = (*x as u64).saturating_mul(u16::MAX as u64).safe_div(sum) as u16;
        });
        weights
    }

    /// Returns False if the weights exceed the configured max weight limit for this network.
    pub fn max_weight_limited(netuid: NetUid, uid: u16, uids: &[u16], weights: &[u16]) -> bool {
        // Allow self weights to exceed max weight limit.
        if Self::is_self_weight(uid, uids, weights) {
            return true;
        }

        // If the max weight limit it u16 max, return true (current constant).
        let max_weight_limit: u16 = Self::get_max_weight_limit(netuid);
        if max_weight_limit == u16::MAX {
            return true;
        }

        // Check if the weights max value is less than or equal to the limit.
        check_vec_max_limited(weights, max_weight_limit)
    }

    /// Returns true if the uids and weights correspond to a self weight on the uid.
    pub fn is_self_weight(uid: u16, uids: &[u16], weights: &[u16]) -> bool {
        if weights.len() != 1 {
            return false;
        }
        let Some(first_uid) = uids.first() else {
            return false;
        };
        if uid != *first_uid {
            return false;
        }
        true
    }

    /// Returns False is the number of uids exceeds the allowed number of uids for this network.
    pub fn check_len_uids_within_allowed(netuid: NetUid, uids: &[u16]) -> bool {
        let subnetwork_n: u16 = Self::get_subnetwork_n(netuid);
        // we should expect at most subnetwork_n uids.
        uids.len() <= subnetwork_n as usize
    }

    /// True when the current epoch is exactly `commit_epoch + reveal_period`.
    ///
    /// `commit_epoch` is the `SubnetEpochIndex` value stored with the commit (CR-v2
    /// `WeightCommits` tuple field 1). The current epoch uses the look-ahead value
    /// so a reveal submitted on a fire-block is judged against the about-to-fire
    /// epoch, consistent with how the commit was tagged.
    pub fn is_reveal_block_range(netuid: NetUid, commit_epoch: u64) -> bool {
        let current_epoch: u64 = Self::current_epoch_with_lookahead(netuid);
        let reveal_period: u64 = Self::get_reveal_period(netuid);

        current_epoch == commit_epoch.saturating_add(reveal_period)
    }

    /// Canonical epoch index for a subnet — the monotonic `SubnetEpochIndex` counter.
    pub fn get_epoch_index(netuid: NetUid, _block_number: u64) -> u64 {
        SubnetEpochIndex::<T>::get(netuid)
    }

    /// Epoch index that a commit or reveal happening at the *current* block
    /// belongs to: the `SubnetEpochIndex` counter, plus one if an epoch slot is
    /// due to fire this block.
    ///
    /// The look-ahead is needed because `block_step` runs in `on_initialize`:
    /// `reveal_crv3_commits` (which must see the about-to-fire epoch) runs before
    /// `run_coinbase` increments the counter, and a commit extrinsic submitted on
    /// a deferred fire-block belongs to the next epoch, not the current one.
    pub fn current_epoch_with_lookahead(netuid: NetUid) -> u64 {
        let block = Self::get_current_block_as_u64();
        let base = SubnetEpochIndex::<T>::get(netuid);
        if Self::should_run_epoch(netuid, block) {
            base.saturating_add(1)
        } else {
            base
        }
    }

    /// True once the current epoch has moved past the commit's reveal epoch
    /// (`commit_epoch + reveal_period`). `commit_epoch` is the stored counter value.
    pub fn is_commit_expired(netuid: NetUid, commit_epoch: u64) -> bool {
        let current_epoch: u64 = Self::current_epoch_with_lookahead(netuid);
        let reveal_period: u64 = Self::get_reveal_period(netuid);

        current_epoch > commit_epoch.saturating_add(reveal_period)
    }

    pub fn set_reveal_period(netuid: NetUid, reveal_period: u64) -> DispatchResult {
        ensure!(
            reveal_period <= MAX_COMMIT_REVEAL_PEROIDS,
            Error::<T>::RevealPeriodTooLarge
        );

        ensure!(
            reveal_period >= MIN_COMMIT_REVEAL_PEROIDS,
            Error::<T>::RevealPeriodTooSmall
        );

        RevealPeriodEpochs::<T>::insert(netuid, reveal_period);

        Self::deposit_event(Event::CommitRevealPeriodsSet(netuid, reveal_period));
        Ok(())
    }
    pub fn get_reveal_period(netuid: NetUid) -> u64 {
        RevealPeriodEpochs::<T>::get(netuid)
    }

    /// Legacy modulo first-block-of-epoch: `epoch * (tempo + 1) - (netuid + 1)`.
    ///
    /// NOT used by live commit-reveal logic — that keys off the stateful
    /// `SubnetEpochIndex` counter. Retained solely so the already-executed,
    /// one-shot `migrate_crv3_commits_add_block` migration stays untouched.
    pub fn get_first_block_of_epoch(netuid: NetUid, epoch: u64) -> u64 {
        let tempo: u64 = Self::get_tempo(netuid) as u64;
        let tempo_plus_one: u64 = tempo.saturating_add(1);
        let netuid_plus_one: u64 = (u16::from(netuid) as u64).saturating_add(1);
        epoch
            .saturating_mul(tempo_plus_one)
            .saturating_sub(netuid_plus_one)
    }

    pub fn get_commit_hash(
        who: &T::AccountId,
        netuid_index: NetUidStorageIndex,
        uids: &[u16],
        values: &[u16],
        salt: &[u16],
        version_key: u64,
    ) -> H256 {
        BlakeTwo256::hash_of(&(who.clone(), netuid_index, uids, values, salt, version_key))
    }

    /// Returns the stored `commit_epoch` (CR-v2 `WeightCommits` tuple field 1) for
    /// the commit with the given hash, if any.
    pub fn find_commit_epoch_via_hash(hash: H256) -> Option<u64> {
        WeightCommits::<T>::iter().find_map(|(_, _, commits)| {
            commits
                .iter()
                .find(|(h, _, _, _)| *h == hash)
                .map(|(_, commit_epoch, _, _)| *commit_epoch)
        })
    }

    pub fn is_batch_reveal_epoch_range(netuid: NetUid, commit_epochs: Vec<u64>) -> bool {
        commit_epochs
            .iter()
            .all(|epoch| Self::is_reveal_block_range(netuid, *epoch))
    }
}
