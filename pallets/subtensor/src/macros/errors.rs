use frame_support::pallet_macros::pallet_section;

/// A [`pallet_section`] that defines the errors for a pallet.
/// This can later be imported into the pallet using [`import_section`].
#[pallet_section]
mod errors {
    #[derive(PartialEq)]
    #[pallet::error]
    pub enum Error<T> {
        /// The root network does not exist.
        RootNetworkDoesNotExist,
        /// The user is trying to serve an axon which is not of type 4 (IPv4) or 6 (IPv6).
        InvalidIpType,
        /// An invalid IP address is passed to the serve function.
        InvalidIpAddress,
        /// An invalid port is passed to the serve function.
        InvalidPort,
        /// The hotkey is not registered in subnet
        HotKeyNotRegisteredInSubNet,
        /// The hotkey does not exists
        HotKeyAccountNotExists,
        /// The hotkey is not registered in any subnet.
        HotKeyNotRegisteredInNetwork,
        /// Request to stake, unstake or subscribe is made by a coldkey that is not associated with
        /// the hotkey account.
        NonAssociatedColdKey,
        /// DEPRECATED: Stake amount to withdraw is zero.
        // StakeToWithdrawIsZero,
        /// The caller does not have enought stake to perform this action.
        NotEnoughStake,
        /// The caller is requesting removing more stake than there exists in the staking account.
        /// See: "[remove_stake()]".
        NotEnoughStakeToWithdraw,
        /// The caller is requesting to set weights but the caller has less than minimum stake
        /// required to set weights (less than WeightsMinStake).
        NotEnoughStakeToSetWeights,
        /// The parent hotkey doesn't have enough own stake to set childkeys.
        NotEnoughStakeToSetChildkeys,
        /// The caller is requesting adding more stake than there exists in the coldkey account.
        /// See: "[add_stake()]"
        NotEnoughBalanceToStake,
        /// The caller is trying to add stake, but for some reason the requested amount could not be
        /// withdrawn from the coldkey account.
        BalanceWithdrawalError,
        /// Unsuccessfully withdraw, balance could be zero (can not make account exist) after
        /// withdrawal.
        ZeroBalanceAfterWithdrawn,
        /// The caller is attempting to set non-self weights without being a permitted validator.
        NeuronNoValidatorPermit,
        /// The caller is attempting to set the weight keys and values but these vectors have
        /// different size.
        WeightVecNotEqualSize,
        /// The caller is attempting to set weights with duplicate UIDs in the weight matrix.
        DuplicateUids,
        /// The caller is attempting to set weight to at least one UID that does not exist in the
        /// metagraph.
        UidVecContainInvalidOne,
        /// The dispatch is attempting to set weights on chain with fewer elements than are allowed.
        WeightVecLengthIsLow,
        /// Number of registrations in this block exceeds the allowed number (i.e., exceeds the
        /// subnet hyperparameter "max_regs_per_block").
        TooManyRegistrationsThisBlock,
        /// The caller is requesting registering a neuron which already exists in the active set.
        HotKeyAlreadyRegisteredInSubNet,
        /// The new hotkey is the same as old one
        NewHotKeyIsSameWithOld,
        /// The new hotkey has outstanding root claimable or non-zero root stake,
        /// so the root rate-book cannot be merged without misallocating dividends.
        NewHotKeyNotCleanForRootSwap,
        /// The supplied PoW hash block is in the future or negative.
        InvalidWorkBlock,
        /// The supplied PoW hash block does not meet the network difficulty.
        InvalidDifficulty,
        /// The supplied PoW hash seal does not match the supplied work.
        InvalidSeal,
        /// The dispatch is attempting to set weights on chain with weight value exceeding the
        /// configured max weight limit (currently `u16::MAX`).
        MaxWeightExceeded,
        /// The hotkey is attempting to become a delegate when the hotkey is already a delegate.
        HotKeyAlreadyDelegate,
        /// A transactor exceeded the rate limit for setting weights.
        SettingWeightsTooFast,
        /// A validator is attempting to set weights from a validator with incorrect weight version.
        IncorrectWeightVersionKey,
        /// An axon or prometheus serving exceeded the rate limit for a registered neuron.
        ServingRateLimitExceeded,
        /// The caller is attempting to set weights with more UIDs than allowed.
        UidsLengthExceedUidsInSubNet, // 32
        /// A transactor exceeded the rate limit for add network transaction.
        NetworkTxRateLimitExceeded,
        /// A transactor exceeded the rate limit for delegate transaction.
        DelegateTxRateLimitExceeded,
        /// A transactor exceeded the rate limit for setting or swapping hotkey.
        HotKeySetTxRateLimitExceeded,
        /// A transactor exceeded the rate limit for staking.
        StakingRateLimitExceeded,
        /// Registration is disabled.
        SubNetRegistrationDisabled,
        /// The number of registration attempts exceeded the allowed number in the interval.
        TooManyRegistrationsThisInterval,
        /// The hotkey is required to be the origin.
        TransactorAccountShouldBeHotKey,
        /// Faucet is disabled.
        FaucetDisabled,
        /// Not a subnet owner.
        NotSubnetOwner,
        /// Operation is not permitted on the root subnet.
        RegistrationNotPermittedOnRootSubnet,
        /// A hotkey with too little stake is attempting to join the root subnet.
        StakeTooLowForRoot,
        /// All subnets are in the immunity period.
        AllNetworksInImmunity,
        /// Not enough balance to pay swapping hotkey.
        NotEnoughBalanceToPaySwapHotKey,
        /// Netuid does not match for setting root network weights.
        NotRootSubnet,
        /// Can not set weights for the root network.
        CanNotSetRootNetworkWeights,
        /// No neuron ID is available.
        NoNeuronIdAvailable,
        /// Delegate take is too low.
        DelegateTakeTooLow,
        /// Delegate take is too high.
        DelegateTakeTooHigh,
        /// No commit found for the provided hotkey+netuid combination when attempting to reveal the
        /// weights.
        NoWeightsCommitFound,
        /// Committed hash does not equal the hashed reveal data.
        InvalidRevealCommitHashNotMatch,
        /// Attempting to call set_weights when commit/reveal is enabled
        CommitRevealEnabled,
        /// Attemtping to commit/reveal weights when disabled.
        CommitRevealDisabled,
        /// Attempting to set alpha high/low while disabled
        LiquidAlphaDisabled,
        /// Alpha high is too low: alpha_high > 0.8
        AlphaHighTooLow,
        /// Alpha low is out of range: alpha_low > 0 && alpha_low < 0.8
        AlphaLowOutOfRange,
        /// The coldkey has already been swapped
        ColdKeyAlreadyAssociated,
        /// The coldkey balance is not enough to pay for the swap
        NotEnoughBalanceToPaySwapColdKey,
        /// Attempting to set an invalid child for a hotkey on a network.
        InvalidChild,
        /// Duplicate child when setting children.
        DuplicateChild,
        /// Proportion overflow when setting children.
        ProportionOverflow,
        /// Too many children MAX 5.
        TooManyChildren,
        /// Default transaction rate limit exceeded.
        TxRateLimitExceeded,
        /// Coldkey swap announcement not found
        ColdkeySwapAnnouncementNotFound,
        /// Coldkey swap too early.
        ColdkeySwapTooEarly,
        /// Coldkey swap reannounced too early.
        ColdkeySwapReannouncedTooEarly,
        /// The announced coldkey hash does not match the new coldkey hash.
        AnnouncedColdkeyHashDoesNotMatch,
        /// Coldkey swap already disputed
        ColdkeySwapAlreadyDisputed,
        /// New coldkey is hotkey
        NewColdKeyIsHotkey,
        /// Childkey take is invalid.
        InvalidChildkeyTake,
        /// Childkey take rate limit exceeded.
        TxChildkeyTakeRateLimitExceeded,
        /// Invalid identity.
        InvalidIdentity,
        /// Subnet mechanism does not exist.
        MechanismDoesNotExist,
        /// Trying to unstake or re-lock the locked amount.
        StakeUnavailable,
        /// Trying to perform action on non-existent subnet.
        SubnetNotExists,
        /// Maximum commit limit reached
        TooManyUnrevealedCommits,
        /// Attempted to reveal weights that are expired.
        ExpiredWeightCommit,
        /// Attempted to reveal weights too early.
        RevealTooEarly,
        /// Attempted to batch reveal weights with mismatched vector input lenghts.
        InputLengthsUnequal,
        /// A transactor exceeded the rate limit for setting weights.
        CommittingWeightsTooFast,
        /// Stake amount is too low.
        AmountTooLow,
        /// Not enough liquidity.
        InsufficientLiquidity,
        /// Slippage is too high for the transaction.
        SlippageTooHigh,
        /// Subnet disallows transfer.
        TransferDisallowed,
        /// Activity cutoff is being set too low.
        ActivityCutoffTooLow,
        /// Call is disabled
        CallDisabled,
        /// FirstEmissionBlockNumber is already set.
        FirstEmissionBlockNumberAlreadySet,
        /// need wait for more blocks to accept the start call extrinsic.
        NeedWaitingMoreBlocksToStarCall,
        /// Not enough AlphaOut on the subnet to recycle
        NotEnoughAlphaOutToRecycle,
        /// Cannot burn or recycle TAO from root subnet
        CannotBurnOrRecycleOnRootSubnet,
        /// Public key cannot be recovered.
        UnableToRecoverPublicKey,
        /// Recovered public key is invalid.
        InvalidRecoveredPublicKey,
        /// SubToken disabled now
        SubtokenDisabled,
        /// Too frequent hotkey swap on subnet
        HotKeySwapOnSubnetIntervalNotPassed,
        /// Invalid netuid duplication
        SameNetuid,
        /// The caller does not have enough balance for the operation.
        InsufficientBalance,
        /// Invalid lease beneficiary to register the leased network.
        InvalidLeaseBeneficiary,
        /// Lease cannot end in the past.
        LeaseCannotEndInThePast,
        /// Couldn't find the lease netuid.
        LeaseNetuidNotFound,
        /// Lease does not exist.
        LeaseDoesNotExist,
        /// Lease has no end block.
        LeaseHasNoEndBlock,
        /// Lease has not ended.
        LeaseHasNotEnded,
        /// An overflow occurred.
        Overflow,
        /// Beneficiary does not own hotkey.
        BeneficiaryDoesNotOwnHotkey,
        /// Expected beneficiary origin.
        ExpectedBeneficiaryOrigin,
        /// Admin operation is prohibited during the protected weights window
        AdminActionProhibitedDuringWeightsWindow,
        /// Symbol does not exist.
        SymbolDoesNotExist,
        /// Symbol already in use.
        SymbolAlreadyInUse,
        /// Incorrect commit-reveal version.
        IncorrectCommitRevealVersion,
        /// Reveal round is older than the most recently stored DRAND round.
        InvalidRevealRound,
        /// Reveal period is too large.
        RevealPeriodTooLarge,
        /// Reveal period is too small.
        RevealPeriodTooSmall,
        /// Generic error for out-of-range parameter value
        InvalidValue,
        /// Subnet limit reached & there is no eligible subnet to prune
        SubnetLimitReached,
        /// Insufficient funds to meet the subnet lock cost
        CannotAffordLockCost,
        /// exceeded the rate limit for associating an EVM key.
        EvmKeyAssociateRateLimitExceeded,
        /// Same auto stake hotkey already set
        SameAutoStakeHotkeyAlreadySet,
        /// The UID map for the subnet could not be cleared
        UidMapCouldNotBeCleared,
        /// Trimming would exceed the max immune neurons percentage
        TrimmingWouldExceedMaxImmunePercentage,
        /// Violating the rules of Childkey-Parentkey consistency
        ChildParentInconsistency,
        /// Invalid number of root claims
        InvalidNumRootClaim,
        /// Invalid value of root claim threshold
        InvalidRootClaimThreshold,
        /// Exceeded subnet limit number or zero.
        InvalidSubnetNumber,
        /// The maximum allowed UIDs times mechanism count should not exceed 256.
        TooManyUIDsPerMechanism,
        /// Voting power tracking is not enabled for this subnet.
        VotingPowerTrackingNotEnabled,
        /// Invalid voting power EMA alpha value (must be <= 10^18).
        InvalidVotingPowerEmaAlpha,
        /// Deprecated call.
        Deprecated,
        /// "Add stake and burn" exceeded the operation rate limit
        AddStakeBurnRateLimitExceeded,
        /// A coldkey swap has been announced for this account.
        ColdkeySwapAnnounced,
        /// A coldkey swap for this account is under dispute.
        ColdkeySwapDisputed,
        /// Coldkey swap clear too early.
        ColdkeySwapClearTooEarly,
        /// Disabled temporarily.
        DisabledTemporarily,
        /// Registration Price Limit Exceeded
        RegistrationPriceLimitExceeded,
        /// Lock hotkey mismatch: existing lock is for a different hotkey.
        LockHotkeyMismatch,
        /// Insufficient stake on subnet to cover the lock amount.
        InsufficientStakeForLock,
        /// No existing lock found for the given coldkey and subnet.
        NoExistingLock,
        /// There is already an active lock for the given coldkey.
        ActiveLockExists,
        /// A system account cannot be used in this operation
        CannotUseSystemAccount,
        /// Trying to unlock more than locked
        UnlockAmountTooHigh,
        /// The supplied tempo is outside the allowed range.
        TempoOutOfBounds,
        /// The supplied activity-cutoff factor is outside the allowed range.
        ActivityCutoffFactorMilliOutOfBounds,
        /// An epoch trigger is already pending for this subnet; wait for it to fire
        /// before triggering again.
        EpochTriggerAlreadyPending,
        /// The next automatic epoch is already imminent; a manual trigger would have
        /// no effect.
        AutoEpochAlreadyImminent,
        /// `trigger_epoch` is blocked because commit-reveal is enabled for this subnet:
        /// an out-of-band epoch would desync the CRv3 reveal window from the wall-clock
        /// Drand schedule and silently drop committed weights.
        DynamicTempoBlockedByCommitReveal,
        /// The destination coldkey rejects incoming locked alpha.
        AccountRejectsLockedAlpha,
    }
}
