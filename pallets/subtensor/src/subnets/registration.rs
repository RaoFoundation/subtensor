use super::*;
use sp_core::{H256, U256};
use sp_io::hashing::{keccak_256, sha2_256};
use sp_runtime::Saturating;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{NetUid, Token};
use subtensor_swap_interface::SwapHandler;
use system::pallet_prelude::BlockNumberFor;

const LOG_TARGET: &str = "runtime::subtensor::registration";

impl<T: Config> Pallet<T> {
    pub fn register_neuron(netuid: NetUid, hotkey: &T::AccountId) -> Result<u16, DispatchError> {
        let block_number: u64 = Self::get_current_block_as_u64();
        let current_subnetwork_n: u16 = Self::get_subnetwork_n(netuid);

        if current_subnetwork_n < Self::get_max_allowed_uids(netuid) {
            // No replacement required, the uid appends the subnetwork.
            let neuron_uid = current_subnetwork_n;

            // Expand subnetwork with new account.
            Self::append_neuron(netuid, hotkey, block_number);
            log::debug!("add new neuron account");

            Ok(neuron_uid)
        } else {
            match Self::get_neuron_to_prune(netuid) {
                Some(uid_to_replace) => {
                    Self::replace_neuron(netuid, uid_to_replace, hotkey, block_number);
                    log::debug!("prune neuron");
                    Ok(uid_to_replace)
                }
                None => Err(Error::<T>::NoNeuronIdAvailable.into()),
            }
        }
    }

    pub fn do_register(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: T::AccountId,
    ) -> DispatchResult {
        // 1) coldkey pays
        let coldkey = ensure_signed(origin)?;
        log::debug!("do_register( coldkey:{coldkey:?} netuid:{netuid:?} hotkey:{hotkey:?} )");

        // 2) network validity
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // 3) registrations allowed
        ensure!(
            Self::get_network_registration_allowed(netuid),
            Error::<T>::SubNetRegistrationDisabled
        );

        // 4) hotkey not already registered
        ensure!(
            !Uids::<T>::contains_key(netuid, &hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );

        // 5) compute current burn price.
        // This has already been decayed in `on_initialize` for this block, and
        // successful registrations in the same block bump it immediately.
        let registration_cost: TaoBalance = Self::get_burn(netuid);

        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, registration_cost.into()),
            Error::<T>::NotEnoughBalanceToStake
        );

        // 6) ensure pairing exists and is correct
        Self::create_account_if_non_existent(&coldkey, &hotkey)?;
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // 7) capacity check + prune candidate if full
        ensure!(
            Self::get_max_allowed_uids(netuid) != 0,
            Error::<T>::NoNeuronIdAvailable
        );

        let current_n = Self::get_subnetwork_n(netuid);
        let max_n = Self::get_max_allowed_uids(netuid);
        if current_n >= max_n {
            ensure!(
                Self::get_neuron_to_prune(netuid).is_some(),
                Error::<T>::NoNeuronIdAvailable
            );
        }

        // 8) burn payment (same mechanics as old burned_register)
        let actual_burn_amount =
            Self::transfer_tao_to_subnet(netuid, &coldkey, registration_cost.into())?;

        let burned_alpha = Self::swap_tao_for_alpha(
            netuid,
            actual_burn_amount,
            T::SwapInterface::max_price(),
            false,
        )?
        .amount_paid_out;

        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_sub(burned_alpha.into())
        });

        // 9) register neuron
        let neuron_uid: u16 = Self::register_neuron(netuid, &hotkey)?;

        // 10) immediate burn bump for subsequent registrations in this block
        Self::bump_registration_price_after_registration(netuid);

        // 11) counters
        RegistrationsThisBlock::<T>::mutate(netuid, |val| val.saturating_inc());
        Self::increase_rao_recycled(netuid, registration_cost.into());

        // Record TAO inflow
        Self::record_tao_inflow(netuid, actual_burn_amount);

        // 12) event
        log::debug!("NeuronRegistered( netuid:{netuid:?} uid:{neuron_uid:?} hotkey:{hotkey:?} )");
        Self::deposit_event(Event::NeuronRegistered(netuid, neuron_uid, hotkey));

        Ok(())
    }

    pub fn do_register_limit(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: T::AccountId,
        limit_price: u64,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin.clone())?;
        log::debug!(
            "do_register_limit( netuid:{netuid:?} coldkey:{coldkey:?} limit_price:{limit_price:?} )"
        );

        // Minimal validation before reading/comparing burn.
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Enforce caller limit before entering the shared registration path.
        let registration_cost: TaoBalance = Self::get_burn(netuid);
        let limit_price_tao: TaoBalance = TaoBalance::from(limit_price);

        ensure!(
            registration_cost <= limit_price_tao,
            Error::<T>::RegistrationPriceLimitExceeded
        );

        // Delegate the full shared registration flow.
        Self::do_register(origin, netuid, hotkey)
    }

    pub fn do_faucet(
        origin: OriginFor<T>,
        block_number: u64,
        nonce: u64,
        work: Vec<u8>,
    ) -> DispatchResult {
        // --- 0. Ensure the faucet is enabled.
        // ensure!(AllowFaucet::<T>::get(), Error::<T>::FaucetDisabled);

        // --- 1. Check that the caller has signed the transaction.
        let coldkey = ensure_signed(origin)?;
        log::debug!("do_faucet( coldkey:{coldkey:?} )");

        // --- 2. Ensure the passed block number is valid, not in the future or too old.
        // Work must have been done within 3 blocks (stops long range attacks).
        let current_block_number: u64 = Self::get_current_block_as_u64();
        ensure!(
            block_number <= current_block_number,
            Error::<T>::InvalidWorkBlock
        );
        ensure!(
            current_block_number.saturating_sub(block_number) < 3,
            Error::<T>::InvalidWorkBlock
        );

        // --- 3. Ensure the supplied work passes the difficulty.
        let difficulty: U256 = U256::from(1_000_000); // Base faucet difficulty.
        let work_hash: H256 = Self::vec_to_hash(work.clone());
        ensure!(
            Self::hash_meets_difficulty(&work_hash, difficulty),
            Error::<T>::InvalidDifficulty
        ); // Check that the work meets difficulty.

        // --- 4. Check Work is the product of the nonce, the block number, and hotkey. Add this as used work.
        let seal: H256 = Self::create_seal_hash(block_number, nonce, &coldkey);
        ensure!(seal == work_hash, Error::<T>::InvalidSeal);
        UsedWork::<T>::insert(work.clone(), current_block_number);

        // --- 5. Add Balance via faucet (mint free TAO)
        let balance_to_add: u64 = 1_000_000_000_000;
        let credit = Self::mint_tao(balance_to_add.into());
        let _ = Self::spend_tao(&coldkey, credit, balance_to_add.into());

        // --- 6. Deposit successful event.
        log::debug!("Faucet( coldkey:{coldkey:?} amount:{balance_to_add:?} ) ");
        Self::deposit_event(Event::Faucet(coldkey, balance_to_add));

        // --- 7. Ok and done.
        Ok(())
    }

    pub fn vec_to_hash(vec_hash: Vec<u8>) -> H256 {
        let de_ref_hash = &vec_hash; // b: &Vec<u8>
        let de_de_ref_hash: &[u8] = de_ref_hash; // c: &[u8]
        let real_hash: H256 = H256::from_slice(de_de_ref_hash);
        real_hash
    }

    fn get_immune_owner_hotkeys(netuid: NetUid, coldkey: &T::AccountId) -> Vec<T::AccountId> {
        Self::get_immune_owner_tuples(netuid, coldkey)
            .into_iter()
            .map(|(_, hk)| hk)
            .collect()
    }

    pub fn get_immune_owner_uids(netuid: NetUid, coldkey: &T::AccountId) -> Vec<u16> {
        Self::get_immune_owner_tuples(netuid, coldkey)
            .into_iter()
            .map(|(uid, _)| uid)
            .collect()
    }

    fn get_immune_owner_tuples(netuid: NetUid, coldkey: &T::AccountId) -> Vec<(u16, T::AccountId)> {
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

        // Sort by BlockAtRegistration (ascending), then by uid (ascending)
        // Recent registration is priority so that we can let older keys expire (get non-immune)
        triples.sort_by(|(b1, u1, _), (b2, u2, _)| b1.cmp(b2).then(u1.cmp(u2)));

        // Keep first ImmuneOwnerUidsLimit
        let limit = ImmuneOwnerUidsLimit::<T>::get(netuid).into();
        if triples.len() > limit {
            triples.truncate(limit);
        }

        // Project to uid/hotkey tuple
        let mut immune_tuples: Vec<(u16, T::AccountId)> =
            triples.into_iter().map(|(_, uid, hk)| (uid, hk)).collect();

        // Insert subnet owner hotkey in the beginning of the list if valid and not
        // already present
        if let Ok(owner_hk) = SubnetOwnerHotkey::<T>::try_get(netuid)
            && let Some(owner_uid) = Uids::<T>::get(netuid, &owner_hk)
            && !immune_tuples.contains(&(owner_uid, owner_hk.clone()))
        {
            immune_tuples.insert(0, (owner_uid, owner_hk.clone()));
            if immune_tuples.len() > limit {
                immune_tuples.truncate(limit);
            }
        }

        immune_tuples
    }

    /// Determine which neuron to prune.
    pub fn get_neuron_to_prune(netuid: NetUid) -> Option<u16> {
        let n = Self::get_subnetwork_n(netuid);
        if n == 0 {
            return None;
        }

        let owner_ck = SubnetOwner::<T>::get(netuid);
        let immortal_hotkeys = Self::get_immune_owner_hotkeys(netuid, &owner_ck);
        let emissions: Vec<AlphaBalance> = Emission::<T>::get(netuid);

        // Single pass:
        // - count current non‑immortal & non‑immune UIDs,
        // - track best non‑immune and best immune candidates separately.
        let mut free_count: u16 = 0;

        // (emission, reg_block, uid)
        let mut best_non_immune: Option<(AlphaBalance, u64, u16)> = None;
        let mut best_immune: Option<(AlphaBalance, u64, u16)> = None;

        for uid in 0..n {
            let hk = match Self::get_hotkey_for_net_and_uid(netuid, uid) {
                Ok(h) => h,
                Err(_) => continue,
            };

            // Skip owner‑immortal hotkeys entirely.
            if immortal_hotkeys.contains(&hk) {
                continue;
            }

            let is_immune = Self::get_neuron_is_immune(netuid, uid);
            let emission = emissions
                .get(uid as usize)
                .cloned()
                .unwrap_or(AlphaBalance::ZERO);
            let reg_block = Self::get_neuron_block_at_registration(netuid, uid);

            // Helper to decide if (e, b, u) beats the current best.
            let consider = |best: &mut Option<(AlphaBalance, u64, u16)>| match best {
                None => *best = Some((emission, reg_block, uid)),
                Some((be, bb, bu)) => {
                    let better = if emission != *be {
                        emission < *be
                    } else if reg_block != *bb {
                        reg_block < *bb
                    } else {
                        uid < *bu
                    };
                    if better {
                        *best = Some((emission, reg_block, uid));
                    }
                }
            };

            if is_immune {
                consider(&mut best_immune);
            } else {
                free_count = free_count.saturating_add(1);
                consider(&mut best_non_immune);
            }
        }

        // No candidates left after filtering out owner‑immortal hotkeys.
        if best_non_immune.is_none() && best_immune.is_none() {
            return None;
        }

        // Safety floor for non‑immortal & non‑immune UIDs.
        let min_free: u16 = Self::get_min_non_immune_uids(netuid);
        let can_prune_non_immune = free_count > min_free;

        // Prefer non‑immune if allowed; otherwise fall back to immune.
        if can_prune_non_immune && let Some((_, _, uid)) = best_non_immune {
            return Some(uid);
        }
        best_immune.map(|(_, _, uid)| uid)
    }

    /// Determine whether the given hash satisfies the given difficulty.
    /// The test is done by multiplying the two together. If the product
    /// overflows the bounds of U256, then the product (and thus the hash)
    /// was too high.
    pub fn hash_meets_difficulty(hash: &H256, difficulty: U256) -> bool {
        let bytes: &[u8] = hash.as_bytes();
        let num_hash: U256 = U256::from_little_endian(bytes);
        let (value, overflowed) = num_hash.overflowing_mul(difficulty);

        log::trace!(
            target: LOG_TARGET,
            "Difficulty: hash: {hash:?}, hash_bytes: {bytes:?}, hash_as_num: {num_hash:?}, difficulty: {difficulty:?}, value: {value:?} overflowed: {overflowed:?}"
        );
        !overflowed
    }

    pub fn get_block_hash_from_u64(block_number: u64) -> H256 {
        let block_number: BlockNumberFor<T> = TryInto::<BlockNumberFor<T>>::try_into(block_number)
            .ok()
            .expect("convert u64 to block number.");
        let block_hash_at_number: <T as frame_system::Config>::Hash =
            system::Pallet::<T>::block_hash(block_number);
        let vec_hash: Vec<u8> = block_hash_at_number.as_ref().to_vec();
        let deref_vec_hash: &[u8] = &vec_hash; // c: &[u8]
        let real_hash: H256 = H256::from_slice(deref_vec_hash);

        log::trace!(
            target: LOG_TARGET,
            "block_number: {block_number:?}, vec_hash: {vec_hash:?}, real_hash: {real_hash:?}"
        );

        real_hash
    }

    pub fn hash_to_vec(hash: H256) -> Vec<u8> {
        let hash_as_bytes: &[u8] = hash.as_bytes();
        let hash_as_vec: Vec<u8> = hash_as_bytes.to_vec();
        hash_as_vec
    }

    pub fn hash_block_and_hotkey(block_hash_bytes: &[u8; 32], hotkey: &T::AccountId) -> H256 {
        let binding = hotkey.encode();
        // Safe because Substrate guarantees that all AccountId types are at least 32 bytes
        let (hotkey_bytes, _) = binding.split_at(32);
        let mut full_bytes = [0u8; 64];
        let (first_half, second_half) = full_bytes.split_at_mut(32);
        first_half.copy_from_slice(block_hash_bytes);
        second_half.copy_from_slice(hotkey_bytes);
        let keccak_256_seal_hash_vec: [u8; 32] = keccak_256(&full_bytes[..]);

        H256::from_slice(&keccak_256_seal_hash_vec)
    }

    pub fn hash_hotkey_to_u64(hotkey: &T::AccountId) -> u64 {
        let binding = hotkey.encode();
        let (hotkey_bytes, _) = binding.split_at(32);
        let mut full_bytes = [0u8; 64];
        // Copy the hotkey_bytes into the first half of full_bytes
        full_bytes[..32].copy_from_slice(hotkey_bytes);
        let keccak_256_seal_hash_vec: [u8; 32] = keccak_256(&full_bytes[..]);
        let hash_u64: u64 = u64::from_le_bytes(
            keccak_256_seal_hash_vec[0..8]
                .try_into()
                .unwrap_or_default(),
        );
        hash_u64
    }

    pub fn create_seal_hash(block_number_u64: u64, nonce_u64: u64, hotkey: &T::AccountId) -> H256 {
        let nonce = nonce_u64.to_le_bytes();
        let block_hash_at_number: H256 = Self::get_block_hash_from_u64(block_number_u64);
        let block_hash_bytes: &[u8; 32] = block_hash_at_number.as_fixed_bytes();
        let binding = Self::hash_block_and_hotkey(block_hash_bytes, hotkey);
        let block_and_hotkey_hash_bytes: &[u8; 32] = binding.as_fixed_bytes();

        let mut full_bytes = [0u8; 40];
        let (first_chunk, second_chunk) = full_bytes.split_at_mut(8);
        first_chunk.copy_from_slice(&nonce);
        second_chunk.copy_from_slice(block_and_hotkey_hash_bytes);
        let sha256_seal_hash_vec: [u8; 32] = sha2_256(&full_bytes[..]);
        let keccak_256_seal_hash_vec: [u8; 32] = keccak_256(&sha256_seal_hash_vec);
        let seal_hash: H256 = H256::from_slice(&keccak_256_seal_hash_vec);

        log::trace!(
            "\n hotkey:{hotkey:?} \nblock_number: {block_number_u64:?}, \nnonce_u64: {nonce_u64:?}, \nblock_hash: {block_hash_at_number:?}, \nfull_bytes: {full_bytes:?}, \nsha256_seal_hash_vec: {sha256_seal_hash_vec:?},  \nkeccak_256_seal_hash_vec: {keccak_256_seal_hash_vec:?}, \nseal_hash: {seal_hash:?}"
        );

        seal_hash
    }

    /// Helper function for creating nonce and work.
    pub fn create_work_for_block_number(
        netuid: NetUid,
        block_number: u64,
        start_nonce: u64,
        hotkey: &T::AccountId,
    ) -> (u64, Vec<u8>) {
        let difficulty: U256 = Self::get_difficulty(netuid);
        let mut nonce: u64 = start_nonce;
        let mut work: H256 = Self::create_seal_hash(block_number, nonce, hotkey);
        while !Self::hash_meets_difficulty(&work, difficulty) {
            nonce.saturating_inc();
            work = Self::create_seal_hash(block_number, nonce, hotkey);
        }
        let vec_work: Vec<u8> = Self::hash_to_vec(work);
        (nonce, vec_work)
    }

    /// Updates neuron burn price.
    ///
    /// Behavior:
    /// - Each non-genesis block: burn decays continuously by a per-block factor `f`,
    ///   where `f ^ BurnHalfLife = 1/2`.
    /// - Burn is clamped to the configured [`MinBurn`, `MaxBurn`] range.
    ///
    pub fn update_registration_prices_for_networks() {
        let current_block: u64 = Self::get_current_block_as_u64();

        for (netuid, _) in NetworksAdded::<T>::iter() {
            // --- 1) Apply continuous per-block decay.
            let burn_u64: u64 = Self::get_burn(netuid).into();
            let min_burn_u64: u64 = Self::get_min_burn(netuid).into();
            let max_burn_u64: u64 = Self::get_max_burn(netuid).into();
            let half_life: u16 = BurnHalfLife::<T>::get(netuid);

            let mut new_burn_u64: u64 = burn_u64;

            if half_life > 0 {
                // Since this function runs every block in `on_initialize`,
                // applying the per-block factor once here gives continuous
                // exponential decay.
                if current_block > 1 {
                    let factor_q32: u64 = Self::decay_factor_q32(half_life);
                    new_burn_u64 = Self::mul_by_q32(burn_u64, factor_q32);
                }
            }

            // Enforce configured burn bounds.
            if new_burn_u64 < min_burn_u64 {
                new_burn_u64 = min_burn_u64;
            }
            if new_burn_u64 > max_burn_u64 {
                new_burn_u64 = max_burn_u64;
            }

            if new_burn_u64 != burn_u64 {
                Self::set_burn(netuid, TaoBalance::from(new_burn_u64));
            }

            // --- 2) Reset per-block registrations counter for the new block.
            Self::set_registrations_this_block(netuid, 0);

            // --- 3) Root keeps interval-based admission, so reset that counter on the root epoch boundary.
            if netuid.is_root() && Self::should_run_epoch(netuid, current_block) {
                Self::set_registrations_this_interval(netuid, 0);
            }
        }
    }

    pub fn bump_registration_price_after_registration(netuid: NetUid) {
        // Root does not use the per-registration burn bump path.
        if netuid.is_root() {
            return;
        }

        let mult: U64F64 = BurnIncreaseMult::<T>::get(netuid).max(U64F64::saturating_from_num(1));
        let burn_u64: u64 = Self::get_burn(netuid).into();
        let min_burn_u64: u64 = Self::get_min_burn(netuid).into();
        let max_burn_u64: u64 = Self::get_max_burn(netuid).into();

        let mut new_burn_u64: u64 = U64F64::saturating_from_num(burn_u64)
            .saturating_mul(mult)
            .saturating_to_num::<u64>();

        // Enforce configured burn bounds.
        if new_burn_u64 < min_burn_u64 {
            new_burn_u64 = min_burn_u64;
        }
        if new_burn_u64 > max_burn_u64 {
            new_burn_u64 = max_burn_u64;
        }

        Self::set_burn(netuid, TaoBalance::from(new_burn_u64));
    }
}
