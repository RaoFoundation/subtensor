use frame_support::pallet_macros::pallet_section;

/// [`pallet_section`] defining [`Error`] for the subtensor pallet (imported via [`import_section`]).
///
/// Variant **names and declaration order are frozen** (Tier B/C metadata). Edit docs only —
/// never rename, reorder, insert, or remove variants.
#[pallet_section]
mod errors {
    #[derive(PartialEq)]
    #[pallet::error]
    pub enum Error<T> {
        /// Root network (netuid 0) is missing from chain state (`NetworksAdded`).
        RootNetworkDoesNotExist,
        /// `serve_axon` / `serve_prometheus` `ip_type` is not 4 (IPv4) or 6 (IPv6).
        InvalidIpType,
        /// `serve_axon` / `serve_prometheus` `ip` is invalid for the declared `ip_type`.
        InvalidIpAddress,
        /// `serve_axon` / `serve_prometheus` `port` is zero (rejected).
        InvalidPort,
        /// Hotkey has no UID on the given netuid (`Uids`); weight/commit/UID paths.
        HotKeyNotRegisteredInSubNet,
        /// Hotkey has no on-chain account (`Owner` missing) — never registered.
        HotKeyAccountNotExists,
        /// Hotkey is not registered on any subnet (or the serving netuid for axon/prometheus).
        HotKeyNotRegisteredInNetwork,
        /// Signing coldkey does not own the target hotkey (`Owner`); stake/swap/serve/children.
        NonAssociatedColdKey,
        // StakeToWithdrawIsZero (deprecated, kept commented out for historical reference).
        /// Generic insufficient-stake failure: hotkey stake below what the action requires.
        NotEnoughStake,
        /// Unstake/move/swap/transfer requested more alpha than the coldkey–hotkey pair holds.
        NotEnoughStakeToWithdraw,
        /// Hotkey stake weight on the subnet is below `StakeThreshold` (owner hotkey exempt).
        NotEnoughStakeToSetWeights,
        /// `set_children`: parent hotkey total stake below `StakeThreshold` (owner exempt).
        NotEnoughStakeToSetChildkeys,
        /// Coldkey free balance below TAO needed for `add_stake` or registration burn.
        NotEnoughBalanceToStake,
        /// Could not withdraw the requested TAO from the coldkey (balance / ED / freeze).
        BalanceWithdrawalError,
        /// Withdrawal would leave the coldkey below existential deposit (account would vanish).
        ZeroBalanceAfterWithdrawn,
        /// Setting non-self weights without a validator permit on that subnet.
        NeuronNoValidatorPermit,
        /// Weight `uids` and `values` vectors have different lengths.
        WeightVecNotEqualSize,
        /// Weight `uids` vector contains the same UID more than once.
        DuplicateUids,
        /// At least one weight target UID is not in the subnet metagraph.
        UidVecContainInvalidOne,
        /// Weight vector has fewer elements than the subnet minimum allows.
        WeightVecLengthIsLow,
        /// Registrations this block exceed subnet hyperparameter `max_regs_per_block`.
        TooManyRegistrationsThisBlock,
        /// Hotkey already holds a UID on the target subnet (or any subnet for some swaps).
        HotKeyAlreadyRegisteredInSubNet,
        /// `swap_hotkey`: `new_hotkey` equals the current hotkey (no-op).
        NewHotKeyIsSameWithOld,
        /// Destination hotkey has root claimable/stake/history; root rate-book cannot merge safely.
        NewHotKeyNotCleanForRootSwap,
        /// PoW `block_number` is in the future or too far in the past (stale work).
        InvalidWorkBlock,
        /// PoW hash does not meet required difficulty (faucet fixed or subnet `Difficulty`).
        InvalidDifficulty,
        /// PoW seal recomputed from block/nonce/key does not match submitted `work`.
        InvalidSeal,
        /// After normalization, a weight exceeds the subnet max weight limit (self-weight exempt).
        MaxWeightExceeded,
        /// `become_delegate`: hotkey is already a delegate (`Delegates`).
        HotKeyAlreadyDelegate,
        /// Weights set again before `WeightsSetRateLimit` blocks since this neuron's last update.
        SettingWeightsTooFast,
        /// `version_key` is older than the subnet's required `WeightsVersionKey`.
        IncorrectWeightVersionKey,
        /// `serve_axon` / `serve_prometheus` before `ServingRateLimit` since last serve update.
        ServingRateLimitExceeded,
        /// Weight `uids` length exceeds the number of UIDs in the subnet.
        UidsLengthExceedUidsInSubNet, // 32
        /// Coldkey `register_network` again before `NetworkRateLimit` elapsed.
        NetworkTxRateLimitExceeded,
        /// Delegate take change before `TxDelegateTakeRateLimit` since last take tx.
        DelegateTxRateLimitExceeded,
        /// Hotkey set/swap before `TxRateLimit` since the coldkey's last such transaction.
        HotKeySetTxRateLimitExceeded,
        /// Staking extrinsic exceeded the staking rate limit for this coldkey.
        StakingRateLimitExceeded,
        /// Neuron registration is disabled on this subnet.
        SubNetRegistrationDisabled,
        /// Registration attempts this interval exceed the subnet allowed count.
        TooManyRegistrationsThisInterval,
        /// Extrinsic requires the origin to be the hotkey account itself.
        TransactorAccountShouldBeHotKey,
        /// `faucet` called on a runtime without the pow-faucet feature (real networks).
        FaucetDisabled,
        /// Signing coldkey is not `SubnetOwner` for the target netuid.
        NotSubnetOwner,
        /// Neuron registration / `set_children` is not allowed on the root subnet (use `root_register`).
        RegistrationNotPermittedOnRootSubnet,
        /// Hotkey stake too low to join the root subnet.
        StakeTooLowForRoot,
        /// New subnet would need a prune, but every candidate is still in network immunity.
        AllNetworksInImmunity,
        /// Coldkey free TAO below the hotkey-swap cost.
        NotEnoughBalanceToPaySwapHotKey,
        /// Call that only operates on root was given a non-root netuid (must be 0).
        NotRootSubnet,
        /// `set_weights` is not allowed on the root network (netuid 0).
        CanNotSetRootNetworkWeights,
        /// No UID available: `MaxAllowedUids` is 0, or subnet full and every neuron is immune.
        NoNeuronIdAvailable,
        /// Delegate `take` below `MinDelegateTake`, or take change not strictly mono vs current.
        DelegateTakeTooLow,
        /// Delegate `take` exceeds `MaxDelegateTake`.
        DelegateTakeTooHigh,
        /// Reveal found no pending non-expired weight commit for this hotkey+netuid.
        NoWeightsCommitFound,
        /// Revealed uids/values/salt/version_key hash matches none of the pending commits.
        InvalidRevealCommitHashNotMatch,
        /// Plain `set_weights` while commit-reveal is enabled; use commit/reveal instead.
        CommitRevealEnabled,
        /// Commit/reveal submitted while commit-reveal is disabled on the subnet.
        CommitRevealDisabled,
        /// Setting liquid-alpha values while `LiquidAlphaOn` is false for the subnet.
        LiquidAlphaDisabled,
        /// `alpha_high` below the liquid-alpha minimum (`u16::MAX / 40` ≈ 1638).
        AlphaHighTooLow,
        /// `alpha_low` below `u16::MAX / 40` or greater than `alpha_high`.
        AlphaLowOutOfRange,
        /// Coldkey-swap destination already has associated staking hotkeys.
        ColdKeyAlreadyAssociated,
        /// Coldkey free TAO cannot cover the coldkey-swap cost.
        NotEnoughBalanceToPaySwapColdKey,
        /// Children/parents list includes a self-loop or invalid child for this hotkey.
        InvalidChild,
        /// `set_children`: the same child hotkey appears more than once.
        DuplicateChild,
        /// `set_children`: child proportions sum overflows u64.
        ProportionOverflow,
        /// `set_children`: more than the maximum of 5 children.
        TooManyChildren,
        /// Default transaction rate limit exceeded for this coldkey.
        TxRateLimitExceeded,
        /// No pending entry in `ColdkeySwapAnnouncements` for this coldkey.
        ColdkeySwapAnnouncementNotFound,
        /// `coldkey_swap` before announcement delay (`ColdkeySwapAnnouncementDelay`) elapsed.
        ColdkeySwapTooEarly,
        /// `announce_coldkey_swap` again before `ColdkeySwapReannouncementDelay` elapsed.
        ColdkeySwapReannouncedTooEarly,
        /// `new_coldkey` hash does not match the hash in `ColdkeySwapAnnouncements`.
        AnnouncedColdkeyHashDoesNotMatch,
        /// `dispute_coldkey_swap` when the announcement is already disputed.
        ColdkeySwapAlreadyDisputed,
        /// Proposed new coldkey is already an existing hotkey (`Owner`).
        NewColdKeyIsHotkey,
        /// Childkey take outside `[MinChildkeyTake, MaxChildkeyTake]` for the subnet.
        InvalidChildkeyTake,
        /// Childkey-take change exceeded its per-hotkey rate limit.
        TxChildkeyTakeRateLimitExceeded,
        /// Coldkey or subnet identity failed validation (field length / malformed data).
        InvalidIdentity,
        /// Target subnet or sub-mechanism missing (`mechid` ≥ `MechanismCountCurrent`, etc.).
        MechanismDoesNotExist,
        /// Alpha is locked/unavailable for unstake, transfer, or re-lock at the requested amount.
        StakeUnavailable,
        /// Operation targeted a netuid that is not an existing subnet.
        SubnetNotExists,
        /// Hotkey has too many unrevealed weight commits on this subnet.
        TooManyUnrevealedCommits,
        /// Reveal after the commit's reveal window expired (`commit_reveal_period`).
        ExpiredWeightCommit,
        /// Reveal before commit epoch + reveal period (`RevealPeriodEpochs`).
        RevealTooEarly,
        /// Batch weights call: parallel input vectors have unequal lengths.
        InputLengthsUnequal,
        /// Weight commit again before per-UID `weights_rate_limit` since last commit.
        CommittingWeightsTooFast,
        /// Stake/unstake/move/swap amount is zero or below `DefaultMinStake` after fees/slippage.
        AmountTooLow,
        /// Pool cannot absorb the swap/stake (simulation failed or reserves too small).
        InsufficientLiquidity,
        /// Slippage / price impact exceeds the caller-supplied max amount.
        SlippageTooHigh,
        /// Subnet disallows the requested stake/alpha transfer.
        TransferDisallowed,
        /// Admin tried to set activity cutoff below the chain-wide minimum.
        ActivityCutoffTooLow,
        /// Extrinsic is switched off in this runtime (no active raise site in current code).
        CallDisabled,
        /// `start_call`: `FirstEmissionBlockNumber` already set; subnet already emitting.
        FirstEmissionBlockNumberAlreadySet,
        /// Legacy start-call delay error (superseded in paths by `StartCallNotReady`).
        NeedWaitingMoreBlocksToStarCall,
        /// Recycle/burn amount exceeds subnet outstanding alpha (`SubnetAlphaOut`).
        NotEnoughAlphaOutToRecycle,
        /// `recycle_alpha` / `burn_alpha` is not allowed on the root subnet.
        CannotBurnOrRecycleOnRootSubnet,
        /// EVM association signature could not recover a public key.
        UnableToRecoverPublicKey,
        /// Recovered EVM pubkey keccak hash does not match the claimed `evm_key`.
        InvalidRecoveredPublicKey,
        /// Subtoken / alpha staking path disabled for this subnet (`SubtokenEnabled`).
        SubtokenDisabled,
        /// Hotkey swap on subnet before `HotkeySwapOnSubnetInterval` since last swap on that netuid.
        HotKeySwapOnSubnetIntervalNotPassed,
        /// `keep_stake=true` refused: old hotkey still has miner collateral (would strand the bond).
        KeepStakeBlockedByCollateral,
        /// Stake move/swap where origin and destination netuid (and keys) leave nothing to change.
        SameNetuid,
        /// Coldkey free TAO below amount needed for transfer, burn/recycle, or registration lock.
        InsufficientTaoBalance,
        /// Leased-network registrant is not the crowdloan creator (beneficiary mismatch).
        InvalidLeaseBeneficiary,
        /// Leased-network `end_block` is not after the current block.
        LeaseCannotEndInThePast,
        /// After leased registration, no subnet owned by the lease coldkey was found.
        LeaseNetuidNotFound,
        /// `lease_id` has no entry in `SubnetLeases`.
        LeaseDoesNotExist,
        /// Lease is perpetual (`end_block` is `None`) and cannot be ended this way.
        LeaseHasNoEndBlock,
        /// Lease termination before stored `end_block`.
        LeaseHasNotEnded,
        /// Checked arithmetic overflow (e.g. `NextSubnetLeaseId` or crowdloan counters).
        Overflow,
        /// Lease end: handover hotkey is not owned by the lease beneficiary coldkey.
        BeneficiaryDoesNotOwnHotkey,
        /// Lease operation signed by someone other than the lease beneficiary coldkey.
        ExpectedBeneficiaryOrigin,
        /// Owner/admin hyperparameter change inside the pre-epoch admin freeze window.
        AdminActionProhibitedDuringWeightsWindow,
        /// Requested subnet symbol is not in the allowed symbol set.
        SymbolDoesNotExist,
        /// Requested subnet symbol is already assigned to another subnet.
        SymbolAlreadyInUse,
        /// `commit_reveal_version` does not match `CommitRevealWeightsVersion`.
        IncorrectCommitRevealVersion,
        /// Timelocked commit `reveal_round` older than drand `LastStoredRound` (would decrypt now).
        InvalidRevealRound,
        /// `set_reveal_period`: period above the compiled-in maximum epochs.
        RevealPeriodTooLarge,
        /// `set_reveal_period`: period below the compiled-in minimum epochs.
        RevealPeriodTooSmall,
        /// Generic out-of-range admin/sudo parameter (mechanism counts, splits, UID bounds, etc.).
        InvalidValue,
        /// Subnet limit reached and no eligible subnet can be pruned.
        SubnetLimitReached,
        /// Coldkey free balance cannot cover the dynamic subnet-creation lock cost.
        CannotAffordLockCost,
        /// `associate_evm_key` before `EvmKeyAssociateRateLimit` since last association for this UID.
        EvmKeyAssociateRateLimitExceeded,
        /// EVM address already at max associated UIDs on this subnet.
        EvmKeyAssociationLimitExceeded,
        /// Auto-stake destination already set to this same hotkey for the coldkey+netuid.
        SameAutoStakeHotkeyAlreadySet,
        /// Subnet UID map could not be cleared (inconsistent UID state).
        UidMapCouldNotBeCleared,
        /// Pruning/trimming would push immune neurons above the max immune percentage.
        TrimmingWouldExceedMaxImmunePercentage,
        /// `set_children` would make a hotkey both child and parent, or reference a missing child.
        ChildParentInconsistency,
        /// `sudo_set_num_root_claims` exceeds compile-time `MAX_NUM_ROOT_CLAIMS`.
        InvalidNumRootClaim,
        /// Root claim threshold exceeds `MAX_ROOT_CLAIM_THRESHOLD`.
        InvalidRootClaimThreshold,
        /// Root-claim subnet set empty or larger than `MAX_SUBNET_CLAIMS`.
        InvalidSubnetNumber,
        /// `MaxAllowedUids` × mechanism count would exceed 256.
        TooManyUIDsPerMechanism,
        /// Voting-power tracking is not enabled for this subnet.
        VotingPowerTrackingNotEnabled,
        /// Voting-power EMA alpha > 10^18 (must be ≤ 1.0 in fixed-point).
        InvalidVotingPowerEmaAlpha,
        /// Extrinsic removed and always fails (e.g. legacy coldkey-swap schedule path).
        Deprecated,
        /// Subnet buyback exceeded its operation rate limit.
        SubnetBuybackRateLimitExceeded,
        /// Subnet already queued in `DissolveCleanupQueue`.
        NetworkDissolveAlreadyQueued,
        /// Add-stake-and-burn exceeded its per-key rate limit.
        AddStakeBurnRateLimitExceeded,
        /// Coldkey has a pending swap announcement; most extrinsics are blocked until clear/swap.
        ColdkeySwapAnnounced,
        /// Coldkey swap is under dispute; extrinsics blocked until root resolves.
        ColdkeySwapDisputed,
        /// Clear announcement before reannouncement delay after the execution block.
        ColdkeySwapClearTooEarly,
        /// Operation temporarily disabled in runtime (hotfix switch; no active raise site now).
        DisabledTemporarily,
        /// `burned_register` price limit below current subnet registration burn (`Burn`).
        RegistrationPriceLimitExceeded,
        /// Existing conviction lock on this coldkey+netuid is bound to a different hotkey.
        LockHotkeyMismatch,
        /// Lock amount exceeds the coldkey's total alpha stake on that subnet (incl. locked mass).
        InsufficientStakeForLock,
        /// No conviction lock for this coldkey on the given subnet.
        NoExistingLock,
        /// Coldkey already has an active nonzero lock on that subnet; cannot create another.
        ActiveLockExists,
        /// Hotkey is a reserved subnet system account (`netuid_for_subnet_account`); use a user key.
        CannotUseSystemAccount,
        /// Unlock requested more alpha than is currently locked.
        UnlockAmountTooHigh,
        /// Intended guard while a dissolved netuid is still cleaning up (declared; not wired).
        WaitingForDissolvedSubnetCleanup,
        /// Supplied tempo outside the allowed range for the subnet.
        TempoOutOfBounds,
        /// Activity-cutoff factor outside the allowed per-mille range (1000–50000).
        ActivityCutoffFactorMilliOutOfBounds,
        /// `trigger_epoch`: a previous manual epoch is still pending (`PendingEpochAt`).
        EpochTriggerAlreadyPending,
        /// `trigger_epoch`: next automatic epoch is already within `AdminFreezeWindow`.
        AutoEpochAlreadyImminent,
        /// `trigger_epoch` blocked while commit-reveal is on (would desync CRv3 from Drand).
        DynamicTempoBlockedByCommitReveal,
        /// Destination coldkey `AccountFlags` reject incoming locked alpha.
        AccountRejectsLockedAlpha,
        /// Network-registration lock-id counter hit `u32::MAX` while queueing a registration.
        LockIdOverFlow,
        /// `start_call` before `NetworkRegisteredAt` + `StartCallDelay` blocks have passed.
        StartCallNotReady,
        /// Stake decrease would debit more alpha than the coldkey–hotkey pair holds on the subnet.
        InsufficientAlphaBalance,
        /// Coldkey swap could not fully migrate miner collateral (`ColdkeyMinerCollateral` nonzero).
        ColdkeyCollateralIncomplete,
        /// Coldkey already at [`crate::MAX_COLDKEY_COLLATERAL_HOTKEYS`] collateral hotkeys on subnet.
        ColdkeyCollateralPositionsFull,
        /// The coldkey has too many staking hotkeys for a single manual root claim.
        TooManyRootClaimHotkeys,
    }
}
