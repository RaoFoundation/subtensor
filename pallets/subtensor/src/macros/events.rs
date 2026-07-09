use frame_support::pallet_macros::pallet_section;

/// A [`pallet_section`] that defines the events for a pallet.
/// This can later be imported into the pallet using [`import_section`].
#[pallet_section]
mod events {
    use codec::Compact;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// a new network is added.
        NetworkAdded(NetUid, u16),
        /// a network is removed.
        NetworkRemoved(NetUid),
        /// stake has been transferred from the a coldkey account onto the hotkey staking account.
        StakeAdded(
            T::AccountId,
            T::AccountId,
            TaoBalance,
            AlphaBalance,
            NetUid,
            u64,
        ),
        /// stake has been removed from the hotkey staking account onto the coldkey account.
        StakeRemoved(
            T::AccountId,
            T::AccountId,
            TaoBalance,
            AlphaBalance,
            NetUid,
            u64,
        ),
        /// stake has been moved from origin (hotkey, subnet ID) to destination (hotkey, subnet ID) of this amount (in TAO).
        StakeMoved(
            T::AccountId,
            T::AccountId,
            NetUid,
            T::AccountId,
            NetUid,
            TaoBalance,
        ),
        /// a caller successfully sets their weights on a subnetwork.
        WeightsSet(NetUidStorageIndex, u16),
        /// a new neuron account has been registered to the chain.
        NeuronRegistered(NetUid, u16, T::AccountId),
        /// multiple uids have been concurrently registered.
        BulkNeuronsRegistered(u16, u16),
        /// FIXME: Not used yet
        BulkBalancesSet(u16, u16),
        /// max allowed uids has been set for a subnetwork.
        MaxAllowedUidsSet(NetUid, u16),
        #[deprecated(note = "Max weight limit is now a constant and this event is unused")]
        /// DEPRECATED: max weight limit updates are no longer supported.
        MaxWeightLimitSet(NetUid, u16),
        /// the difficulty has been set for a subnet.
        DifficultySet(NetUid, u64),
        /// the adjustment interval is set for a subnet.
        AdjustmentIntervalSet(NetUid, u16),
        /// registration per interval is set for a subnet.
        RegistrationPerIntervalSet(NetUid, u16),
        /// we set max registrations per block.
        MaxRegistrationsPerBlockSet(NetUid, u16),
        /// an activity cutoff is set for a subnet.
        ActivityCutoffSet(NetUid, u16),
        /// Rho value is set.
        RhoSet(NetUid, u16),
        /// steepness of the sigmoid used to compute alpha values.
        AlphaSigmoidSteepnessSet(NetUid, i16),
        /// Kappa is set for a subnet.
        KappaSet(NetUid, u16),
        /// minimum allowed weight is set for a subnet.
        MinAllowedWeightSet(NetUid, u16),
        /// the validator pruning length has been set.
        ValidatorPruneLenSet(NetUid, u64),
        /// the scaling law power has been set for a subnet.
        ScalingLawPowerSet(NetUid, u16),
        /// weights set rate limit has been set for a subnet.
        WeightsSetRateLimitSet(NetUid, u64),
        /// immunity period is set for a subnet.
        ImmunityPeriodSet(NetUid, u16),
        /// bonds moving average is set for a subnet.
        BondsMovingAverageSet(NetUid, u64),
        /// bonds penalty is set for a subnet.
        BondsPenaltySet(NetUid, u16),
        /// bonds reset is set for a subnet.
        BondsResetOnSet(NetUid, bool),
        /// setting the max number of allowed validators on a subnet.
        MaxAllowedValidatorsSet(NetUid, u16),
        /// the axon server information is added to the network.
        AxonServed(NetUid, T::AccountId),
        /// the prometheus server information is added to the network.
        PrometheusServed(NetUid, T::AccountId),
        /// a hotkey has become a delegate.
        DelegateAdded(T::AccountId, T::AccountId, u16),
        /// the default take is set.
        DefaultTakeSet(u16),
        /// weights version key is set for a network.
        WeightsVersionKeySet(NetUid, u64),
        /// setting min difficulty on a network.
        MinDifficultySet(NetUid, u64),
        /// setting max difficulty on a network.
        MaxDifficultySet(NetUid, u64),
        /// setting the prometheus serving rate limit.
        ServingRateLimitSet(NetUid, u64),
        /// setting burn on a network.
        BurnSet(NetUid, TaoBalance),
        /// setting max burn on a network.
        MaxBurnSet(NetUid, TaoBalance),
        /// setting min burn on a network.
        MinBurnSet(NetUid, TaoBalance),
        /// setting the per-block epoch cap (dynamic tempo throttle).
        MaxEpochsPerBlockSet(u8),
        /// setting the transaction rate limit.
        TxRateLimitSet(u64),
        /// setting the delegate take transaction rate limit.
        TxDelegateTakeRateLimitSet(u64),
        /// setting the childkey take transaction rate limit.
        TxChildKeyTakeRateLimitSet(u64),
        /// setting the admin freeze window length (last N blocks of tempo)
        AdminFreezeWindowSet(u16),
        /// setting the owner hyperparameter rate limit in epochs
        OwnerHyperparamRateLimitSet(u16),
        /// minimum childkey take set
        MinChildKeyTakeSet(u16),
        /// subnet-specific minimum childkey take set
        MinChildKeyTakePerSubnetSet(NetUid, u16),
        /// maximum childkey take set
        MaxChildKeyTakeSet(u16),
        /// childkey take set
        ChildKeyTakeSet(T::AccountId, u16),
        /// a sudo call is done.
        Sudid(DispatchResult),
        /// registration is allowed/disallowed for a subnet.
        RegistrationAllowed(NetUid, bool),
        /// POW registration is allowed/disallowed for a subnet.
        PowRegistrationAllowed(NetUid, bool),
        /// setting tempo on a network
        TempoSet(NetUid, u16),
        /// setting the RAO recycled for registration.
        RAORecycledForRegistrationSet(NetUid, TaoBalance),
        /// min stake is set for validators to set weights.
        StakeThresholdSet(u64),
        /// setting the adjustment alpha on a subnet.
        AdjustmentAlphaSet(NetUid, u64),
        /// the faucet it called on the test net.
        Faucet(T::AccountId, u64),
        /// the subnet owner cut is set.
        SubnetOwnerCutSet(u16),
        /// the network creation rate limit is set.
        NetworkRateLimitSet(u64),
        /// the network immunity period is set.
        NetworkImmunityPeriodSet(u64),
        /// the start call delay is set.
        StartCallDelaySet(u64),
        /// the network minimum locking cost is set.
        NetworkMinLockCostSet(TaoBalance),
        /// the maximum number of subnets is set
        SubnetLimitSet(u16),
        /// the lock cost reduction is set
        NetworkLockCostReductionIntervalSet(u64),
        /// the take for a delegate is decreased.
        TakeDecreased(T::AccountId, T::AccountId, u16),
        /// the take for a delegate is increased.
        TakeIncreased(T::AccountId, T::AccountId, u16),
        /// the hotkey is swapped
        HotkeySwapped {
            /// the account ID of coldkey
            coldkey: T::AccountId,
            /// the account ID of old hotkey
            old_hotkey: T::AccountId,
            /// the account ID of new hotkey
            new_hotkey: T::AccountId,
        },
        /// maximum delegate take is set by sudo/admin transaction
        MaxDelegateTakeSet(u16),
        /// minimum delegate take is set by sudo/admin transaction
        MinDelegateTakeSet(u16),
        /// A coldkey swap announcement has been made.
        ColdkeySwapAnnounced {
            /// The account ID of the coldkey that made the announcement.
            who: T::AccountId,
            /// The hash of the new coldkey.
            new_coldkey_hash: T::Hash,
        },
        /// A coldkey swap has been reset.
        ColdkeySwapReset {
            /// The account ID of the coldkey for which the swap has been reset.
            who: T::AccountId,
        },
        /// A coldkey has been swapped.
        ColdkeySwapped {
            /// The account ID of old coldkey.
            old_coldkey: T::AccountId,
            /// The account ID of new coldkey.
            new_coldkey: T::AccountId,
        },
        /// A coldkey swap has been disputed.
        ColdkeySwapDisputed {
            /// The account ID of the coldkey that was disputed.
            coldkey: T::AccountId,
        },
        /// All balance of a hotkey has been unstaked and transferred to a new coldkey
        AllBalanceUnstakedAndTransferredToNewColdkey {
            /// The account ID of the current coldkey
            current_coldkey: T::AccountId,
            /// The account ID of the new coldkey
            new_coldkey: T::AccountId,
            /// The total balance of the hotkey
            total_balance: <<T as Config>::Currency as fungible::Inspect<
                <T as frame_system::Config>::AccountId,
            >>::Balance,
        },
        /// The arbitration period has been extended
        ArbitrationPeriodExtended {
            /// The account ID of the coldkey
            coldkey: T::AccountId,
        },
        /// Setting of children of a hotkey have been scheduled
        SetChildrenScheduled(T::AccountId, NetUid, u64, Vec<(u64, T::AccountId)>),
        /// The children of a hotkey have been set
        SetChildren(T::AccountId, NetUid, Vec<(u64, T::AccountId)>),
        // /// The hotkey emission tempo has been set
        // HotkeyEmissionTempoSet(u64),
        // /// The network maximum stake has been set
        // NetworkMaxStakeSet(u16, u64),
        /// The identity of a coldkey has been set
        ChainIdentitySet(T::AccountId),
        /// The identity of a subnet has been set
        SubnetIdentitySet(NetUid),
        /// The identity of a subnet has been removed
        SubnetIdentityRemoved(NetUid),
        /// A dissolve network extrinsic scheduled.
        DissolveNetworkScheduled {
            /// The account ID schedule the dissolve network extrinsic
            account: T::AccountId,
            /// network ID will be dissolved
            netuid: NetUid,
            /// extrinsic execution block number
            execution_block: BlockNumberFor<T>,
        },
        /// The coldkey swap announcement delay has been set.
        ColdkeySwapAnnouncementDelaySet(BlockNumberFor<T>),
        /// The coldkey swap reannouncement delay has been set.
        ColdkeySwapReannouncementDelaySet(BlockNumberFor<T>),
        /// The duration of dissolve network has been set
        DissolveNetworkScheduleDurationSet(BlockNumberFor<T>),
        /// Commit-reveal v3 weights have been successfully committed.
        ///
        /// - **who**: The account ID of the user committing the weights.
        /// - **netuid**: The network identifier.
        /// - **commit_hash**: The hash representing the committed weights.
        CRV3WeightsCommitted(T::AccountId, NetUidStorageIndex, H256),
        /// Weights have been successfully committed.
        ///
        /// - **who**: The account ID of the user committing the weights.
        /// - **netuid**: The network identifier.
        /// - **commit_hash**: The hash representing the committed weights.
        WeightsCommitted(T::AccountId, NetUidStorageIndex, H256),

        /// Weights have been successfully revealed.
        ///
        /// - **who**: The account ID of the user revealing the weights.
        /// - **netuid**: The network identifier.
        /// - **commit_hash**: The hash of the revealed weights.
        WeightsRevealed(T::AccountId, NetUidStorageIndex, H256),

        /// Weights have been successfully batch revealed.
        ///
        /// - **who**: The account ID of the user revealing the weights.
        /// - **netuid**: The network identifier.
        /// - **revealed_hashes**: A vector of hashes representing each revealed weight set.
        WeightsBatchRevealed(T::AccountId, NetUid, Vec<H256>),

        /// A batch of weights (or commits) have been force-set.
        ///
        /// - **netuids**: The netuids these weights were successfully set/committed for.
        /// - **who**: The hotkey that set this batch.
        BatchWeightsCompleted(Vec<Compact<NetUid>>, T::AccountId),

        /// A batch extrinsic completed but with some errors.
        BatchCompletedWithErrors(),

        /// A weight set among a batch of weights failed.
        ///
        /// - **netuid**: The netuid of the batch item that failed.
        /// - **error**: The dispatch error emitted by the failed item.
        BatchWeightItemFailed(NetUid, sp_runtime::DispatchError),

        /// Stake has been transferred from one coldkey to another on the same subnet.
        /// Parameters:
        /// (origin_coldkey, destination_coldkey, hotkey, origin_netuid, destination_netuid, amount)
        StakeTransferred(
            T::AccountId,
            T::AccountId,
            T::AccountId,
            NetUid,
            NetUid,
            TaoBalance,
        ),

        /// Stake has been swapped from one subnet to another for the same coldkey-hotkey pair.
        ///
        /// Parameters:
        /// (coldkey, hotkey, origin_netuid, destination_netuid, amount)
        StakeSwapped(T::AccountId, T::AccountId, NetUid, NetUid, TaoBalance),

        /// Event called when transfer is toggled on a subnet.
        ///
        /// Parameters:
        /// (netuid, bool)
        TransferToggle(NetUid, bool),

        /// The owner hotkey for a subnet has been set.
        ///
        /// Parameters:
        /// (netuid, new_hotkey)
        SubnetOwnerHotkeySet(NetUid, T::AccountId),
        /// FirstEmissionBlockNumber is set via start call extrinsic
        ///
        /// Parameters:
        /// netuid
        /// block number
        FirstEmissionBlockNumberSet(NetUid, u64),

        /// Alpha has been recycled, reducing AlphaOut on a subnet.
        ///
        /// Parameters:
        /// (coldkey, hotkey, amount, subnet_id)
        AlphaRecycled(T::AccountId, T::AccountId, AlphaBalance, NetUid),

        /// Alpha have been burned without reducing AlphaOut.
        ///
        /// Parameters:
        /// (coldkey, hotkey, amount, subnet_id)
        AlphaBurned(T::AccountId, T::AccountId, AlphaBalance, NetUid),

        /// An EVM key has been associated with a hotkey.
        EvmKeyAssociated {
            /// The subnet that the hotkey belongs to.
            netuid: NetUid,
            /// The hotkey associated with the EVM key.
            hotkey: T::AccountId,
            /// The EVM key being associated with the hotkey.
            evm_key: H160,
            /// The block where the association happened.
            block_associated: u64,
        },

        /// CRV3 Weights have been successfully revealed.
        ///
        /// - **netuid**: The network identifier.
        /// - **who**: The account ID of the user revealing the weights.
        CRV3WeightsRevealed(NetUid, T::AccountId),

        /// Commit-Reveal periods has been successfully set.
        ///
        /// - **netuid**: The network identifier.
        /// - **periods**: The number of epochs before the reveal.
        CommitRevealPeriodsSet(NetUid, u64),

        /// Commit-Reveal has been successfully toggled.
        ///
        /// - **netuid**: The network identifier.
        /// - **Enabled**: Is Commit-Reveal enabled.
        CommitRevealEnabled(NetUid, bool),

        /// the hotkey is swapped
        HotkeySwappedOnSubnet {
            /// the account ID of coldkey
            coldkey: T::AccountId,
            /// the account ID of old hotkey
            old_hotkey: T::AccountId,
            /// the account ID of new hotkey
            new_hotkey: T::AccountId,
            /// the subnet ID
            netuid: NetUid,
        },
        /// A subnet lease has been created.
        SubnetLeaseCreated {
            /// The beneficiary of the lease.
            beneficiary: T::AccountId,
            /// The lease ID
            lease_id: LeaseId,
            /// The subnet ID
            netuid: NetUid,
            /// The end block of the lease
            end_block: Option<BlockNumberFor<T>>,
        },

        /// A subnet lease has been terminated.
        SubnetLeaseTerminated {
            /// The beneficiary of the lease.
            beneficiary: T::AccountId,
            /// The subnet ID
            netuid: NetUid,
        },

        /// The symbol for a subnet has been updated.
        SymbolUpdated {
            /// The subnet ID
            netuid: NetUid,
            /// The symbol that has been updated.
            symbol: Vec<u8>,
        },

        /// Commit Reveal Weights version has been updated.
        ///
        /// - **version**: The required version.
        CommitRevealVersionSet(u16),

        /// Timelocked weights have been successfully committed.
        ///
        /// - **who**: The account ID of the user committing the weights.
        /// - **netuid**: The network identifier.
        /// - **commit_hash**: The hash representing the committed weights.
        /// - **reveal_round**: The round at which weights can be revealed.
        TimelockedWeightsCommitted(T::AccountId, NetUidStorageIndex, H256, u64),

        /// Timelocked Weights have been successfully revealed.
        ///
        /// - **netuid**: The network identifier.
        /// - **who**: The account ID of the user revealing the weights.
        TimelockedWeightsRevealed(NetUidStorageIndex, T::AccountId),

        /// Auto-staking hotkey received stake
        AutoStakeAdded {
            /// Subnet identifier.
            netuid: NetUid,
            /// Destination account that received the auto-staked funds.
            destination: T::AccountId,
            /// Hotkey account whose stake was auto-staked.
            hotkey: T::AccountId,
            /// Owner (coldkey) account associated with the hotkey.
            owner: T::AccountId,
            /// Amount of alpha auto-staked.
            incentive: AlphaBalance,
        },

        /// End-of-epoch miner incentive alpha by UID
        IncentiveAlphaEmittedToMiners {
            /// Subnet identifier.
            netuid: NetUidStorageIndex,
            /// UID-indexed array of miner incentive alpha; index equals UID.
            emissions: Vec<AlphaBalance>,
        },

        /// The minimum allowed UIDs for a subnet have been set.
        MinAllowedUidsSet(NetUid, u16),

        /// The auto stake destination has been set.
        ///
        /// - **coldkey**: The account ID of the coldkey.
        /// - **netuid**: The network identifier.
        /// - **hotkey**: The account ID of the hotkey.
        AutoStakeDestinationSet {
            /// The account ID of the coldkey.
            coldkey: T::AccountId,
            /// The network identifier.
            netuid: NetUid,
            /// The account ID of the hotkey.
            hotkey: T::AccountId,
        },

        /// The minimum allowed non-Immune UIDs has been set.
        MinNonImmuneUidsSet(NetUid, u16),
        /// Root emissions have been claimed for a coldkey on all subnets and hotkeys.
        /// Parameters:
        /// (coldkey)
        RootClaimed {
            /// Claim coldkey
            coldkey: T::AccountId,
        },

        /// Root claim type for a coldkey has been set.
        /// Parameters:
        /// (coldkey, u8)
        RootClaimTypeSet {
            /// Claim coldkey
            coldkey: T::AccountId,

            /// Claim type
            root_claim_type: RootClaimTypeEnum,
        },

        /// Voting power tracking has been enabled for a subnet.
        VotingPowerTrackingEnabled {
            /// The subnet ID
            netuid: NetUid,
        },

        /// Voting power tracking has been scheduled for disabling.
        /// Tracking will continue until disable_at_block, then stop and clear entries.
        VotingPowerTrackingDisableScheduled {
            /// The subnet ID
            netuid: NetUid,
            /// Block at which tracking will be disabled
            disable_at_block: u64,
        },

        /// Voting power tracking has been fully disabled and entries cleared.
        VotingPowerTrackingDisabled {
            /// The subnet ID
            netuid: NetUid,
        },

        /// Voting power EMA alpha has been set for a subnet.
        VotingPowerEmaAlphaSet {
            /// The subnet ID
            netuid: NetUid,
            /// The new alpha value (u64 with 18 decimal precision)
            alpha: u64,
        },

        /// Subnet lease dividends have been distributed.
        SubnetLeaseDividendsDistributed {
            /// The lease ID
            lease_id: LeaseId,
            /// The contributor
            contributor: T::AccountId,
            /// The amount of alpha distributed
            alpha: AlphaBalance,
        },

        /// "Add stake and burn" event: alpha token was purchased and burned.
        AddStakeBurn {
            /// The subnet ID
            netuid: NetUid,
            /// hotky account ID
            hotkey: T::AccountId,
            /// Tao provided
            amount: TaoBalance,
            /// Alpha burned
            alpha: AlphaBalance,
        },

        /// data for a dissolved network has been cleaned up.
        NetworkDissolveCleanupCompleted {
            /// The subnet ID
            netuid: NetUid,
        },

        /// A coldkey swap announcement has been cleared.
        ColdkeySwapCleared {
            /// The account ID of the coldkey that cleared the announcement.
            who: T::AccountId,
        },

        /// Transaction fee was paid in Alpha.
        ///
        /// Emitted in addition to `TransactionFeePaid` when the fee payment path is Alpha.
        /// `alpha_fee` is the exact Alpha amount deducted.
        TransactionFeePaidWithAlpha {
            /// Account that paid the transaction fee.
            who: T::AccountId,
            /// Netuid
            netuid: NetUid,
            /// Exact fee deducted in Alpha units.
            alpha_fee: AlphaBalance,
            /// Resulting swapped TAO amount
            tao_amount: TaoBalance,
        },
        /// Burn half-life set for neuron registration.
        BurnHalfLifeSet {
            /// The subnet identifier.
            netuid: NetUid,
            /// The burn half-life value for neuron registration.
            burn_half_life: u16,
        },

        /// Burn increase multiplier set for neuron registration.
        BurnIncreaseMultSet {
            /// The subnet identifier.
            netuid: NetUid,
            /// The burn increase multiplier value for neuron registration.
            burn_increase_mult: U64F64,
        },

        /// A root validator toggled the "auto parent delegation" flag.
        AutoParentDelegationEnabledSet {
            /// The validator hotkey.
            hotkey: T::AccountId,
            /// Whether delegation is now enabled.
            enabled: bool,
        },

        /// Stake has been locked to a hotkey on a subnet.
        StakeLocked {
            /// The coldkey that locked the stake.
            coldkey: T::AccountId,
            /// The hotkey the stake is locked to.
            hotkey: T::AccountId,
            /// The subnet the stake is locked on.
            netuid: NetUid,
            /// The alpha amount locked.
            amount: AlphaBalance,
        },

        /// Stake has been unlocked from a hotkey on a subnet.
        StakeUnlocked {
            /// The coldkey that unlocked the stake.
            coldkey: T::AccountId,
            /// The hotkey the stake was locked to.
            hotkey: T::AccountId,
            /// The subnet the stake was locked on.
            netuid: NetUid,
            /// The alpha amount unlocked.
            amount: AlphaBalance,
        },

        /// Stake has been unlocked from a hotkey on a subnet.
        LockMoved {
            /// The coldkey that moved the lock.
            coldkey: T::AccountId,
            /// The hotkey the lock was moved from.
            origin_hotkey: T::AccountId,
            /// The hotkey the lock was moved to.
            destination_hotkey: T::AccountId,
            /// The subnet the lock is on.
            netuid: NetUid,
        },

        /// Activity-cutoff factor (per-mille) set on a subnet by its owner.
        ActivityCutoffFactorMilliSet {
            /// The subnet identifier.
            netuid: NetUid,
            /// Factor (per-mille).
            factor_milli: u32,
        },

        /// Owner manually triggered an epoch for their subnet.
        EpochTriggered {
            /// The subnet identifier.
            netuid: NetUid,
            /// The account that triggered the epoch.
            by: T::AccountId,
            /// The earliest block at which the triggered epoch may execute.
            fires_at: u64,
        },

        /// An epoch slot was deferred to the next block due to the per-block epoch cap.
        EpochDeferred {
            /// The subnet identifier.
            netuid: NetUid,
            /// Block at which the epoch was originally scheduled.
            from_block: u64,
            /// Block to which the epoch was deferred.
            to_block: u64,
        },

        /// Epoch execution skipped by `is_epoch_input_state_consistent` returned false or other errors.
        EpochSkipped {
            /// The subnet identifier.
            netuid: NetUid,
            /// The block at which the slot was consumed.
            block: u64,
        },

        /// Subnet ownership was reassigned by lock conviction.
        SubnetOwnerChanged {
            /// The subnet whose owner changed.
            netuid: NetUid,
            /// The previous owner coldkey.
            old_coldkey: T::AccountId,
            /// The new owner coldkey.
            new_coldkey: T::AccountId,
        },

        /// A coldkey's perpetual lock flag was updated.
        PerpetualLockUpdated {
            /// The coldkey whose flag changed.
            coldkey: T::AccountId,
            /// The subnet whose coldkey flag changed.
            netuid: NetUid,
            /// Whether this coldkey's locks are now perpetual.
            enabled: bool,
        },

        /// A network registration cost has been queued.
        NetworkRegistrationQueued {
            /// The network registration information.
            coldkey: T::AccountId,
            /// The hotkey that registered the network.
            hotkey: T::AccountId,
            /// The mechanism that registered the network.
            mechid: u16,
            /// The identity that registered the network.
            identity: Option<SubnetIdentityOfV3>,
            /// The lock amount that registered the network.
            lock_amount: TaoBalance,
            /// The median subnet alpha price that registered the network.
            median_subnet_alpha_price: U64F64,
            /// The block at which the network was registered.
            registration_block: u64,
        },

        /// A coldkey's reject locked alpha account flag was updated.
        RejectLockedAlphaUpdated {
            /// The coldkey whose flag changed.
            coldkey: T::AccountId,
            /// Whether this coldkey rejects incoming locked alpha.
            enabled: bool,
        },
    }
}
