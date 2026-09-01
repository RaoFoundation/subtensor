use super::*;
use frame_support::PalletId;
use pallet_alpha_assets::AlphaAssetsInterface;
use safe_math::FixedExt;
use sp_core::Get;
use sp_runtime::{SaturatedConversion, traits::AccountIdConversion};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{NetUid, TaoBalance};

/// Data structure for a pending network registration in the execution queue.
#[crate::freeze_struct("c47fe93995c89025")]
#[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct NetworkRegistrationInfo<AccountId> {
    /// The account that registered the network.
    pub coldkey: AccountId,
    /// The account that registered the network.
    pub hotkey: AccountId,
    /// The mechanism that registered the network.
    pub mechid: u16,
    /// The identity that registered the network.
    pub identity: Option<SubnetIdentityOfV3>,
    /// The lock amount that registered the network.
    pub lock_amount: TaoBalance,
    /// The median subnet alpha price that registered the network.
    pub median_subnet_alpha_price: U64F64,
    /// The block at which the network was registered.
    pub registration_block: u64,
    /// The lock id that registered the network.
    pub lock_id: u32,
}

impl<T: Config> Pallet<T> {
    /// Returns true if the subnetwork exists.
    ///
    /// This function checks if a subnetwork with the given UID exists.
    ///
    /// # Returns
    /// * `bool`: Whether the subnet exists.
    ///
    pub fn if_subnet_exist(netuid: NetUid) -> bool {
        NetworksAdded::<T>::get(netuid)
    }

    /// Returns a list of subnet netuid equal to total networks.
    ///
    ///
    /// This iterates through all the networks and returns a list of netuids.
    /// Only keys with `added == true` are returned — leftover `false` entries
    /// (old dissolve paths that wrote the flag instead of removing the key)
    /// must not inflate subnet counts or claim-fee quotes.
    ///
    /// # Returns
    /// * `Vec<NetUid>`: Netuids of all existing subnets.
    ///
    pub fn get_all_subnet_netuids() -> Vec<NetUid> {
        NetworksAdded::<T>::iter()
            .filter(|(_, added)| *added)
            .map(|(netuid, _)| netuid)
            .collect()
    }

    /// Returns the mechanism id for a subnet.
    ///
    ///
    /// This checks the Mechanism map for the value, defaults to 0.
    ///
    /// # Arguments
    /// * `NetUid`: The subnet netuid.
    ///
    /// # Returns
    /// * `u16`: The subnet mechanism
    ///
    pub fn get_subnet_mechanism(netuid: NetUid) -> u16 {
        SubnetMechanism::<T>::get(netuid)
    }

    /// Finds the next available subnet netuid.
    ///
    /// This function iterates through possible subnet netuids starting from 1
    /// until it finds an ID that is not currently in use.
    ///
    /// # Returns
    /// * `NetUid`: The next available subnet netuid.
    pub fn get_next_netuid() -> NetUid {
        let mut next_netuid = NetUid::from(1); // do not allow creation of root
        let netuids = Self::get_all_subnet_netuids();
        let netuids_in_cleanup: Vec<NetUid> = DissolveCleanupQueue::<T>::get()
            .iter()
            .map(|netuid| NetUid::from(netuid.inner()))
            .collect();
        loop {
            if !netuids.contains(&next_netuid) && !netuids_in_cleanup.contains(&next_netuid) {
                break next_netuid;
            }
            next_netuid = next_netuid.next();
        }
    }

    /// Sets the network rate limit and emit the `NetworkRateLimitSet` event
    ///
    pub fn set_network_rate_limit(limit: u64) {
        NetworkRateLimit::<T>::set(limit);
        Self::deposit_event(Event::NetworkRateLimitSet(limit));
    }

    /// Checks if registrations are allowed for a given subnet.
    ///
    /// This function retrieves the subnet hyperparameters for the specified subnet and checks the
    /// `registration_allowed` flag. If the subnet doesn't exist or doesn't have hyperparameters
    /// defined, it returns `false`.
    ///
    /// # Arguments
    ///
    /// * `netuid`: The unique identifier of the subnet.
    ///
    /// # Returns
    ///
    /// * `bool`: `true` if registrations are allowed for the subnet, `false` otherwise.
    pub fn is_registration_allowed(netuid: NetUid) -> bool {
        Self::get_subnet_hyperparams(netuid)
            .map(|params| params.registration_allowed)
            .unwrap_or(false)
    }

    /// Facilitates user registration of a new subnetwork.
    ///
    /// # Arguments
    /// * **`origin`** – `T::RuntimeOrigin` &nbsp;Must be **signed** by the coldkey.
    /// * **`hotkey`** – `&T::AccountId` &nbsp;First neuron of the new subnet.
    /// * **`mechid`** – `u16` &nbsp;Only the dynamic mechanism (`1`) is currently supported.
    /// * **`identity`** – `Option<SubnetIdentityOfV3>` &nbsp;Optional metadata for the subnet.
    ///
    /// # Events
    /// * `NetworkAdded(netuid, mechid)` – always.
    /// * `SubnetIdentitySet(netuid)`   – when a custom identity is supplied.
    /// * `NetworkRemoved(netuid)`      – when a subnet is pruned to make room.
    ///
    /// # Errors
    /// * `NonAssociatedColdKey`            – `hotkey` already belongs to another coldkey.
    /// * `MechanismDoesNotExist`           – unsupported `mechid`.
    /// * `NetworkTxRateLimitExceeded`      – caller hit the register-network rate limit.
    /// * `SubnetLimitReached`              – limit hit **and** no eligible subnet to prune.
    /// * `CannotAffordLockCost`            – caller lacks the lock cost.
    /// * `BalanceWithdrawalError`          – failed to lock balance.
    /// * `InvalidIdentity`                 – supplied `identity` failed validation.
    ///
    pub fn do_register_network(
        origin: OriginFor<T>,
        hotkey: &T::AccountId,
        mechid: u16,
        identity: Option<SubnetIdentityOfV3>,
    ) -> DispatchResult {
        // --- 1. Ensure the caller is a signed user.
        let coldkey = ensure_signed(origin)?;

        // --- 2. Ensure the hotkey does not exist or is owned by the coldkey.
        ensure!(
            !Self::hotkey_account_exists(hotkey) || Self::coldkey_owns_hotkey(&coldkey, hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // Ensure that hotkey is not a special account
        ensure!(
            Self::is_subnet_account_id(hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );

        // --- 3. Ensure the mechanism is Dynamic.
        ensure!(mechid == 1, Error::<T>::MechanismDoesNotExist);

        let current_block = Self::get_current_block_as_u64();

        ensure!(
            current_block >= NetworkRegistrationStartBlock::<T>::get(),
            Error::<T>::SubNetRegistrationDisabled
        );

        // --- 4. Rate limit for network registrations.
        ensure!(
            TransactionType::RegisterNetwork.passes_rate_limit::<T>(&coldkey),
            Error::<T>::NetworkTxRateLimitExceeded
        );

        if let Some(identity_value) = identity.clone() {
            ensure!(
                Self::is_valid_subnet_identity(&identity_value),
                Error::<T>::InvalidIdentity
            );
        }

        // --- 5. Check if we need to prune a subnet (if at SubnetLimit).
        //         But do not prune yet; we only do it after all checks pass.
        let subnet_limit = Self::get_max_subnets();
        let current_count: u16 = NetworksAdded::<T>::iter()
            .filter(|(netuid, added)| *added && *netuid != NetUid::ROOT)
            .count() as u16;

        let cleanup_queue_len = DissolveCleanupQueue::<T>::get().len();
        let registration_queue_len = NetworkRegistrationQueue::<T>::get().len();

        let mut prune_netuid: Option<NetUid> = None;
        let mut wait_to_cleanup = false;

        if current_count.saturating_add(cleanup_queue_len.saturated_into::<u16>()) >= subnet_limit {
            // no netuid available now, but enough netuids in the cleanup queue
            // unnecessary to prune now, we need to wait to cleanup
            if cleanup_queue_len > registration_queue_len {
                wait_to_cleanup = true;
            } else if let Some(netuid) = Self::get_network_to_prune() {
                prune_netuid = Some(netuid);
            } else {
                return Err(Error::<T>::SubnetLimitReached.into());
            }
        }

        // A registration that needs to prune must fail as a registration before it locks funds
        // or mutates either network. During the seed, removing the old subnet would stop its
        // epochs and strand the deferred PendingRootAlphaDivs backlog.
        if prune_netuid.is_some() {
            Self::ensure_beta_basket_seed_idle()?;
        }

        // --- 6. Calculate and lock the required tokens.
        let lock_amount = Self::get_network_lock_cost();
        log::debug!("network lock_amount: {lock_amount:?}");
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, lock_amount.into()),
            Error::<T>::CannotAffordLockCost
        );

        // --- 7. If we reach the limit and need prune a subnet, do it now.
        if let Some(prune_netuid) = prune_netuid {
            Self::do_dissolve_network(prune_netuid)?;
        }

        // can't get a netuid to register, so queue the registration
        if wait_to_cleanup || prune_netuid.is_some() {
            let lock_id = NetworkRegistrationLockId::<T>::get();
            ensure!(lock_id != u32::MAX, Error::<T>::LockIdOverFlow);

            Self::lock_network_registration_cost(&coldkey, lock_amount.into(), lock_id)?;
            NetworkRegistrationLockId::<T>::set(lock_id.saturating_add(1));

            let median_subnet_alpha_price = Self::get_median_subnet_alpha_price();
            let info = NetworkRegistrationInfo::<T::AccountId> {
                coldkey: coldkey.clone(),
                hotkey: hotkey.clone(),
                mechid,
                identity: identity.clone(),
                lock_amount,
                median_subnet_alpha_price,
                registration_block: current_block,
                lock_id,
            };
            NetworkRegistrationQueue::<T>::mutate(|queue| queue.push(info));
            Self::deposit_event(Event::NetworkRegistrationQueued {
                coldkey: coldkey.clone(),
                hotkey: hotkey.clone(),
                mechid,
                identity,
                lock_amount,
                median_subnet_alpha_price,
                registration_block: current_block,
            });
            return Ok(());
        }

        // --- 8. Set the new network state.
        Self::set_new_network_state(
            &coldkey,
            hotkey,
            mechid,
            identity,
            lock_amount,
            Self::get_median_subnet_alpha_price(),
            None,
        )
        .map(|_| ())
        .map_err(|e| e.error)
    }

    pub fn set_new_network_state(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        mechid: u16,
        identity: Option<SubnetIdentityOfV3>,
        lock_amount: TaoBalance,
        median_subnet_alpha_price: U64F64,
        lock_id: Option<u32>,
    ) -> DispatchResultWithPostInfo {
        let db_weight = T::DbWeight::get();
        let mut weight = Weight::from_parts(0, 0);

        // --- 1. Determine netuid to register.
        let current_block = Self::get_current_block_as_u64();
        weight.saturating_accrue(db_weight.reads(1));

        let subnet_limit = Self::get_max_subnets();
        weight.saturating_accrue(db_weight.reads(1));

        let mut networks_added_reads: u64 = 0;
        let mut current_count: u16 = 0;
        for (netuid, added) in NetworksAdded::<T>::iter() {
            networks_added_reads = networks_added_reads.saturating_add(1);
            if added && netuid != NetUid::ROOT {
                current_count = current_count.saturating_add(1);
            }
        }
        weight.saturating_accrue(db_weight.reads(networks_added_reads));

        let cleanup_queue_len: u16 = DissolveCleanupQueue::<T>::get().len() as u16;
        weight.saturating_accrue(db_weight.reads(1));

        let netuid_to_register = if current_count.saturating_add(cleanup_queue_len) >= subnet_limit
        {
            return Err(Error::<T>::SubnetLimitReached.into());
        } else {
            // `get_next_netuid` scans `NetworksAdded` and `DissolveCleanupQueue` again.
            weight.saturating_accrue(db_weight.reads(networks_added_reads.saturating_add(1)));
            Self::get_next_netuid()
        };

        // --- 2. Unlock the registration cost if the fund is locked.
        if let Some(lock_id) = lock_id {
            Self::unlock_network_registration_cost(coldkey, lock_id)?;
        }

        // A netuid identifies a new alpha asset generation after reuse. Clear any
        // legacy counters defensively before creating that generation; normal
        // dissolution cleanup performs the same reset after its last recycle.
        T::AlphaAssets::clear_alpha_counters(netuid_to_register);
        weight.saturating_accrue(db_weight.writes(3));

        let default_tempo = DefaultTempo::<T>::get();
        weight.saturating_accrue(db_weight.reads(1));

        Self::init_new_network(netuid_to_register, default_tempo);
        // SubnetworkN, NetworksAdded, Tempo, TotalNetworks, default hyperparams, and
        // explicit default-value inserts in `init_new_network`.
        weight.saturating_accrue(db_weight.reads(5));
        weight.saturating_accrue(db_weight.writes(15));
        log::debug!("init_new_network: {netuid_to_register:?}");

        let actual_tao_lock_amount =
            Self::transfer_tao_to_subnet(netuid_to_register, coldkey, lock_amount.into())?;
        // `get_subnet_account_id` + coldkey/subnet balance transfer.
        weight.saturating_accrue(db_weight.reads(3));
        weight.saturating_accrue(db_weight.writes(2));
        log::debug!("actual_tao_lock_amount: {actual_tao_lock_amount:?}");

        // --- 3. Set the lock amount for use to determine pricing.
        Self::set_network_last_lock(actual_tao_lock_amount);
        Self::set_network_last_lock_block(current_block);
        weight.saturating_accrue(db_weight.reads(1));
        weight.saturating_accrue(db_weight.writes(2));

        // --- 4. Add the caller to the neuron set.
        let hotkey_is_new = !Self::hotkey_account_exists(hotkey);
        Self::create_account_if_non_existent(coldkey, hotkey)?;
        if hotkey_is_new {
            weight.saturating_accrue(db_weight.reads(4));
            weight.saturating_accrue(db_weight.writes(3));
        } else {
            weight.saturating_accrue(db_weight.reads(2));
        }

        Self::append_neuron(netuid_to_register, hotkey, current_block);
        weight.saturating_accrue(db_weight.reads(10));
        weight.saturating_accrue(db_weight.writes(13));
        log::debug!("Appended neuron for netuid {netuid_to_register:?}, hotkey: {hotkey:?}");

        // --- 5. Set the mechanism.
        SubnetMechanism::<T>::insert(netuid_to_register, mechid);
        weight.saturating_accrue(db_weight.writes(1));
        log::debug!("SubnetMechanism for netuid {netuid_to_register:?} set to: {mechid:?}");

        // --- 6. Set the creation terms.
        NetworkRegisteredAt::<T>::insert(netuid_to_register, current_block);
        RegisteredSubnetCounter::<T>::mutate(netuid_to_register, |c| *c = c.saturating_add(1));
        weight.saturating_accrue(db_weight.reads(1));
        weight.saturating_accrue(db_weight.writes(2));

        // --- 7. Set the symbol.
        let symbol = Self::get_next_available_symbol(netuid_to_register);
        TokenSymbol::<T>::insert(netuid_to_register, symbol);
        // `get_next_available_symbol` scans all assigned symbols once.
        weight.saturating_accrue(db_weight.reads(networks_added_reads.max(1)));
        weight.saturating_accrue(db_weight.writes(1));

        // Keep the locked TAO in the pool instead of recycling the excess.
        // Size the pool alpha reserve from the total TAO reserve at that same price.
        let pool_initial_tao: TaoBalance = Self::get_network_min_lock();
        weight.saturating_accrue(db_weight.reads(1));

        let total_pool_tao: TaoBalance = if actual_tao_lock_amount >= pool_initial_tao {
            actual_tao_lock_amount
        } else {
            pool_initial_tao
        };

        let total_pool_alpha: AlphaBalance = U64F64::saturating_from_num(total_pool_tao.to_u64())
            .safe_div(median_subnet_alpha_price)
            .saturating_floor()
            .saturating_to_num::<u64>()
            .into();

        // // With the full lock retained in the reserve, this will normally be zero.
        let tao_recycled_for_registration = actual_tao_lock_amount.saturating_sub(total_pool_tao);

        // Core pool + ownership
        SubnetTAO::<T>::insert(netuid_to_register, total_pool_tao);
        SubnetAlphaIn::<T>::insert(netuid_to_register, total_pool_alpha);
        SubnetOwner::<T>::insert(netuid_to_register, coldkey.clone());
        Self::set_subnet_owner_hotkey(netuid_to_register, hotkey)?;
        SubnetLocked::<T>::insert(netuid_to_register, actual_tao_lock_amount);
        SubnetAlphaOut::<T>::insert(netuid_to_register, AlphaBalance::ZERO);
        SubnetVolume::<T>::insert(netuid_to_register, 0u128);
        RAORecycledForRegistration::<T>::insert(netuid_to_register, tao_recycled_for_registration);
        weight.saturating_accrue(db_weight.reads(2));
        weight.saturating_accrue(db_weight.writes(8));

        if tao_recycled_for_registration > TaoBalance::ZERO
            && let Some(subnet_account_id) = Self::get_subnet_account_id(netuid_to_register)
        {
            // The subnet account ID is guaranteed to have adequate balance for this
            // recycle because of transfer operation earlier. No need to check this result.
            let _ = Self::recycle_tao(&subnet_account_id, tao_recycled_for_registration);
            weight.saturating_accrue(db_weight.reads(2));
            weight.saturating_accrue(db_weight.writes(1));
        }

        if total_pool_tao > TaoBalance::ZERO {
            // Record in TotalStake the initial TAO in the pool.
            Self::increase_total_stake(total_pool_tao);
            weight.saturating_accrue(db_weight.reads(1));
            weight.saturating_accrue(db_weight.writes(1));
        }

        // --- 8. Add the identity if it exists
        if let Some(identity_value) = identity {
            SubnetIdentitiesV3::<T>::insert(netuid_to_register, identity_value);
            Self::deposit_event(Event::SubnetIdentitySet(netuid_to_register));
            weight.saturating_accrue(db_weight.writes(1));
        }

        // --- 9. Schedule root validators as parents of the subnet owner hotkey.
        weight.saturating_accrue(db_weight.reads(2));
        if let Err(e) = Self::do_set_root_validators_for_subnet(netuid_to_register) {
            log::warn!(
                "Failed to set root validators for netuid {:?}: {:?}",
                netuid_to_register,
                e
            );
        }

        // --- 10. Default emission off
        SubnetEmissionEnabled::<T>::insert(netuid_to_register, false);
        weight.saturating_accrue(db_weight.writes(1));

        // --- 11. Emit the NetworkAdded event.
        log::info!("NetworkAdded( netuid:{netuid_to_register:?}, mechanism:{mechid:?} )");
        Self::deposit_event(Event::NetworkAdded(netuid_to_register, mechid));

        // --- 12. Return success.
        Ok(Some(weight).into())
    }

    /// Sets initial and custom parameters for a new network.
    pub fn init_new_network(netuid: NetUid, tempo: u16) {
        // --- 1. Set network to 0 size.
        SubnetworkN::<T>::insert(netuid, 0);

        // --- 2. Set this network uid to alive.
        NetworksAdded::<T>::insert(netuid, true);

        // --- 3. Fill tempo memory item.
        Tempo::<T>::insert(netuid, tempo);

        // --- 3.1. Initialise `LastEpochBlock` with a per-netuid stagger
        let now = Self::get_current_block_as_u64();
        let period = (tempo as u64).max(1);
        let stagger = (u16::from(netuid) as u64).checked_rem(period).unwrap_or(0);
        LastEpochBlock::<T>::insert(netuid, now.saturating_sub(stagger));

        // --- 4. Increase total network count.
        TotalNetworks::<T>::mutate(|n| *n = n.saturating_add(1));

        // --- 5. Set all default values **explicitly**.
        Self::set_network_registration_allowed(netuid, true);
        Self::set_max_allowed_uids(netuid, 256);
        Self::set_max_allowed_validators(netuid, 64);
        Self::set_min_allowed_weights(netuid, 1);
        Self::set_immunity_period(netuid, 5000);
        Self::set_yuma3_enabled(netuid, true);
        Self::set_burn(netuid, DefaultNeuronBurnCost::<T>::get());

        // New subnets should never inherit a prior subnet owner's disabled state
        // when a netuid is reused after pruning/dissolve.
        SubnetEmissionEnabled::<T>::insert(netuid, true);

        // Make network parameters explicit.
        if !Tempo::<T>::contains_key(netuid) {
            Tempo::<T>::insert(netuid, Tempo::<T>::get(netuid));
        }
        if !Kappa::<T>::contains_key(netuid) {
            Kappa::<T>::insert(netuid, Kappa::<T>::get(netuid));
        }
        if !MaxAllowedUids::<T>::contains_key(netuid) {
            MaxAllowedUids::<T>::insert(netuid, MaxAllowedUids::<T>::get(netuid));
        }
        if !ImmunityPeriod::<T>::contains_key(netuid) {
            ImmunityPeriod::<T>::insert(netuid, ImmunityPeriod::<T>::get(netuid));
        }
        if !ActivityCutoff::<T>::contains_key(netuid) {
            ActivityCutoff::<T>::insert(netuid, ActivityCutoff::<T>::get(netuid));
        }
        if !MinAllowedWeights::<T>::contains_key(netuid) {
            MinAllowedWeights::<T>::insert(netuid, MinAllowedWeights::<T>::get(netuid));
        }
        if !RegistrationsThisInterval::<T>::contains_key(netuid) {
            RegistrationsThisInterval::<T>::insert(
                netuid,
                RegistrationsThisInterval::<T>::get(netuid),
            );
        }
    }

    /// Execute the start call for a subnet.
    ///
    /// This function is used to trigger the start call process for a subnet identified by `netuid`.
    /// It ensures that the subnet exists, the caller is the subnet owner,
    /// and the last emission block number has not been set yet.
    /// It then sets the last emission block number to the current block number.
    ///
    /// # Arguments
    ///
    /// * `origin`: The origin of the call, which is used to ensure the caller is the subnet owner.
    /// * `netuid`: The unique identifier of the subnet for which the start call process is being initiated.
    ///
    /// # Errors
    ///
    /// * `Error::<T>::SubnetNotExists`: If the subnet does not exist.
    /// * `DispatchError::BadOrigin`: If the caller is not the subnet owner.
    /// * `Error::<T>::FirstEmissionBlockNumberAlreadySet`: If the last emission block number has already been set.
    ///
    /// # Returns
    ///
    /// * `DispatchResult`: A result indicating the success or failure of the operation.
    pub fn do_start_call(origin: OriginFor<T>, netuid: NetUid) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subnet_owner(origin, netuid)?;
        ensure!(
            FirstEmissionBlockNumber::<T>::get(netuid).is_none(),
            Error::<T>::FirstEmissionBlockNumberAlreadySet
        );

        let registration_block_number = NetworkRegisteredAt::<T>::get(netuid);
        let current_block_number = Self::get_current_block_as_u64();

        ensure!(
            current_block_number
                >= registration_block_number.saturating_add(StartCallDelay::<T>::get()),
            Error::<T>::StartCallNotReady
        );
        let next_block_number = current_block_number.saturating_add(1);

        FirstEmissionBlockNumber::<T>::insert(netuid, next_block_number);
        SubtokenEnabled::<T>::insert(netuid, true);
        Self::deposit_event(Event::FirstEmissionBlockNumberSet(
            netuid,
            next_block_number,
        ));
        Ok(())
    }

    /// Sets or updates the hotkey account associated with the owner of a specific subnet.
    ///
    /// This function allows either the root origin or the current subnet owner to set or update
    /// the hotkey for a given subnet. The subnet must already exist. To prevent abuse, the call is
    /// rate-limited to once per configured interval (default: one week) per subnet.
    ///
    /// # Arguments
    /// * `origin`: The dispatch origin of the call. Must be either root or the current owner of the subnet.
    /// * `netuid`: The unique identifier of the subnet whose owner hotkey is being set.
    /// * `hotkey`: The new hotkey account to associate with the subnet owner.
    ///
    /// # Returns
    /// * `DispatchResult`: Returns `Ok(())` if the hotkey was successfully set, or an appropriate error otherwise.
    ///
    /// # Errors
    /// * `Error::SubnetNotExists`: If the specified subnet does not exist.
    /// * `Error::TxRateLimitExceeded`: If the function is called more frequently than the allowed rate limit.
    ///
    /// # Access Control
    /// Only callable by:
    /// * Root origin, or
    /// * The coldkey account that owns the subnet.
    ///
    /// # Storage
    /// * Updates [`SubnetOwnerHotkey`] for the given `netuid`.
    /// * Reads and updates [`LastRateLimitedBlock`] for rate-limiting.
    /// * Reads [`DefaultSetSNOwnerHotkeyRateLimit`] to determine the interval between allowed updates.
    ///
    /// # Rate Limiting
    /// This function is rate-limited to one call per subnet per interval (e.g., one week).
    pub fn do_set_sn_owner_hotkey(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: &T::AccountId,
    ) -> DispatchResult {
        // Ensure the caller is either root or subnet owner.
        Self::ensure_subnet_owner_or_root(origin, netuid)?;

        // Ensure that the subnet exists.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Rate limit: 1 call per week
        ensure!(
            TransactionType::SetSNOwnerHotkey.passes_rate_limit_on_subnet::<T>(
                hotkey, // ignored
                netuid, // Specific to a subnet.
            ),
            Error::<T>::TxRateLimitExceeded
        );

        // Set last transaction block
        let current_block = Self::get_current_block_as_u64();
        TransactionType::SetSNOwnerHotkey.set_last_block_on_subnet::<T>(
            hotkey,
            netuid,
            current_block,
        );

        // Insert/update the hotkey
        Self::set_subnet_owner_hotkey(netuid, hotkey)?;

        // Return success.
        Ok(())
    }

    pub fn is_valid_subnet_for_emission(netuid: NetUid) -> bool {
        FirstEmissionBlockNumber::<T>::get(netuid).is_some()
    }

    pub fn get_subnet_account_id(netuid: NetUid) -> Option<T::AccountId> {
        if NetworksAdded::<T>::contains_key(netuid)
            || netuid == NetUid::ROOT
            || DissolveCleanupQueue::<T>::get().contains(&netuid)
        {
            Some(T::SubtensorPalletId::get().into_sub_account_truncating(u16::from(netuid)))
        } else {
            None
        }
    }

    pub fn is_subnet_account_id(account: &T::AccountId) -> Option<NetUid> {
        let pallet_id = T::SubtensorPalletId::get();

        match PalletId::try_from_sub_account::<NetUid>(account) {
            Some((decoded_pallet_id, netuid)) if decoded_pallet_id == pallet_id => Some(netuid),
            _ => None,
        }
    }

    /// Returns whether the owner cut is enabled for the given subnet.
    ///
    /// Returns `true` if the owner cut is enabled for the subnet, otherwise `false`.
    pub fn get_owner_cut_enabled(netuid: NetUid) -> bool {
        OwnerCutEnabled::<T>::get(netuid)
    }

    /// Sets whether the owner cut is enabled for the given subnet.
    ///
    /// # Arguments
    /// * `netuid`: The identifier of the subnet to update.
    /// * `value`: `true` to enable the owner cut for the subnet, `false` to disable it.
    pub fn set_owner_cut_enabled_flag(netuid: NetUid, value: bool) {
        OwnerCutEnabled::<T>::insert(netuid, value);
    }

    /// Returns whether owner cut auto-locking is enabled for the given subnet.
    pub fn get_owner_cut_auto_lock_enabled(netuid: NetUid) -> bool {
        OwnerCutAutoLockEnabled::<T>::get(netuid)
    }

    /// Sets whether owner cut should be auto-locked for the given subnet.
    pub fn set_owner_cut_auto_lock_enabled(netuid: NetUid, value: bool) {
        OwnerCutAutoLockEnabled::<T>::insert(netuid, value);
    }
}
