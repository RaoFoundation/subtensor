use frame_support::pallet_macros::pallet_section;

/// `pallet_section` defining [`Event`] for the subtensor pallet (`SubtensorModule` in runtime metadata).
///
/// Imported into the pallet via [`import_section`]. Variant **names and order are frozen** for
/// metadata / client compatibility — docs only may change.
#[pallet_section]
mod events {
    use codec::Compact;

    /// On-chain events emitted by `SubtensorModule`.
    ///
    /// Prefer searching variant names (e.g. `StakeAdded`, `WeightsSet`) from deposit sites or
    /// explorers; field order for tuple variants is documented on each variant.
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A subnet was registered and added to the active set.
        ///
        /// Fields: `(netuid, mechid)`.
        NetworkAdded(NetUid, u16),
        /// A subnet was dissolved / removed from the active set.
        ///
        /// Fields: `(netuid)`.
        NetworkRemoved(NetUid),
        /// Stake was added: TAO from a coldkey was swapped into alpha and credited to a hotkey on a subnet.
        ///
        /// Fields: `(coldkey, hotkey, tao, alpha, netuid, fee)`.
        /// `tao` / `alpha` are the amounts involved in the stake add; `fee` is the swap fee paid (rao).
        StakeAdded(
            T::AccountId,
            T::AccountId,
            TaoBalance,
            AlphaBalance,
            NetUid,
            u64,
        ),
        /// Stake was removed: alpha on a hotkey/subnet was swapped back to TAO and paid to the coldkey.
        ///
        /// Fields: `(coldkey, hotkey, tao, alpha, netuid, fee)`.
        StakeRemoved(
            T::AccountId,
            T::AccountId,
            TaoBalance,
            AlphaBalance,
            NetUid,
            u64,
        ),
        /// Stake was moved between hotkeys and/or subnets for the same coldkey (TAO-equivalent amount).
        ///
        /// Fields: `(coldkey, origin_hotkey, origin_netuid, destination_hotkey, destination_netuid, tao)`.
        StakeMoved(
            T::AccountId,
            T::AccountId,
            NetUid,
            T::AccountId,
            NetUid,
            TaoBalance,
        ),
        /// A neuron successfully set weights on a subnet (or mechanism index).
        ///
        /// Fields: `(netuid_index, uid)`.
        WeightsSet(NetUidStorageIndex, u16),
        /// A hotkey was registered as a neuron on a subnet and assigned a uid.
        ///
        /// Fields: `(netuid, uid, hotkey)`.
        NeuronRegistered(NetUid, u16, T::AccountId),
        /// Multiple neurons were registered in one bulk operation.
        ///
        /// Fields: `(u16, u16)` — historically subnet / count style args; no active deposit site today.
        BulkNeuronsRegistered(u16, u16),
        // FIXME: Not used yet.
        /// Placeholder for a bulk-balance-set path; currently unused.
        ///
        /// Fields: `(u16, u16)`.
        BulkBalancesSet(u16, u16),
        /// Max allowed uids (`MaxAllowedUids`) was set for a subnet.
        ///
        /// Fields: `(netuid, max_allowed_uids)`.
        MaxAllowedUidsSet(NetUid, u16),
        /// Max weight limit was set for a subnet (deprecated: limit is now constant; event unused).
        ///
        /// Fields: `(netuid, max_weight_limit)`.
        #[deprecated(note = "Max weight limit is now a constant and this event is unused")]
        MaxWeightLimitSet(NetUid, u16),
        /// PoW registration difficulty was set for a subnet.
        ///
        /// Fields: `(netuid, difficulty)`.
        DifficultySet(NetUid, u64),
        /// Difficulty adjustment interval (blocks) was set for a subnet.
        ///
        /// Fields: `(netuid, adjustment_interval)`.
        AdjustmentIntervalSet(NetUid, u16),
        /// Target registrations per adjustment interval was set for a subnet.
        ///
        /// Fields: `(netuid, target_registrations)`.
        RegistrationPerIntervalSet(NetUid, u16),
        /// Max neuron registrations allowed per block was set for a subnet.
        ///
        /// Fields: `(netuid, max_registrations_per_block)`.
        MaxRegistrationsPerBlockSet(NetUid, u16),
        /// Activity cutoff (blocks without update before a neuron is inactive) was set for a subnet.
        ///
        /// Fields: `(netuid, activity_cutoff)`.
        ActivityCutoffSet(NetUid, u16),
        /// Consensus hyperparameter Rho was set for a subnet.
        ///
        /// Fields: `(netuid, rho)`.
        RhoSet(NetUid, u16),
        /// Steepness of the sigmoid used when computing alpha values was set for a subnet.
        ///
        /// Fields: `(netuid, steepness)`.
        AlphaSigmoidSteepnessSet(NetUid, i16),
        /// Consensus hyperparameter Kappa was set for a subnet.
        ///
        /// Fields: `(netuid, kappa)`.
        KappaSet(NetUid, u16),
        /// Minimum allowed weight value was set for a subnet.
        ///
        /// Fields: `(netuid, min_allowed_weight)`.
        MinAllowedWeightSet(NetUid, u16),
        /// Validator pruning length was set for a subnet.
        ///
        /// Fields: `(netuid, validator_prune_len)`.
        ValidatorPruneLenSet(NetUid, u64),
        /// Scaling-law power hyperparameter was set for a subnet.
        ///
        /// Fields: `(netuid, scaling_law_power)`.
        ScalingLawPowerSet(NetUid, u16),
        /// Rate limit (blocks) between weight-set extrinsics was set for a subnet.
        ///
        /// Fields: `(netuid, rate_limit)`.
        WeightsSetRateLimitSet(NetUid, u64),
        /// Immunity period (blocks) for newly registered neurons was set for a subnet.
        ///
        /// Fields: `(netuid, immunity_period)`.
        ImmunityPeriodSet(NetUid, u16),
        /// Bonds moving-average hyperparameter was set for a subnet.
        ///
        /// Fields: `(netuid, bonds_moving_average)`.
        BondsMovingAverageSet(NetUid, u64),
        /// Bonds penalty hyperparameter was set for a subnet.
        ///
        /// Fields: `(netuid, bonds_penalty)`.
        BondsPenaltySet(NetUid, u16),
        /// Whether bonds reset on weight set was configured for a subnet.
        ///
        /// Fields: `(netuid, bonds_reset_on)`.
        BondsResetOnSet(NetUid, bool),
        /// Max allowed validators on a subnet was set.
        ///
        /// Fields: `(netuid, max_allowed_validators)`.
        MaxAllowedValidatorsSet(NetUid, u16),
        /// Axon (serve) endpoint metadata was published for a hotkey on a subnet.
        ///
        /// Fields: `(netuid, hotkey)`.
        AxonServed(NetUid, T::AccountId),
        /// Prometheus endpoint metadata was published for a hotkey on a subnet.
        ///
        /// Fields: `(netuid, hotkey)`.
        PrometheusServed(NetUid, T::AccountId),
        /// A hotkey became a delegate (nominatable) with the given take.
        ///
        /// Fields: `(coldkey, hotkey, take)`.
        DelegateAdded(T::AccountId, T::AccountId, PerU16),
        /// Global default delegate take was set.
        ///
        /// Fields: `(default_take)`.
        DefaultTakeSet(PerU16),
        /// Weights version key required by a subnet was set.
        ///
        /// Fields: `(netuid, weights_version_key)`.
        WeightsVersionKeySet(NetUid, u64),
        /// Minimum PoW difficulty for a subnet was set.
        ///
        /// Fields: `(netuid, min_difficulty)`.
        MinDifficultySet(NetUid, u64),
        /// Maximum PoW difficulty for a subnet was set.
        ///
        /// Fields: `(netuid, max_difficulty)`.
        MaxDifficultySet(NetUid, u64),
        /// Rate limit for axon/prometheus serve updates was set for a subnet.
        ///
        /// Fields: `(netuid, serving_rate_limit)`.
        ServingRateLimitSet(NetUid, u64),
        /// Current registration burn (TAO) was set for a subnet.
        ///
        /// Fields: `(netuid, burn)`.
        BurnSet(NetUid, TaoBalance),
        /// Maximum registration burn (TAO) was set for a subnet.
        ///
        /// Fields: `(netuid, max_burn)`.
        MaxBurnSet(NetUid, TaoBalance),
        /// Minimum registration burn (TAO) was set for a subnet.
        ///
        /// Fields: `(netuid, min_burn)`.
        MinBurnSet(NetUid, TaoBalance),
        /// Per-block cap on how many subnet epochs may run (dynamic tempo throttle) was set.
        ///
        /// Fields: `(max_epochs_per_block)`.
        MaxEpochsPerBlockSet(u8),
        /// Global transaction rate limit was set.
        ///
        /// Fields: `(tx_rate_limit)`.
        TxRateLimitSet(u64),
        /// Rate limit for delegate-take changes was set.
        ///
        /// Fields: `(tx_delegate_take_rate_limit)`.
        TxDelegateTakeRateLimitSet(u64),
        /// Rate limit for childkey-take changes was set.
        ///
        /// Fields: `(tx_childkey_take_rate_limit)`.
        TxChildKeyTakeRateLimitSet(u64),
        /// Admin freeze window length (last N blocks of a tempo where owner admin calls are frozen) was set.
        ///
        /// Fields: `(admin_freeze_window)`.
        AdminFreezeWindowSet(u16),
        /// Owner hyperparameter rate limit, measured in epochs, was set.
        ///
        /// Fields: `(owner_hyperparam_rate_limit_epochs)`.
        OwnerHyperparamRateLimitSet(u16),
        /// Global minimum childkey take was set.
        ///
        /// Fields: `(min_childkey_take)`.
        MinChildKeyTakeSet(PerU16),
        /// Per-subnet minimum childkey take was set.
        ///
        /// Fields: `(netuid, min_childkey_take)`.
        MinChildKeyTakePerSubnetSet(NetUid, PerU16),
        /// Global maximum childkey take was set.
        ///
        /// Fields: `(max_childkey_take)`.
        MaxChildKeyTakeSet(PerU16),
        /// Childkey take for a specific hotkey was set.
        ///
        /// Fields: `(hotkey, childkey_take)`.
        ChildKeyTakeSet(T::AccountId, PerU16),
        /// A privileged sudo call finished with the given dispatch result.
        ///
        /// Fields: `(result)`.
        Sudid(DispatchResult),
        /// Whether normal (non-PoW) registration is allowed was toggled for a subnet.
        ///
        /// Fields: `(netuid, registration_allowed)`.
        RegistrationAllowed(NetUid, bool),
        /// Whether PoW registration is allowed was toggled for a subnet.
        ///
        /// Fields: `(netuid, pow_registration_allowed)`.
        PowRegistrationAllowed(NetUid, bool),
        /// Subnet tempo (blocks per epoch) was set.
        ///
        /// Fields: `(netuid, tempo)`.
        TempoSet(NetUid, u16),
        /// RAO recycled into the subnet pool on registration was set.
        ///
        /// Fields: `(netuid, rao_recycled)`.
        RAORecycledForRegistrationSet(NetUid, TaoBalance),
        /// Minimum stake threshold required for validators to set weights was set.
        ///
        /// Fields: `(stake_threshold)`.
        StakeThresholdSet(u64),
        /// Difficulty adjustment alpha was set for a subnet.
        ///
        /// Fields: `(netuid, adjustment_alpha)`.
        AdjustmentAlphaSet(NetUid, u64),
        /// Testnet faucet credited free balance to an account.
        ///
        /// Fields: `(coldkey, balance_added)`.
        Faucet(T::AccountId, u64),
        /// Global subnet-owner cut of emissions was set.
        ///
        /// Fields: `(subnet_owner_cut)`.
        SubnetOwnerCutSet(u16),
        /// Minimum blocks between network registrations was set.
        ///
        /// Fields: `(network_rate_limit)`.
        NetworkRateLimitSet(u64),
        /// Network immunity period (blocks a new subnet is immune from deregistration) was set.
        ///
        /// Fields: `(network_immunity_period)`.
        NetworkImmunityPeriodSet(u64),
        /// Delay before `start_call` may enable emissions on a new subnet was set.
        ///
        /// Fields: `(start_call_delay)`.
        StartCallDelaySet(u64),
        /// Minimum TAO lock cost to register a new subnet was set.
        ///
        /// Fields: `(network_min_lock_cost)`.
        NetworkMinLockCostSet(TaoBalance),
        /// Maximum number of subnets was set.
        ///
        /// Fields: `(subnet_limit)`.
        SubnetLimitSet(u16),
        /// Interval over which network lock cost decays was set.
        ///
        /// Fields: `(lock_cost_reduction_interval)`.
        NetworkLockCostReductionIntervalSet(u64),
        /// A delegate decreased its take.
        ///
        /// Fields: `(coldkey, hotkey, take)`.
        TakeDecreased(T::AccountId, T::AccountId, PerU16),
        /// A delegate increased its take.
        ///
        /// Fields: `(coldkey, hotkey, take)`.
        TakeIncreased(T::AccountId, T::AccountId, PerU16),
        /// A coldkey swapped its associated hotkey globally.
        HotkeySwapped {
            /// Coldkey that owns the hotkey association.
            coldkey: T::AccountId,
            /// Hotkey being replaced.
            old_hotkey: T::AccountId,
            /// Hotkey that replaces `old_hotkey`.
            new_hotkey: T::AccountId,
        },
        /// Maximum delegate take was set via sudo/admin.
        ///
        /// Fields: `(max_delegate_take)`.
        MaxDelegateTakeSet(PerU16),
        /// Minimum delegate take was set via sudo/admin.
        ///
        /// Fields: `(min_delegate_take)`.
        MinDelegateTakeSet(PerU16),
        /// A coldkey announced an intent to swap to a new coldkey (commitment by hash).
        ColdkeySwapAnnounced {
            /// Coldkey that made the announcement.
            who: T::AccountId,
            /// Hash commitment of the new coldkey.
            new_coldkey_hash: T::Hash,
        },
        /// A pending coldkey swap announcement was reset for an account.
        ColdkeySwapReset {
            /// Coldkey whose swap announcement was cleared/reset.
            who: T::AccountId,
        },
        /// A coldkey swap completed; ownership moved from `old_coldkey` to `new_coldkey`.
        ColdkeySwapped {
            /// Previous coldkey.
            old_coldkey: T::AccountId,
            /// New coldkey that now owns the accounts/stake.
            new_coldkey: T::AccountId,
        },
        /// A coldkey swap was disputed during the arbitration window.
        ColdkeySwapDisputed {
            /// Coldkey whose swap was disputed.
            coldkey: T::AccountId,
        },
        /// All balance of a hotkey was unstaked and transferred to a new coldkey during a swap path.
        AllBalanceUnstakedAndTransferredToNewColdkey {
            /// Coldkey that previously owned the funds.
            current_coldkey: T::AccountId,
            /// Coldkey that received the unstaked balance.
            new_coldkey: T::AccountId,
            /// Total free balance transferred.
            total_balance: <<T as Config>::Currency as fungible::Inspect<
                <T as frame_system::Config>::AccountId,
            >>::Balance,
        },
        /// The arbitration period for a coldkey swap was extended.
        ArbitrationPeriodExtended {
            /// Coldkey whose arbitration window was extended.
            coldkey: T::AccountId,
        },
        /// Setting children of a parent hotkey was scheduled (cooldown before it takes effect).
        ///
        /// Fields: `(hotkey, netuid, cooldown_block, children)` where each child is `(proportion, child_hotkey)`.
        SetChildrenScheduled(T::AccountId, NetUid, u64, Vec<(u64, T::AccountId)>),
        /// Children of a parent hotkey were applied on a subnet.
        ///
        /// Fields: `(hotkey, netuid, children)` where each child is `(proportion, child_hotkey)`.
        SetChildren(T::AccountId, NetUid, Vec<(u64, T::AccountId)>),
        // /// The hotkey emission tempo has been set
        // HotkeyEmissionTempoSet(u64),
        // /// The network maximum stake has been set
        // NetworkMaxStakeSet(u16, u64),
        /// On-chain identity for a coldkey was set or updated.
        ///
        /// Fields: `(coldkey)`.
        ChainIdentitySet(T::AccountId),
        /// On-chain identity metadata for a subnet was set or updated.
        ///
        /// Fields: `(netuid)`.
        SubnetIdentitySet(NetUid),
        /// On-chain identity metadata for a subnet was removed.
        ///
        /// Fields: `(netuid)`.
        SubnetIdentityRemoved(NetUid),
        /// Dissolving a subnet was scheduled for a future block.
        DissolveNetworkScheduled {
            /// Account that scheduled the dissolve.
            account: T::AccountId,
            /// Subnet that will be dissolved.
            netuid: NetUid,
            /// Block at which the dissolve extrinsic executes.
            execution_block: BlockNumberFor<T>,
        },
        /// Delay between coldkey-swap announcement and execution was set.
        ///
        /// Fields: `(announcement_delay)`.
        ColdkeySwapAnnouncementDelaySet(BlockNumberFor<T>),
        /// Delay required before re-announcing a coldkey swap was set.
        ///
        /// Fields: `(reannouncement_delay)`.
        ColdkeySwapReannouncementDelaySet(BlockNumberFor<T>),
        /// Duration used when scheduling network dissolve was set.
        ///
        /// Fields: `(schedule_duration)`.
        DissolveNetworkScheduleDurationSet(BlockNumberFor<T>),
        /// Commit-reveal v3 weights were committed (hash only; reveal comes later).
        ///
        /// Fields: `(who, netuid_index, commit_hash)`.
        CRV3WeightsCommitted(T::AccountId, NetUidStorageIndex, H256),
        /// Weights were committed under the commit-reveal flow (hash only).
        ///
        /// Fields: `(who, netuid_index, commit_hash)`.
        WeightsCommitted(T::AccountId, NetUidStorageIndex, H256),

        /// Previously committed weights were revealed on-chain.
        ///
        /// Fields: `(who, netuid_index, commit_hash)`.
        WeightsRevealed(T::AccountId, NetUidStorageIndex, H256),

        /// Multiple previously committed weight sets were revealed in one batch.
        ///
        /// Fields: `(who, netuid, revealed_hashes)`.
        WeightsBatchRevealed(T::AccountId, NetUid, Vec<H256>),

        /// A batch of weight sets / commits completed successfully for the listed netuids.
        ///
        /// Fields: `(netuids, hotkey)`.
        BatchWeightsCompleted(Vec<Compact<NetUid>>, T::AccountId),

        /// A batch weight extrinsic finished but at least one item failed (see `BatchWeightItemFailed`).
        BatchCompletedWithErrors(),

        /// One item inside a batch weight set/commit failed.
        ///
        /// Fields: `(netuid, error)`.
        BatchWeightItemFailed(NetUid, sp_runtime::DispatchError),

        /// Stake was transferred from one coldkey to another (same hotkey; subnets may differ).
        ///
        /// Fields: `(origin_coldkey, destination_coldkey, hotkey, origin_netuid, destination_netuid, tao)`.
        StakeTransferred(
            T::AccountId,
            T::AccountId,
            T::AccountId,
            NetUid,
            NetUid,
            TaoBalance,
        ),

        /// Stake was swapped from one subnet to another for the same coldkey–hotkey pair.
        ///
        /// Fields: `(coldkey, hotkey, origin_netuid, destination_netuid, tao)`.
        StakeSwapped(T::AccountId, T::AccountId, NetUid, NetUid, TaoBalance),

        /// Stake transfer to/from a subnet was enabled or disabled.
        ///
        /// Fields: `(netuid, transfers_enabled)`.
        TransferToggle(NetUid, bool),

        /// Owner hotkey for a subnet was set (hotkey authorized for owner actions).
        ///
        /// Fields: `(netuid, owner_hotkey)`.
        SubnetOwnerHotkeySet(NetUid, T::AccountId),
        /// First block at which a subnet may emit was set (typically via `start_call`).
        ///
        /// Fields: `(netuid, first_emission_block)`.
        FirstEmissionBlockNumberSet(NetUid, u64),

        /// Alpha was recycled from a stake position, reducing `AlphaOut` on the subnet.
        ///
        /// Fields: `(coldkey, hotkey, alpha, netuid)`.
        AlphaRecycled(T::AccountId, T::AccountId, AlphaBalance, NetUid),

        /// Alpha was burned from a stake position without reducing `AlphaOut`.
        ///
        /// Fields: `(coldkey, hotkey, alpha, netuid)`.
        AlphaBurned(T::AccountId, T::AccountId, AlphaBalance, NetUid),

        /// An EVM address was associated with a hotkey on a subnet.
        EvmKeyAssociated {
            /// Subnet the hotkey belongs to.
            netuid: NetUid,
            /// Hotkey associated with the EVM key.
            hotkey: T::AccountId,
            /// EVM address being associated.
            evm_key: H160,
            /// Block at which the association was recorded.
            block_associated: u64,
        },

        /// Commit-reveal v3 weights were revealed for a hotkey on a subnet.
        ///
        /// Fields: `(netuid, who)`.
        CRV3WeightsRevealed(NetUid, T::AccountId),

        /// Commit-reveal reveal period (epochs) was set for a subnet.
        ///
        /// Fields: `(netuid, periods)`.
        CommitRevealPeriodsSet(NetUid, u64),

        /// Commit-reveal weight setting was enabled or disabled for a subnet.
        ///
        /// Fields: `(netuid, enabled)`.
        CommitRevealEnabled(NetUid, bool),

        /// A coldkey swapped its hotkey association on a single subnet only.
        HotkeySwappedOnSubnet {
            /// Coldkey that owns the association.
            coldkey: T::AccountId,
            /// Hotkey being replaced on this subnet.
            old_hotkey: T::AccountId,
            /// Replacement hotkey on this subnet.
            new_hotkey: T::AccountId,
            /// Subnet where the hotkey association changed.
            netuid: NetUid,
        },
        /// A subnet lease was created for a beneficiary.
        SubnetLeaseCreated {
            /// Beneficiary of the lease.
            beneficiary: T::AccountId,
            /// Lease identifier.
            lease_id: LeaseId,
            /// Leased subnet.
            netuid: NetUid,
            /// Optional end block; `None` means open-ended until terminated.
            end_block: Option<BlockNumberFor<T>>,
        },

        /// A subnet lease was terminated.
        SubnetLeaseTerminated {
            /// Former beneficiary of the lease.
            beneficiary: T::AccountId,
            /// Subnet whose lease ended.
            netuid: NetUid,
        },

        /// Token symbol metadata for a subnet was updated.
        SymbolUpdated {
            /// Subnet whose symbol changed.
            netuid: NetUid,
            /// New symbol bytes.
            symbol: Vec<u8>,
        },

        /// Required commit-reveal protocol version was set globally.
        ///
        /// Fields: `(version)`.
        CommitRevealVersionSet(u16),

        /// Timelocked weights were committed (reveal allowed at `reveal_round`).
        ///
        /// Fields: `(who, netuid_index, commit_hash, reveal_round)`.
        TimelockedWeightsCommitted(T::AccountId, NetUidStorageIndex, H256, u64),

        /// Timelocked weights were revealed.
        ///
        /// Fields: `(netuid_index, who)`.
        TimelockedWeightsRevealed(NetUidStorageIndex, T::AccountId),

        /// Auto-staking path credited alpha incentive to a destination hotkey.
        AutoStakeAdded {
            /// Subnet of the auto-stake.
            netuid: NetUid,
            /// Destination account that received the auto-staked funds.
            destination: T::AccountId,
            /// Hotkey whose stake was auto-staked.
            hotkey: T::AccountId,
            /// Owner coldkey associated with the hotkey.
            owner: T::AccountId,
            /// Amount of alpha auto-staked (incentive).
            incentive: AlphaBalance,
        },

        /// End-of-epoch miner incentive alpha was emitted, indexed by uid.
        IncentiveAlphaEmittedToMiners {
            /// Subnet (mechanism) index for this emission.
            netuid: NetUidStorageIndex,
            /// UID-indexed miner incentive alpha; vector index equals uid.
            emissions: Vec<AlphaBalance>,
        },

        /// Minimum allowed uids for a subnet was set.
        ///
        /// Fields: `(netuid, min_allowed_uids)`.
        MinAllowedUidsSet(NetUid, u16),

        /// Auto-stake destination hotkey was set for a coldkey on a subnet.
        AutoStakeDestinationSet {
            /// Coldkey configuring the destination.
            coldkey: T::AccountId,
            /// Subnet the destination applies to.
            netuid: NetUid,
            /// Hotkey that will receive auto-staked funds.
            hotkey: T::AccountId,
        },

        /// Minimum number of non-immune uids required on a subnet was set.
        ///
        /// Fields: `(netuid, min_non_immune_uids)`.
        MinNonImmuneUidsSet(NetUid, u16),
        /// Root emissions were claimed for a coldkey across its subnets/hotkeys.
        RootClaimed {
            /// Coldkey that claimed root emissions.
            coldkey: T::AccountId,
        },

        /// Root claim type for a coldkey was configured.
        RootClaimTypeSet {
            /// Coldkey whose claim type changed.
            coldkey: T::AccountId,

            /// Selected root claim type.
            root_claim_type: RootClaimTypeEnum,
        },

        /// Voting-power tracking was enabled for a subnet.
        VotingPowerTrackingEnabled {
            /// Subnet where tracking started.
            netuid: NetUid,
        },

        /// Voting-power tracking disable was scheduled; tracking continues until `disable_at_block`.
        VotingPowerTrackingDisableScheduled {
            /// Subnet whose tracking will stop.
            netuid: NetUid,
            /// Block at which tracking disables and entries clear.
            disable_at_block: u64,
        },

        /// Voting-power tracking was fully disabled and entries cleared for a subnet.
        VotingPowerTrackingDisabled {
            /// Subnet where tracking stopped.
            netuid: NetUid,
        },

        /// Voting-power EMA alpha was set for a subnet (`u64` with 18-decimal fixed-point precision).
        VotingPowerEmaAlphaSet {
            /// Subnet whose EMA alpha changed.
            netuid: NetUid,
            /// New alpha value (u64 with 18 decimal precision).
            alpha: u64,
        },

        /// Subnet lease dividends (alpha) were distributed to a contributor.
        SubnetLeaseDividendsDistributed {
            /// Lease that paid the dividend.
            lease_id: LeaseId,
            /// Contributor receiving alpha.
            contributor: T::AccountId,
            /// Alpha amount distributed.
            alpha: AlphaBalance,
        },

        /// Add-stake-and-burn: TAO was used to buy alpha that was then burned.
        AddStakeBurn {
            /// Subnet where alpha was purchased and burned.
            netuid: NetUid,
            /// Hotkey path used for the stake/burn.
            hotkey: T::AccountId,
            /// TAO provided as input.
            amount: TaoBalance,
            /// Alpha that was burned.
            alpha: AlphaBalance,
        },

        /// Deferred cleanup of storage for a dissolved subnet completed.
        NetworkDissolveCleanupCompleted {
            /// Dissolved subnet whose residual maps were cleaned.
            netuid: NetUid,
        },

        /// A coldkey swap announcement was cleared without completing the swap.
        ColdkeySwapCleared {
            /// Coldkey that cleared its announcement.
            who: T::AccountId,
        },

        /// A transaction fee was paid in alpha (in addition to any TAO fee accounting).
        ///
        /// Emitted alongside fee payment when the fee path uses alpha; `alpha_fee` is the exact
        /// alpha deducted and `tao_amount` is the TAO-equivalent from the swap.
        TransactionFeePaidWithAlpha {
            /// Account that paid the fee.
            who: T::AccountId,
            /// Subnet whose alpha was used to pay the fee.
            netuid: NetUid,
            /// Exact fee deducted in alpha.
            alpha_fee: AlphaBalance,
            /// TAO amount obtained from swapping the alpha fee.
            tao_amount: TaoBalance,
        },
        /// Registration burn half-life was set for a subnet.
        BurnHalfLifeSet {
            /// Subnet whose burn half-life changed.
            netuid: NetUid,
            /// Burn half-life used by the registration burn schedule.
            burn_half_life: u16,
        },

        /// Registration burn increase multiplier was set for a subnet.
        BurnIncreaseMultSet {
            /// Subnet whose burn increase multiplier changed.
            netuid: NetUid,
            /// Multiplier applied when increasing registration burn.
            burn_increase_mult: U64F64,
        },

        /// A root validator toggled auto parent-delegation.
        AutoParentDelegationEnabledSet {
            /// Validator hotkey whose flag changed.
            hotkey: T::AccountId,
            /// Whether auto parent-delegation is now enabled.
            enabled: bool,
        },

        /// Stake (alpha) was locked to a hotkey on a subnet.
        StakeLocked {
            /// Coldkey that locked the stake.
            coldkey: T::AccountId,
            /// Hotkey the stake is locked to.
            hotkey: T::AccountId,
            /// Subnet the stake is locked on.
            netuid: NetUid,
            /// Alpha amount locked.
            amount: AlphaBalance,
        },

        /// Previously locked stake (alpha) was unlocked from a hotkey on a subnet.
        StakeUnlocked {
            /// Coldkey that unlocked the stake.
            coldkey: T::AccountId,
            /// Hotkey the stake was locked to.
            hotkey: T::AccountId,
            /// Subnet the stake was locked on.
            netuid: NetUid,
            /// Alpha amount unlocked.
            amount: AlphaBalance,
        },

        /// A stake lock was moved from one hotkey to another on the same subnet (same coldkey).
        LockMoved {
            /// Coldkey that moved the lock.
            coldkey: T::AccountId,
            /// Hotkey the lock was moved from.
            origin_hotkey: T::AccountId,
            /// Hotkey the lock was moved to.
            destination_hotkey: T::AccountId,
            /// Subnet the lock remains on.
            netuid: NetUid,
        },

        /// Activity-cutoff factor (per-mille) was set on a subnet by its owner.
        ActivityCutoffFactorMilliSet {
            /// Subnet whose activity-cutoff factor changed.
            netuid: NetUid,
            /// Factor in per-mille.
            factor_milli: u32,
        },

        /// Subnet owner manually triggered an epoch; execution is deferred until `fires_at`.
        EpochTriggered {
            /// Subnet whose epoch was triggered.
            netuid: NetUid,
            /// Account that triggered the epoch.
            by: T::AccountId,
            /// Earliest block at which the triggered epoch may execute.
            fires_at: u64,
        },

        /// An epoch slot was deferred to a later block due to the per-block epoch cap.
        EpochDeferred {
            /// Subnet whose epoch was deferred.
            netuid: NetUid,
            /// Block at which the epoch was originally scheduled.
            from_block: u64,
            /// Block to which the epoch was deferred.
            to_block: u64,
        },

        /// An epoch slot was skipped (e.g. inconsistent input state or other execution error).
        EpochSkipped {
            /// Subnet whose epoch was skipped.
            netuid: NetUid,
            /// Block at which the slot was consumed without running the epoch.
            block: u64,
        },

        /// Subnet ownership was reassigned (e.g. via lock conviction).
        SubnetOwnerChanged {
            /// Subnet whose owner changed.
            netuid: NetUid,
            /// Previous owner coldkey.
            old_coldkey: T::AccountId,
            /// New owner coldkey.
            new_coldkey: T::AccountId,
        },

        /// A coldkey's perpetual-lock flag was updated for a subnet.
        PerpetualLockUpdated {
            /// Coldkey whose flag changed.
            coldkey: T::AccountId,
            /// Subnet the flag applies to.
            netuid: NetUid,
            /// Whether this coldkey's locks on the subnet are now perpetual.
            enabled: bool,
        },

        /// A network registration was queued (pending activation / later materialization).
        NetworkRegistrationQueued {
            /// Coldkey that paid / owns the registration.
            coldkey: T::AccountId,
            /// Hotkey supplied at registration.
            hotkey: T::AccountId,
            /// Mechanism id used for the registration.
            mechid: u16,
            /// Optional subnet identity attached at registration.
            identity: Option<SubnetIdentityOfV3>,
            /// TAO locked for the registration.
            lock_amount: TaoBalance,
            /// Median subnet alpha price snapshot used for pricing.
            median_subnet_alpha_price: U64F64,
            /// Block at which the registration was queued.
            registration_block: u64,
        },

        /// A coldkey toggled whether it rejects incoming locked alpha.
        RejectLockedAlphaUpdated {
            /// Coldkey whose flag changed.
            coldkey: T::AccountId,
            /// Whether this coldkey rejects incoming locked alpha.
            enabled: bool,
        },

        /// Stake was transferred from one coldkey to another, landing on a different hotkey
        /// (and optionally a different subnet).
        StakeAndHotkeyTransferred {
            /// Coldkey the stake left.
            origin_coldkey: T::AccountId,
            /// Coldkey that now owns the stake.
            destination_coldkey: T::AccountId,
            /// Hotkey the stake left.
            origin_hotkey: T::AccountId,
            /// Hotkey the stake landed on.
            destination_hotkey: T::AccountId,
            /// Subnet the stake left.
            origin_netuid: NetUid,
            /// Subnet the stake landed on.
            destination_netuid: NetUid,
            /// TAO-equivalent amount moved.
            amount: TaoBalance,
        },

        /// Miner collateral was staked and locked (at registration or via `add_collateral`).
        ///
        /// Appended at the end of the enum so existing event indices stay stable.
        CollateralLocked {
            /// Subnet identifier.
            netuid: NetUid,
            /// Miner hotkey the collateral is attached to.
            hotkey: T::AccountId,
            /// Alpha locked by this operation.
            locked: AlphaBalance,
            /// Total alpha now locked for this hotkey on this subnet.
            total_locked: AlphaBalance,
        },

        /// A miner set the self-maintaining collateral floor for a hotkey.
        MinCollateralSet {
            /// Subnet identifier.
            netuid: NetUid,
            /// Miner hotkey the floor applies to.
            hotkey: T::AccountId,
            /// New floor; zero clears it.
            min_locked: AlphaBalance,
        },
    }
}
