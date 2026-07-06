import { Enum, GetEnum, FixedSizeBinary, Binary, SS58String, FixedSizeArray, ResultPayload, TxCallData } from "polkadot-api";
type AnonymousEnum<T extends {}> = T & {
    __anonymous: true;
};
type MyTuple<T> = [T, ...T[]];
type SeparateUndefined<T> = undefined extends T ? undefined | Exclude<T, undefined> : T;
type Anonymize<T> = SeparateUndefined<T extends FixedSizeBinary<infer L> ? number extends L ? Binary : FixedSizeBinary<L> : T extends string | number | bigint | boolean | void | undefined | null | symbol | Uint8Array | Enum<any> ? T : T extends AnonymousEnum<infer V> ? Enum<V> : T extends MyTuple<any> ? {
    [K in keyof T]: T[K];
} : T extends [] ? [] : T extends FixedSizeArray<infer L, infer T> ? number extends L ? Array<T> : FixedSizeArray<L, T> : {
    [K in keyof T & string]: T[K];
}>;
export type I5sesotjlssv2d = {
    "nonce": number;
    "consumers": number;
    "providers": number;
    "sufficients": number;
    "data": Anonymize<I1q8tnt1cluu5j>;
};
export type I1q8tnt1cluu5j = {
    "free": bigint;
    "reserved": bigint;
    "frozen": bigint;
    "flags": bigint;
};
export type Iffmde3ekjedi9 = {
    "normal": Anonymize<I4q39t5hn830vp>;
    "operational": Anonymize<I4q39t5hn830vp>;
    "mandatory": Anonymize<I4q39t5hn830vp>;
};
export type I4q39t5hn830vp = {
    "ref_time": bigint;
    "proof_size": bigint;
};
export type I4mddgoa69c0a2 = Array<DigestItem>;
export type DigestItem = Enum<{
    "PreRuntime": [FixedSizeBinary<4>, Binary];
    "Consensus": [FixedSizeBinary<4>, Binary];
    "Seal": [FixedSizeBinary<4>, Binary];
    "Other": Binary;
    "RuntimeEnvironmentUpdated": undefined;
}>;
export declare const DigestItem: GetEnum<DigestItem>;
export type Icr4vaj0vrd6je = Array<{
    "phase": Phase;
    "event": Enum<{
        "System": Anonymize<Icfds8s8ncea16>;
        "Grandpa": GrandpaEvent;
        "Balances": Anonymize<Iao8h4hv7atnq3>;
        "TransactionPayment": TransactionPaymentEvent;
        "SubtensorModule": Anonymize<I2hado50khaobu>;
        "Utility": Anonymize<If1btu4npshog7>;
        "Sudo": Anonymize<I8gl492p92bk6l>;
        "Multisig": Anonymize<Ibmokdg2doop8d>;
        "Preimage": PreimageEvent;
        "Scheduler": Anonymize<Iav5po3qov3sjo>;
        "Proxy": Anonymize<I65un9enf26o4o>;
        "Registry": Anonymize<I626vh1cit09ni>;
        "Commitments": Anonymize<I5ohlg8gv4pe9g>;
        "AdminUtils": Anonymize<Ic1vmbif9o0nug>;
        "SafeMode": Anonymize<I3q8c83f5dvokp>;
        "Ethereum": Anonymize<I510u4q1qqh897>;
        "EVM": Anonymize<I9k071kk4cn1u8>;
        "BaseFee": Anonymize<I3bmatomsds8j7>;
        "Drand": Anonymize<Ibdlgbf9b95hbj>;
        "Crowdloan": Anonymize<Ifj1h07t3i0np9>;
        "Swap": Anonymize<I6qodidnq3s4e1>;
        "Contracts": Anonymize<I211sbjvh5hjqu>;
        "MevShield": Anonymize<I70thgcbmbqm91>;
    }>;
    "topics": Anonymize<Ic5m5lp1oioo8r>;
}>;
export type Phase = Enum<{
    "ApplyExtrinsic": number;
    "Finalization": undefined;
    "Initialization": undefined;
}>;
export declare const Phase: GetEnum<Phase>;
export type Icfds8s8ncea16 = AnonymousEnum<{
    /**
     * An extrinsic completed successfully.
     */
    "ExtrinsicSuccess": Anonymize<Ia82mnkmeo2rhc>;
    /**
     * An extrinsic failed.
     */
    "ExtrinsicFailed": Anonymize<I6u3ru0d29kkj0>;
    /**
     * `:code` was updated.
     */
    "CodeUpdated": undefined;
    /**
     * A new account was created.
     */
    "NewAccount": Anonymize<Icbccs0ug47ilf>;
    /**
     * An account was reaped.
     */
    "KilledAccount": Anonymize<Icbccs0ug47ilf>;
    /**
     * On on-chain remark happened.
     */
    "Remarked": Anonymize<I855j4i3kr8ko1>;
    /**
     * An upgrade was authorized.
     */
    "UpgradeAuthorized": Anonymize<Ibgl04rn6nbfm6>;
    /**
     * An invalid authorized upgrade was rejected while trying to apply it.
     */
    "RejectedInvalidAuthorizedUpgrade": Anonymize<Ibk0nulspilods>;
}>;
export type Ia82mnkmeo2rhc = {
    "dispatch_info": Anonymize<Ic9s8f85vjtncc>;
};
export type Ic9s8f85vjtncc = {
    "weight": Anonymize<I4q39t5hn830vp>;
    "class": DispatchClass;
    "pays_fee": Anonymize<Iehg04bj71rkd>;
};
export type DispatchClass = Enum<{
    "Normal": undefined;
    "Operational": undefined;
    "Mandatory": undefined;
}>;
export declare const DispatchClass: GetEnum<DispatchClass>;
export type Iehg04bj71rkd = AnonymousEnum<{
    "Yes": undefined;
    "No": undefined;
}>;
export type I6u3ru0d29kkj0 = {
    "dispatch_error": Anonymize<Ic871mj76419vm>;
    "dispatch_info": Anonymize<Ic9s8f85vjtncc>;
};
export type Ic871mj76419vm = AnonymousEnum<{
    "Other": undefined;
    "CannotLookup": undefined;
    "BadOrigin": undefined;
    "Module": Enum<{
        "System": Anonymize<I5o0s7c8q1cc9b>;
        "RandomnessCollectiveFlip": undefined;
        "Timestamp": undefined;
        "Aura": undefined;
        "Grandpa": Anonymize<I7q8i0pp1gkas6>;
        "Balances": Anonymize<Idj13i7adlomht>;
        "TransactionPayment": undefined;
        "SubtensorModule": Anonymize<Ib31febi51tc1>;
        "Utility": Anonymize<I499qmubmch1cg>;
        "Sudo": Anonymize<Iaug04qjhbli00>;
        "Multisig": Anonymize<Ia76qmhhg4jvb9>;
        "Preimage": Anonymize<I4cfhml1prt4lu>;
        "Scheduler": Anonymize<If7oa8fprnilo5>;
        "Proxy": Anonymize<I7ae37ntp06co6>;
        "Registry": Anonymize<Id6jmtdau2lr6l>;
        "Commitments": Anonymize<I8a8dfn9etteh2>;
        "AdminUtils": Anonymize<I8br6rdnlvg28h>;
        "SafeMode": Anonymize<I65gapcjsc3grr>;
        "Ethereum": Anonymize<I1mp6vnoh32l4q>;
        "EVM": Anonymize<I226s9mgj51cd2>;
        "EVMChainId": undefined;
        "BaseFee": undefined;
        "Drand": Anonymize<I8veee4gumsdel>;
        "Crowdloan": Anonymize<I1ots9pukq67tt>;
        "Swap": Anonymize<I581cn6i0ettg7>;
        "Contracts": Anonymize<I2489g9rnboo1t>;
        "MevShield": Anonymize<I4ngcc5keahtro>;
    }>;
    "ConsumerRemaining": undefined;
    "NoProviders": undefined;
    "TooManyConsumers": undefined;
    "Token": TokenError;
    "Arithmetic": ArithmeticError;
    "Transactional": TransactionalError;
    "Exhausted": undefined;
    "Corruption": undefined;
    "Unavailable": undefined;
    "RootNotAllowed": undefined;
    "Trie": Enum<{
        "InvalidStateRoot": undefined;
        "IncompleteDatabase": undefined;
        "ValueAtIncompleteKey": undefined;
        "DecoderError": undefined;
        "InvalidHash": undefined;
        "DuplicateKey": undefined;
        "ExtraneousNode": undefined;
        "ExtraneousValue": undefined;
        "ExtraneousHashReference": undefined;
        "InvalidChildReference": undefined;
        "ValueMismatch": undefined;
        "IncompleteProof": undefined;
        "RootMismatch": undefined;
        "DecodeError": undefined;
    }>;
}>;
export type I5o0s7c8q1cc9b = AnonymousEnum<{
    /**
     * The name of specification does not match between the current runtime
     * and the new runtime.
     */
    "InvalidSpecName": undefined;
    /**
     * The specification version is not allowed to decrease between the current runtime
     * and the new runtime.
     */
    "SpecVersionNeedsToIncrease": undefined;
    /**
     * Failed to extract the runtime version from the new runtime.
     *
     * Either calling `Core_version` or decoding `RuntimeVersion` failed.
     */
    "FailedToExtractRuntimeVersion": undefined;
    /**
     * Suicide called when the account has non-default composite data.
     */
    "NonDefaultComposite": undefined;
    /**
     * There is a non-zero reference count preventing the account from being purged.
     */
    "NonZeroRefCount": undefined;
    /**
     * The origin filter prevent the call to be dispatched.
     */
    "CallFiltered": undefined;
    /**
     * A multi-block migration is ongoing and prevents the current code from being replaced.
     */
    "MultiBlockMigrationsOngoing": undefined;
    /**
     * No upgrade authorized.
     */
    "NothingAuthorized": undefined;
    /**
     * The submitted code is not authorized.
     */
    "Unauthorized": undefined;
}>;
export type I7q8i0pp1gkas6 = AnonymousEnum<{
    /**
     * Attempt to signal GRANDPA pause when the authority set isn't live
     * (either paused or already pending pause).
     */
    "PauseFailed": undefined;
    /**
     * Attempt to signal GRANDPA resume when the authority set isn't paused
     * (either live or already pending resume).
     */
    "ResumeFailed": undefined;
    /**
     * Attempt to signal GRANDPA change with one already pending.
     */
    "ChangePending": undefined;
    /**
     * Cannot signal forced change so soon after last.
     */
    "TooSoon": undefined;
    /**
     * A key ownership proof provided as part of an equivocation report is invalid.
     */
    "InvalidKeyOwnershipProof": undefined;
    /**
     * An equivocation proof provided as part of an equivocation report is invalid.
     */
    "InvalidEquivocationProof": undefined;
    /**
     * A given equivocation report is valid but already previously reported.
     */
    "DuplicateOffenceReport": undefined;
}>;
export type Idj13i7adlomht = AnonymousEnum<{
    /**
     * Vesting balance too high to send value.
     */
    "VestingBalance": undefined;
    /**
     * Account liquidity restrictions prevent withdrawal.
     */
    "LiquidityRestrictions": undefined;
    /**
     * Balance too low to send value.
     */
    "InsufficientBalance": undefined;
    /**
     * Value too low to create account due to existential deposit.
     */
    "ExistentialDeposit": undefined;
    /**
     * Transfer/payment would kill account.
     */
    "Expendability": undefined;
    /**
     * A vesting schedule already exists for this account.
     */
    "ExistingVestingSchedule": undefined;
    /**
     * Beneficiary account must pre-exist.
     */
    "DeadAccount": undefined;
    /**
     * Number of named reserves exceed `MaxReserves`.
     */
    "TooManyReserves": undefined;
    /**
     * Number of holds exceed `VariantCountOf<T::RuntimeHoldReason>`.
     */
    "TooManyHolds": undefined;
    /**
     * Number of freezes exceed `MaxFreezes`.
     */
    "TooManyFreezes": undefined;
    /**
     * The issuance cannot be modified since it is already deactivated.
     */
    "IssuanceDeactivated": undefined;
    /**
     * The delta cannot be zero.
     */
    "DeltaZero": undefined;
}>;
export type Ib31febi51tc1 = AnonymousEnum<{
    /**
     * The root network does not exist.
     */
    "RootNetworkDoesNotExist": undefined;
    /**
     * The user is trying to serve an axon which is not of type 4 (IPv4) or 6 (IPv6).
     */
    "InvalidIpType": undefined;
    /**
     * An invalid IP address is passed to the serve function.
     */
    "InvalidIpAddress": undefined;
    /**
     * An invalid port is passed to the serve function.
     */
    "InvalidPort": undefined;
    /**
     * The hotkey is not registered in subnet
     */
    "HotKeyNotRegisteredInSubNet": undefined;
    /**
     * The hotkey does not exists
     */
    "HotKeyAccountNotExists": undefined;
    /**
     * The hotkey is not registered in any subnet.
     */
    "HotKeyNotRegisteredInNetwork": undefined;
    /**
     * Request to stake, unstake or subscribe is made by a coldkey that is not associated with
     * the hotkey account.
     */
    "NonAssociatedColdKey": undefined;
    /**
     * DEPRECATED: Stake amount to withdraw is zero.
     * The caller does not have enought stake to perform this action.
     */
    "NotEnoughStake": undefined;
    /**
     * The caller is requesting removing more stake than there exists in the staking account.
     * See: "[remove_stake()]".
     */
    "NotEnoughStakeToWithdraw": undefined;
    /**
     * The caller is requesting to set weights but the caller has less than minimum stake
     * required to set weights (less than WeightsMinStake).
     */
    "NotEnoughStakeToSetWeights": undefined;
    /**
     * The parent hotkey doesn't have enough own stake to set childkeys.
     */
    "NotEnoughStakeToSetChildkeys": undefined;
    /**
     * The caller is requesting adding more stake than there exists in the coldkey account.
     * See: "[add_stake()]"
     */
    "NotEnoughBalanceToStake": undefined;
    /**
     * The caller is trying to add stake, but for some reason the requested amount could not be
     * withdrawn from the coldkey account.
     */
    "BalanceWithdrawalError": undefined;
    /**
     * Unsuccessfully withdraw, balance could be zero (can not make account exist) after
     * withdrawal.
     */
    "ZeroBalanceAfterWithdrawn": undefined;
    /**
     * The caller is attempting to set non-self weights without being a permitted validator.
     */
    "NeuronNoValidatorPermit": undefined;
    /**
     * The caller is attempting to set the weight keys and values but these vectors have
     * different size.
     */
    "WeightVecNotEqualSize": undefined;
    /**
     * The caller is attempting to set weights with duplicate UIDs in the weight matrix.
     */
    "DuplicateUids": undefined;
    /**
     * The caller is attempting to set weight to at least one UID that does not exist in the
     * metagraph.
     */
    "UidVecContainInvalidOne": undefined;
    /**
     * The dispatch is attempting to set weights on chain with fewer elements than are allowed.
     */
    "WeightVecLengthIsLow": undefined;
    /**
     * Number of registrations in this block exceeds the allowed number (i.e., exceeds the
     * subnet hyperparameter "max_regs_per_block").
     */
    "TooManyRegistrationsThisBlock": undefined;
    /**
     * The caller is requesting registering a neuron which already exists in the active set.
     */
    "HotKeyAlreadyRegisteredInSubNet": undefined;
    /**
     * The new hotkey is the same as old one
     */
    "NewHotKeyIsSameWithOld": undefined;
    /**
     * The supplied PoW hash block is in the future or negative.
     */
    "InvalidWorkBlock": undefined;
    /**
     * The supplied PoW hash block does not meet the network difficulty.
     */
    "InvalidDifficulty": undefined;
    /**
     * The supplied PoW hash seal does not match the supplied work.
     */
    "InvalidSeal": undefined;
    /**
     * The dispatch is attempting to set weights on chain with weight value exceeding the
     * configured max weight limit (currently `u16::MAX`).
     */
    "MaxWeightExceeded": undefined;
    /**
     * The hotkey is attempting to become a delegate when the hotkey is already a delegate.
     */
    "HotKeyAlreadyDelegate": undefined;
    /**
     * A transactor exceeded the rate limit for setting weights.
     */
    "SettingWeightsTooFast": undefined;
    /**
     * A validator is attempting to set weights from a validator with incorrect weight version.
     */
    "IncorrectWeightVersionKey": undefined;
    /**
     * An axon or prometheus serving exceeded the rate limit for a registered neuron.
     */
    "ServingRateLimitExceeded": undefined;
    /**
     * The caller is attempting to set weights with more UIDs than allowed.
     */
    "UidsLengthExceedUidsInSubNet": undefined;
    /**
     * A transactor exceeded the rate limit for add network transaction.
     */
    "NetworkTxRateLimitExceeded": undefined;
    /**
     * A transactor exceeded the rate limit for delegate transaction.
     */
    "DelegateTxRateLimitExceeded": undefined;
    /**
     * A transactor exceeded the rate limit for setting or swapping hotkey.
     */
    "HotKeySetTxRateLimitExceeded": undefined;
    /**
     * A transactor exceeded the rate limit for staking.
     */
    "StakingRateLimitExceeded": undefined;
    /**
     * Registration is disabled.
     */
    "SubNetRegistrationDisabled": undefined;
    /**
     * The number of registration attempts exceeded the allowed number in the interval.
     */
    "TooManyRegistrationsThisInterval": undefined;
    /**
     * The hotkey is required to be the origin.
     */
    "TransactorAccountShouldBeHotKey": undefined;
    /**
     * Faucet is disabled.
     */
    "FaucetDisabled": undefined;
    /**
     * Not a subnet owner.
     */
    "NotSubnetOwner": undefined;
    /**
     * Operation is not permitted on the root subnet.
     */
    "RegistrationNotPermittedOnRootSubnet": undefined;
    /**
     * A hotkey with too little stake is attempting to join the root subnet.
     */
    "StakeTooLowForRoot": undefined;
    /**
     * All subnets are in the immunity period.
     */
    "AllNetworksInImmunity": undefined;
    /**
     * Not enough balance to pay swapping hotkey.
     */
    "NotEnoughBalanceToPaySwapHotKey": undefined;
    /**
     * Netuid does not match for setting root network weights.
     */
    "NotRootSubnet": undefined;
    /**
     * Can not set weights for the root network.
     */
    "CanNotSetRootNetworkWeights": undefined;
    /**
     * No neuron ID is available.
     */
    "NoNeuronIdAvailable": undefined;
    /**
     * Delegate take is too low.
     */
    "DelegateTakeTooLow": undefined;
    /**
     * Delegate take is too high.
     */
    "DelegateTakeTooHigh": undefined;
    /**
     * No commit found for the provided hotkey+netuid combination when attempting to reveal the
     * weights.
     */
    "NoWeightsCommitFound": undefined;
    /**
     * Committed hash does not equal the hashed reveal data.
     */
    "InvalidRevealCommitHashNotMatch": undefined;
    /**
     * Attempting to call set_weights when commit/reveal is enabled
     */
    "CommitRevealEnabled": undefined;
    /**
     * Attemtping to commit/reveal weights when disabled.
     */
    "CommitRevealDisabled": undefined;
    /**
     * Attempting to set alpha high/low while disabled
     */
    "LiquidAlphaDisabled": undefined;
    /**
     * Alpha high is too low: alpha_high > 0.8
     */
    "AlphaHighTooLow": undefined;
    /**
     * Alpha low is out of range: alpha_low > 0 && alpha_low < 0.8
     */
    "AlphaLowOutOfRange": undefined;
    /**
     * The coldkey has already been swapped
     */
    "ColdKeyAlreadyAssociated": undefined;
    /**
     * The coldkey balance is not enough to pay for the swap
     */
    "NotEnoughBalanceToPaySwapColdKey": undefined;
    /**
     * Attempting to set an invalid child for a hotkey on a network.
     */
    "InvalidChild": undefined;
    /**
     * Duplicate child when setting children.
     */
    "DuplicateChild": undefined;
    /**
     * Proportion overflow when setting children.
     */
    "ProportionOverflow": undefined;
    /**
     * Too many children MAX 5.
     */
    "TooManyChildren": undefined;
    /**
     * Default transaction rate limit exceeded.
     */
    "TxRateLimitExceeded": undefined;
    /**
     * Coldkey swap announcement not found
     */
    "ColdkeySwapAnnouncementNotFound": undefined;
    /**
     * Coldkey swap too early.
     */
    "ColdkeySwapTooEarly": undefined;
    /**
     * Coldkey swap reannounced too early.
     */
    "ColdkeySwapReannouncedTooEarly": undefined;
    /**
     * The announced coldkey hash does not match the new coldkey hash.
     */
    "AnnouncedColdkeyHashDoesNotMatch": undefined;
    /**
     * Coldkey swap already disputed
     */
    "ColdkeySwapAlreadyDisputed": undefined;
    /**
     * New coldkey is hotkey
     */
    "NewColdKeyIsHotkey": undefined;
    /**
     * Childkey take is invalid.
     */
    "InvalidChildkeyTake": undefined;
    /**
     * Childkey take rate limit exceeded.
     */
    "TxChildkeyTakeRateLimitExceeded": undefined;
    /**
     * Invalid identity.
     */
    "InvalidIdentity": undefined;
    /**
     * Subnet mechanism does not exist.
     */
    "MechanismDoesNotExist": undefined;
    /**
     * Trying to unstake your lock amount.
     */
    "CannotUnstakeLock": undefined;
    /**
     * Trying to perform action on non-existent subnet.
     */
    "SubnetNotExists": undefined;
    /**
     * Maximum commit limit reached
     */
    "TooManyUnrevealedCommits": undefined;
    /**
     * Attempted to reveal weights that are expired.
     */
    "ExpiredWeightCommit": undefined;
    /**
     * Attempted to reveal weights too early.
     */
    "RevealTooEarly": undefined;
    /**
     * Attempted to batch reveal weights with mismatched vector input lenghts.
     */
    "InputLengthsUnequal": undefined;
    /**
     * A transactor exceeded the rate limit for setting weights.
     */
    "CommittingWeightsTooFast": undefined;
    /**
     * Stake amount is too low.
     */
    "AmountTooLow": undefined;
    /**
     * Not enough liquidity.
     */
    "InsufficientLiquidity": undefined;
    /**
     * Slippage is too high for the transaction.
     */
    "SlippageTooHigh": undefined;
    /**
     * Subnet disallows transfer.
     */
    "TransferDisallowed": undefined;
    /**
     * Activity cutoff is being set too low.
     */
    "ActivityCutoffTooLow": undefined;
    /**
     * Call is disabled
     */
    "CallDisabled": undefined;
    /**
     * FirstEmissionBlockNumber is already set.
     */
    "FirstEmissionBlockNumberAlreadySet": undefined;
    /**
     * need wait for more blocks to accept the start call extrinsic.
     */
    "NeedWaitingMoreBlocksToStarCall": undefined;
    /**
     * Not enough AlphaOut on the subnet to recycle
     */
    "NotEnoughAlphaOutToRecycle": undefined;
    /**
     * Cannot burn or recycle TAO from root subnet
     */
    "CannotBurnOrRecycleOnRootSubnet": undefined;
    /**
     * Public key cannot be recovered.
     */
    "UnableToRecoverPublicKey": undefined;
    /**
     * Recovered public key is invalid.
     */
    "InvalidRecoveredPublicKey": undefined;
    /**
     * SubToken disabled now
     */
    "SubtokenDisabled": undefined;
    /**
     * Too frequent hotkey swap on subnet
     */
    "HotKeySwapOnSubnetIntervalNotPassed": undefined;
    /**
     * Zero max stake amount
     */
    "ZeroMaxStakeAmount": undefined;
    /**
     * Invalid netuid duplication
     */
    "SameNetuid": undefined;
    /**
     * The caller does not have enough balance for the operation.
     */
    "InsufficientBalance": undefined;
    /**
     * Too frequent staking operations
     */
    "StakingOperationRateLimitExceeded": undefined;
    /**
     * Invalid lease beneficiary to register the leased network.
     */
    "InvalidLeaseBeneficiary": undefined;
    /**
     * Lease cannot end in the past.
     */
    "LeaseCannotEndInThePast": undefined;
    /**
     * Couldn't find the lease netuid.
     */
    "LeaseNetuidNotFound": undefined;
    /**
     * Lease does not exist.
     */
    "LeaseDoesNotExist": undefined;
    /**
     * Lease has no end block.
     */
    "LeaseHasNoEndBlock": undefined;
    /**
     * Lease has not ended.
     */
    "LeaseHasNotEnded": undefined;
    /**
     * An overflow occurred.
     */
    "Overflow": undefined;
    /**
     * Beneficiary does not own hotkey.
     */
    "BeneficiaryDoesNotOwnHotkey": undefined;
    /**
     * Expected beneficiary origin.
     */
    "ExpectedBeneficiaryOrigin": undefined;
    /**
     * Admin operation is prohibited during the protected weights window
     */
    "AdminActionProhibitedDuringWeightsWindow": undefined;
    /**
     * Symbol does not exist.
     */
    "SymbolDoesNotExist": undefined;
    /**
     * Symbol already in use.
     */
    "SymbolAlreadyInUse": undefined;
    /**
     * Incorrect commit-reveal version.
     */
    "IncorrectCommitRevealVersion": undefined;
    /**
     * Reveal period is too large.
     */
    "RevealPeriodTooLarge": undefined;
    /**
     * Reveal period is too small.
     */
    "RevealPeriodTooSmall": undefined;
    /**
     * Generic error for out-of-range parameter value
     */
    "InvalidValue": undefined;
    /**
     * Subnet limit reached & there is no eligible subnet to prune
     */
    "SubnetLimitReached": undefined;
    /**
     * Insufficient funds to meet the subnet lock cost
     */
    "CannotAffordLockCost": undefined;
    /**
     * exceeded the rate limit for associating an EVM key.
     */
    "EvmKeyAssociateRateLimitExceeded": undefined;
    /**
     * Same auto stake hotkey already set
     */
    "SameAutoStakeHotkeyAlreadySet": undefined;
    /**
     * The UID map for the subnet could not be cleared
     */
    "UidMapCouldNotBeCleared": undefined;
    /**
     * Trimming would exceed the max immune neurons percentage
     */
    "TrimmingWouldExceedMaxImmunePercentage": undefined;
    /**
     * Violating the rules of Childkey-Parentkey consistency
     */
    "ChildParentInconsistency": undefined;
    /**
     * Invalid number of root claims
     */
    "InvalidNumRootClaim": undefined;
    /**
     * Invalid value of root claim threshold
     */
    "InvalidRootClaimThreshold": undefined;
    /**
     * Exceeded subnet limit number or zero.
     */
    "InvalidSubnetNumber": undefined;
    /**
     * The maximum allowed UIDs times mechanism count should not exceed 256.
     */
    "TooManyUIDsPerMechanism": undefined;
    /**
     * Voting power tracking is not enabled for this subnet.
     */
    "VotingPowerTrackingNotEnabled": undefined;
    /**
     * Invalid voting power EMA alpha value (must be <= 10^18).
     */
    "InvalidVotingPowerEmaAlpha": undefined;
    /**
     * Unintended precision loss when unstaking alpha
     */
    "PrecisionLoss": undefined;
    /**
     * Deprecated call.
     */
    "Deprecated": undefined;
    /**
     * "Add stake and burn" exceeded the operation rate limit
     */
    "AddStakeBurnRateLimitExceeded": undefined;
}>;
export type I499qmubmch1cg = AnonymousEnum<{
    /**
     * Too many calls batched.
     */
    "TooManyCalls": undefined;
    /**
     * Bad input data for derived account ID
     */
    "InvalidDerivedAccount": undefined;
}>;
export type Iaug04qjhbli00 = AnonymousEnum<{
    /**
     * Sender must be the Sudo account.
     */
    "RequireSudo": undefined;
}>;
export type Ia76qmhhg4jvb9 = AnonymousEnum<{
    /**
     * Threshold must be 2 or greater.
     */
    "MinimumThreshold": undefined;
    /**
     * Call is already approved by this signatory.
     */
    "AlreadyApproved": undefined;
    /**
     * Call doesn't need any (more) approvals.
     */
    "NoApprovalsNeeded": undefined;
    /**
     * There are too few signatories in the list.
     */
    "TooFewSignatories": undefined;
    /**
     * There are too many signatories in the list.
     */
    "TooManySignatories": undefined;
    /**
     * The signatories were provided out of order; they should be ordered.
     */
    "SignatoriesOutOfOrder": undefined;
    /**
     * The sender was contained in the other signatories; it shouldn't be.
     */
    "SenderInSignatories": undefined;
    /**
     * Multisig operation not found in storage.
     */
    "NotFound": undefined;
    /**
     * Only the account that originally created the multisig is able to cancel it or update
     * its deposits.
     */
    "NotOwner": undefined;
    /**
     * No timepoint was given, yet the multisig operation is already underway.
     */
    "NoTimepoint": undefined;
    /**
     * A different timepoint was given to the multisig operation that is underway.
     */
    "WrongTimepoint": undefined;
    /**
     * A timepoint was given, yet no multisig operation is underway.
     */
    "UnexpectedTimepoint": undefined;
    /**
     * The maximum weight information provided was too low.
     */
    "MaxWeightTooLow": undefined;
    /**
     * The data to be stored is already stored.
     */
    "AlreadyStored": undefined;
}>;
export type I4cfhml1prt4lu = AnonymousEnum<{
    /**
     * Preimage is too large to store on-chain.
     */
    "TooBig": undefined;
    /**
     * Preimage has already been noted on-chain.
     */
    "AlreadyNoted": undefined;
    /**
     * The user is not authorized to perform this action.
     */
    "NotAuthorized": undefined;
    /**
     * The preimage cannot be removed since it has not yet been noted.
     */
    "NotNoted": undefined;
    /**
     * A preimage may not be removed when there are outstanding requests.
     */
    "Requested": undefined;
    /**
     * The preimage request cannot be removed since no outstanding requests exist.
     */
    "NotRequested": undefined;
    /**
     * More than `MAX_HASH_UPGRADE_BULK_COUNT` hashes were requested to be upgraded at once.
     */
    "TooMany": undefined;
    /**
     * Too few hashes were requested to be upgraded (i.e. zero).
     */
    "TooFew": undefined;
}>;
export type If7oa8fprnilo5 = AnonymousEnum<{
    /**
     * Failed to schedule a call
     */
    "FailedToSchedule": undefined;
    /**
     * Cannot find the scheduled call.
     */
    "NotFound": undefined;
    /**
     * Given target block number is in the past.
     */
    "TargetBlockNumberInPast": undefined;
    /**
     * Reschedule failed because it does not change scheduled time.
     */
    "RescheduleNoChange": undefined;
    /**
     * Attempt to use a non-named function on a named task.
     */
    "Named": undefined;
}>;
export type I7ae37ntp06co6 = AnonymousEnum<{
    /**
     * There are too many proxies registered or too many announcements pending.
     */
    "TooMany": undefined;
    /**
     * Proxy registration not found.
     */
    "NotFound": undefined;
    /**
     * Sender is not a proxy of the account to be proxied.
     */
    "NotProxy": undefined;
    /**
     * A call which is incompatible with the proxy type's filter was attempted.
     */
    "Unproxyable": undefined;
    /**
     * Account is already a proxy.
     */
    "Duplicate": undefined;
    /**
     * Call may not be made by proxy because it may escalate its privileges.
     */
    "NoPermission": undefined;
    /**
     * Announcement, if made at all, was made too recently.
     */
    "Unannounced": undefined;
    /**
     * Cannot add self as proxy.
     */
    "NoSelfProxy": undefined;
    /**
     * Invariant violated: deposit recomputation returned None after updating announcements.
     */
    "AnnouncementDepositInvariantViolated": undefined;
    /**
     * Failed to derive a valid account id from the provided entropy.
     */
    "InvalidDerivedAccountId": undefined;
}>;
export type Id6jmtdau2lr6l = AnonymousEnum<{
    /**
     * Account attempted to register an identity but does not meet the requirements.
     */
    "CannotRegister": undefined;
    /**
     * Account passed too many additional fields to their identity
     */
    "TooManyFieldsInIdentityInfo": undefined;
    /**
     * Account doesn't have a registered identity
     */
    "NotRegistered": undefined;
}>;
export type I8a8dfn9etteh2 = AnonymousEnum<{
    /**
     * Account passed too many additional fields to their commitment
     */
    "TooManyFieldsInCommitmentInfo": undefined;
    /**
     * Account is not allowed to make commitments to the chain
     */
    "AccountNotAllowedCommit": undefined;
    /**
     * Space Limit Exceeded for the current interval
     */
    "SpaceLimitExceeded": undefined;
    /**
     * Indicates that unreserve returned a leftover, which is unexpected.
     */
    "UnexpectedUnreserveLeftover": undefined;
}>;
export type I8br6rdnlvg28h = AnonymousEnum<{
    /**
     * The subnet does not exist, check the netuid parameter
     */
    "SubnetDoesNotExist": undefined;
    /**
     * The maximum number of subnet validators must be less than the maximum number of allowed UIDs in the subnet.
     */
    "MaxValidatorsLargerThanMaxUIds": undefined;
    /**
     * The maximum number of subnet validators must be more than the current number of UIDs already in the subnet.
     */
    "MaxAllowedUIdsLessThanCurrentUIds": undefined;
    /**
     * The maximum value for bonds moving average is reached
     */
    "BondsMovingAverageMaxReached": undefined;
    /**
     * Only root can set negative sigmoid steepness values
     */
    "NegativeSigmoidSteepness": undefined;
    /**
     * Value not in allowed bounds.
     */
    "ValueNotInBounds": undefined;
    /**
     * The minimum allowed UIDs must be less than the current number of UIDs in the subnet.
     */
    "MinAllowedUidsGreaterThanCurrentUids": undefined;
    /**
     * The minimum allowed UIDs must be less than the maximum allowed UIDs.
     */
    "MinAllowedUidsGreaterThanMaxAllowedUids": undefined;
    /**
     * The maximum allowed UIDs must be greater than the minimum allowed UIDs.
     */
    "MaxAllowedUidsLessThanMinAllowedUids": undefined;
    /**
     * The maximum allowed UIDs must be less than the default maximum allowed UIDs.
     */
    "MaxAllowedUidsGreaterThanDefaultMaxAllowedUids": undefined;
    /**
     * Bad parameter value
     */
    "InvalidValue": undefined;
}>;
export type I65gapcjsc3grr = AnonymousEnum<{
    /**
     * The safe-mode is (already or still) entered.
     */
    "Entered": undefined;
    /**
     * The safe-mode is (already or still) exited.
     */
    "Exited": undefined;
    /**
     * This functionality of the pallet is disabled by the configuration.
     */
    "NotConfigured": undefined;
    /**
     * There is no balance reserved.
     */
    "NoDeposit": undefined;
    /**
     * The account already has a deposit reserved and can therefore not enter or extend again.
     */
    "AlreadyDeposited": undefined;
    /**
     * This deposit cannot be released yet.
     */
    "CannotReleaseYet": undefined;
    /**
     * An error from the underlying `Currency`.
     */
    "CurrencyError": undefined;
}>;
export type I1mp6vnoh32l4q = AnonymousEnum<{
    /**
     * Signature is invalid.
     */
    "InvalidSignature": undefined;
    /**
     * Pre-log is present, therefore transact is not allowed.
     */
    "PreLogExists": undefined;
}>;
export type I226s9mgj51cd2 = AnonymousEnum<{
    /**
     * Not enough balance to perform action
     */
    "BalanceLow": undefined;
    /**
     * Calculating total fee overflowed
     */
    "FeeOverflow": undefined;
    /**
     * Calculating total payment overflowed
     */
    "PaymentOverflow": undefined;
    /**
     * Withdraw fee failed
     */
    "WithdrawFailed": undefined;
    /**
     * Gas price is too low.
     */
    "GasPriceTooLow": undefined;
    /**
     * Nonce is invalid
     */
    "InvalidNonce": undefined;
    /**
     * Gas limit is too low.
     */
    "GasLimitTooLow": undefined;
    /**
     * Gas limit is too high.
     */
    "GasLimitTooHigh": undefined;
    /**
     * The chain id is invalid.
     */
    "InvalidChainId": undefined;
    /**
     * the signature is invalid.
     */
    "InvalidSignature": undefined;
    /**
     * EVM reentrancy
     */
    "Reentrancy": undefined;
    /**
     * EIP-3607,
     */
    "TransactionMustComeFromEOA": undefined;
    /**
     * Undefined error.
     */
    "Undefined": undefined;
    /**
     * Origin is not allowed to perform the operation.
     */
    "NotAllowed": undefined;
    /**
     * Address not allowed to deploy contracts either via CREATE or CALL(CREATE).
     */
    "CreateOriginNotAllowed": undefined;
}>;
export type I8veee4gumsdel = AnonymousEnum<{
    /**
     * The value retrieved was `None` as no value was previously set.
     */
    "NoneValue": undefined;
    /**
     * There was an attempt to increment the value in storage over `u32::MAX`.
     */
    "StorageOverflow": undefined;
    /**
     * failed to connect to the
     */
    "DrandConnectionFailure": undefined;
    /**
     * the pulse is invalid
     */
    "UnverifiedPulse": undefined;
    /**
     * the round number did not increment
     */
    "InvalidRoundNumber": undefined;
    /**
     * the pulse could not be verified
     */
    "PulseVerificationError": undefined;
}>;
export type I1ots9pukq67tt = AnonymousEnum<{
    /**
     * The crowdloan initial deposit is too low.
     */
    "DepositTooLow": undefined;
    /**
     * The crowdloan cap is too low.
     */
    "CapTooLow": undefined;
    /**
     * The minimum contribution is too low.
     */
    "MinimumContributionTooLow": undefined;
    /**
     * The crowdloan cannot end in the past.
     */
    "CannotEndInPast": undefined;
    /**
     * The crowdloan block duration is too short.
     */
    "BlockDurationTooShort": undefined;
    /**
     * The block duration is too long.
     */
    "BlockDurationTooLong": undefined;
    /**
     * The account does not have enough balance to pay for the initial deposit/contribution.
     */
    "InsufficientBalance": undefined;
    /**
     * An overflow occurred.
     */
    "Overflow": undefined;
    /**
     * The crowdloan id is invalid.
     */
    "InvalidCrowdloanId": undefined;
    /**
     * The crowdloan cap has been fully raised.
     */
    "CapRaised": undefined;
    /**
     * The contribution period has ended.
     */
    "ContributionPeriodEnded": undefined;
    /**
     * The contribution is too low.
     */
    "ContributionTooLow": undefined;
    /**
     * The origin of this call is invalid.
     */
    "InvalidOrigin": undefined;
    /**
     * The crowdloan has already been finalized.
     */
    "AlreadyFinalized": undefined;
    /**
     * The crowdloan contribution period has not ended yet.
     */
    "ContributionPeriodNotEnded": undefined;
    /**
     * The contributor has no contribution for this crowdloan.
     */
    "NoContribution": undefined;
    /**
     * The crowdloan cap has not been raised.
     */
    "CapNotRaised": undefined;
    /**
     * An underflow occurred.
     */
    "Underflow": undefined;
    /**
     * Call to dispatch was not found in the preimage storage.
     */
    "CallUnavailable": undefined;
    /**
     * The crowdloan is not ready to be dissolved, it still has contributions.
     */
    "NotReadyToDissolve": undefined;
    /**
     * The deposit cannot be withdrawn from the crowdloan.
     */
    "DepositCannotBeWithdrawn": undefined;
    /**
     * The maximum number of contributors has been reached.
     */
    "MaxContributorsReached": undefined;
}>;
export type I581cn6i0ettg7 = AnonymousEnum<{
    /**
     * The fee rate is too high
     */
    "FeeRateTooHigh": undefined;
    /**
     * The provided amount is insufficient for the swap.
     */
    "InsufficientInputAmount": undefined;
    /**
     * The provided liquidity is insufficient for the operation.
     */
    "InsufficientLiquidity": undefined;
    /**
     * The operation would exceed the price limit.
     */
    "PriceLimitExceeded": undefined;
    /**
     * The caller does not have enough balance for the operation.
     */
    "InsufficientBalance": undefined;
    /**
     * Attempted to remove liquidity that does not exist.
     */
    "LiquidityNotFound": undefined;
    /**
     * The provided tick range is invalid.
     */
    "InvalidTickRange": undefined;
    /**
     * Maximum user positions exceeded
     */
    "MaxPositionsExceeded": undefined;
    /**
     * Too many swap steps
     */
    "TooManySwapSteps": undefined;
    /**
     * Provided liquidity parameter is invalid (likely too small)
     */
    "InvalidLiquidityValue": undefined;
    /**
     * Reserves too low for operation.
     */
    "ReservesTooLow": undefined;
    /**
     * The subnet does not exist.
     */
    "MechanismDoesNotExist": undefined;
    /**
     * User liquidity operations are disabled for this subnet
     */
    "UserLiquidityDisabled": undefined;
    /**
     * The subnet does not have subtoken enabled
     */
    "SubtokenDisabled": undefined;
}>;
export type I2489g9rnboo1t = AnonymousEnum<{
    /**
     * Invalid schedule supplied, e.g. with zero weight of a basic operation.
     */
    "InvalidSchedule": undefined;
    /**
     * Invalid combination of flags supplied to `seal_call` or `seal_delegate_call`.
     */
    "InvalidCallFlags": undefined;
    /**
     * The executed contract exhausted its gas limit.
     */
    "OutOfGas": undefined;
    /**
     * The output buffer supplied to a contract API call was too small.
     */
    "OutputBufferTooSmall": undefined;
    /**
     * Performing the requested transfer failed. Probably because there isn't enough
     * free balance in the sender's account.
     */
    "TransferFailed": undefined;
    /**
     * Performing a call was denied because the calling depth reached the limit
     * of what is specified in the schedule.
     */
    "MaxCallDepthReached": undefined;
    /**
     * No contract was found at the specified address.
     */
    "ContractNotFound": undefined;
    /**
     * The code supplied to `instantiate_with_code` exceeds the limit specified in the
     * current schedule.
     */
    "CodeTooLarge": undefined;
    /**
     * No code could be found at the supplied code hash.
     */
    "CodeNotFound": undefined;
    /**
     * No code info could be found at the supplied code hash.
     */
    "CodeInfoNotFound": undefined;
    /**
     * A buffer outside of sandbox memory was passed to a contract API function.
     */
    "OutOfBounds": undefined;
    /**
     * Input passed to a contract API function failed to decode as expected type.
     */
    "DecodingFailed": undefined;
    /**
     * Contract trapped during execution.
     */
    "ContractTrapped": undefined;
    /**
     * The size defined in `T::MaxValueSize` was exceeded.
     */
    "ValueTooLarge": undefined;
    /**
     * Termination of a contract is not allowed while the contract is already
     * on the call stack. Can be triggered by `seal_terminate`.
     */
    "TerminatedWhileReentrant": undefined;
    /**
     * `seal_call` forwarded this contracts input. It therefore is no longer available.
     */
    "InputForwarded": undefined;
    /**
     * The subject passed to `seal_random` exceeds the limit.
     */
    "RandomSubjectTooLong": undefined;
    /**
     * The amount of topics passed to `seal_deposit_events` exceeds the limit.
     */
    "TooManyTopics": undefined;
    /**
     * The chain does not provide a chain extension. Calling the chain extension results
     * in this error. Note that this usually  shouldn't happen as deploying such contracts
     * is rejected.
     */
    "NoChainExtension": undefined;
    /**
     * Failed to decode the XCM program.
     */
    "XCMDecodeFailed": undefined;
    /**
     * A contract with the same AccountId already exists.
     */
    "DuplicateContract": undefined;
    /**
     * A contract self destructed in its constructor.
     *
     * This can be triggered by a call to `seal_terminate`.
     */
    "TerminatedInConstructor": undefined;
    /**
     * A call tried to invoke a contract that is flagged as non-reentrant.
     * The only other cause is that a call from a contract into the runtime tried to call back
     * into `pallet-contracts`. This would make the whole pallet reentrant with regard to
     * contract code execution which is not supported.
     */
    "ReentranceDenied": undefined;
    /**
     * A contract attempted to invoke a state modifying API while being in read-only mode.
     */
    "StateChangeDenied": undefined;
    /**
     * Origin doesn't have enough balance to pay the required storage deposits.
     */
    "StorageDepositNotEnoughFunds": undefined;
    /**
     * More storage was created than allowed by the storage deposit limit.
     */
    "StorageDepositLimitExhausted": undefined;
    /**
     * Code removal was denied because the code is still in use by at least one contract.
     */
    "CodeInUse": undefined;
    /**
     * The contract ran to completion but decided to revert its storage changes.
     * Please note that this error is only returned from extrinsics. When called directly
     * or via RPC an `Ok` will be returned. In this case the caller needs to inspect the flags
     * to determine whether a reversion has taken place.
     */
    "ContractReverted": undefined;
    /**
     * The contract's code was found to be invalid during validation.
     *
     * The most likely cause of this is that an API was used which is not supported by the
     * node. This happens if an older node is used with a new version of ink!. Try updating
     * your node to the newest available version.
     *
     * A more detailed error can be found on the node console if debug messages are enabled
     * by supplying `-lruntime::contracts=debug`.
     */
    "CodeRejected": undefined;
    /**
     * An indeterministic code was used in a context where this is not permitted.
     */
    "Indeterministic": undefined;
    /**
     * A pending migration needs to complete before the extrinsic can be called.
     */
    "MigrationInProgress": undefined;
    /**
     * Migrate dispatch call was attempted but no migration was performed.
     */
    "NoMigrationPerformed": undefined;
    /**
     * The contract has reached its maximum number of delegate dependencies.
     */
    "MaxDelegateDependenciesReached": undefined;
    /**
     * The dependency was not found in the contract's delegate dependencies.
     */
    "DelegateDependencyNotFound": undefined;
    /**
     * The contract already depends on the given delegate dependency.
     */
    "DelegateDependencyAlreadyExists": undefined;
    /**
     * Can not add a delegate dependency to the code hash of the contract itself.
     */
    "CannotAddSelfAsDelegateDependency": undefined;
    /**
     * Can not add more data to transient storage.
     */
    "OutOfTransientStorage": undefined;
}>;
export type I4ngcc5keahtro = AnonymousEnum<{
    /**
     * A submission with the same id already exists in `Submissions`.
     */
    "SubmissionAlreadyExists": undefined;
    /**
     * The referenced submission id does not exist in `Submissions`.
     */
    "MissingSubmission": undefined;
    /**
     * The recomputed commitment does not match the stored commitment.
     */
    "CommitmentMismatch": undefined;
    /**
     * The provided signature over the payload is invalid.
     */
    "SignatureInvalid": undefined;
    /**
     * The announced ML‑KEM public key length is invalid.
     */
    "BadPublicKeyLen": undefined;
    /**
     * The MEV‑Shield key epoch for this submission has expired and is no longer accepted.
     */
    "KeyExpired": undefined;
    /**
     * The provided `key_hash` does not match the expected epoch key hash.
     */
    "KeyHashMismatch": undefined;
}>;
export type TokenError = Enum<{
    "FundsUnavailable": undefined;
    "OnlyProvider": undefined;
    "BelowMinimum": undefined;
    "CannotCreate": undefined;
    "UnknownAsset": undefined;
    "Frozen": undefined;
    "Unsupported": undefined;
    "CannotCreateHold": undefined;
    "NotExpendable": undefined;
    "Blocked": undefined;
}>;
export declare const TokenError: GetEnum<TokenError>;
export type ArithmeticError = Enum<{
    "Underflow": undefined;
    "Overflow": undefined;
    "DivisionByZero": undefined;
}>;
export declare const ArithmeticError: GetEnum<ArithmeticError>;
export type TransactionalError = Enum<{
    "LimitReached": undefined;
    "NoLayer": undefined;
}>;
export declare const TransactionalError: GetEnum<TransactionalError>;
export type Icbccs0ug47ilf = {
    "account": SS58String;
};
export type I855j4i3kr8ko1 = {
    "sender": SS58String;
    "hash": FixedSizeBinary<32>;
};
export type Ibgl04rn6nbfm6 = {
    "code_hash": FixedSizeBinary<32>;
    "check_version": boolean;
};
export type Ibk0nulspilods = {
    "code_hash": FixedSizeBinary<32>;
    "error": Anonymize<Ic871mj76419vm>;
};
export type GrandpaEvent = Enum<{
    /**
     * New authority set has been applied.
     */
    "NewAuthorities": Anonymize<I5768ac424h061>;
    /**
     * Current authority set has been paused.
     */
    "Paused": undefined;
    /**
     * Current authority set has been resumed.
     */
    "Resumed": undefined;
}>;
export declare const GrandpaEvent: GetEnum<GrandpaEvent>;
export type I5768ac424h061 = {
    "authority_set": Anonymize<I3geksg000c171>;
};
export type I3geksg000c171 = Array<[FixedSizeBinary<32>, bigint]>;
export type Iao8h4hv7atnq3 = AnonymousEnum<{
    /**
     * An account was created with some free balance.
     */
    "Endowed": Anonymize<Icv68aq8841478>;
    /**
     * An account was removed whose balance was non-zero but below ExistentialDeposit,
     * resulting in an outright loss.
     */
    "DustLost": Anonymize<Ic262ibdoec56a>;
    /**
     * Transfer succeeded.
     */
    "Transfer": Anonymize<Iflcfm9b6nlmdd>;
    /**
     * A balance was set by root.
     */
    "BalanceSet": Anonymize<Ijrsf4mnp3eka>;
    /**
     * Some balance was reserved (moved from free to reserved).
     */
    "Reserved": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some balance was unreserved (moved from reserved to free).
     */
    "Unreserved": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some balance was moved from the reserve of the first account to the second account.
     * Final argument indicates the destination balance type.
     */
    "ReserveRepatriated": Anonymize<I8tjvj9uq4b7hi>;
    /**
     * Some amount was deposited (e.g. for transaction fees).
     */
    "Deposit": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some amount was withdrawn from the account (e.g. for transaction fees).
     */
    "Withdraw": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some amount was removed from the account (e.g. for misbehavior).
     */
    "Slashed": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some amount was minted into an account.
     */
    "Minted": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some amount was burned from an account.
     */
    "Burned": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some amount was suspended from an account (it can be restored later).
     */
    "Suspended": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some amount was restored into an account.
     */
    "Restored": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * An account was upgraded.
     */
    "Upgraded": Anonymize<I4cbvqmqadhrea>;
    /**
     * Total issuance was increased by `amount`, creating a credit to be balanced.
     */
    "Issued": Anonymize<I3qt1hgg4djhgb>;
    /**
     * Total issuance was decreased by `amount`, creating a debt to be balanced.
     */
    "Rescinded": Anonymize<I3qt1hgg4djhgb>;
    /**
     * Some balance was locked.
     */
    "Locked": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some balance was unlocked.
     */
    "Unlocked": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some balance was frozen.
     */
    "Frozen": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * Some balance was thawed.
     */
    "Thawed": Anonymize<Id5fm4p8lj5qgi>;
    /**
     * The `TotalIssuance` was forcefully changed.
     */
    "TotalIssuanceForced": Anonymize<I4fooe9dun9o0t>;
}>;
export type Icv68aq8841478 = {
    "account": SS58String;
    "free_balance": bigint;
};
export type Ic262ibdoec56a = {
    "account": SS58String;
    "amount": bigint;
};
export type Iflcfm9b6nlmdd = {
    "from": SS58String;
    "to": SS58String;
    "amount": bigint;
};
export type Ijrsf4mnp3eka = {
    "who": SS58String;
    "free": bigint;
};
export type Id5fm4p8lj5qgi = {
    "who": SS58String;
    "amount": bigint;
};
export type I8tjvj9uq4b7hi = {
    "from": SS58String;
    "to": SS58String;
    "amount": bigint;
    "destination_status": BalanceStatus;
};
export type BalanceStatus = Enum<{
    "Free": undefined;
    "Reserved": undefined;
}>;
export declare const BalanceStatus: GetEnum<BalanceStatus>;
export type I4cbvqmqadhrea = {
    "who": SS58String;
};
export type I3qt1hgg4djhgb = {
    "amount": bigint;
};
export type I4fooe9dun9o0t = {
    "old": bigint;
    "new": bigint;
};
export type TransactionPaymentEvent = Enum<{
    /**
     * A transaction fee `actual_fee`, of which `tip` was added to the minimum inclusion fee,
     * has been paid by `who`.
     */
    "TransactionFeePaid": Anonymize<Ier2cke86dqbr2>;
}>;
export declare const TransactionPaymentEvent: GetEnum<TransactionPaymentEvent>;
export type Ier2cke86dqbr2 = {
    "who": SS58String;
    "actual_fee": bigint;
    "tip": bigint;
};
export type I2hado50khaobu = AnonymousEnum<{
    /**
     * a new network is added.
     */
    "NetworkAdded": Anonymize<I9jd27rnpm8ttv>;
    /**
     * a network is removed.
     */
    "NetworkRemoved": number;
    /**
     * stake has been transferred from the a coldkey account onto the hotkey staking account.
     */
    "StakeAdded": Anonymize<Io45lnue7n40k>;
    /**
     * stake has been removed from the hotkey staking account onto the coldkey account.
     */
    "StakeRemoved": Anonymize<Io45lnue7n40k>;
    /**
     * stake has been moved from origin (hotkey, subnet ID) to destination (hotkey, subnet ID) of this amount (in TAO).
     */
    "StakeMoved": Anonymize<I83e4tgdv5ohg1>;
    /**
     * a caller successfully sets their weights on a subnetwork.
     */
    "WeightsSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * a new neuron account has been registered to the chain.
     */
    "NeuronRegistered": Anonymize<I6o6dmud53u1fj>;
    /**
     * multiple uids have been concurrently registered.
     */
    "BulkNeuronsRegistered": Anonymize<I9jd27rnpm8ttv>;
    /**
     * FIXME: Not used yet
     */
    "BulkBalancesSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * max allowed uids has been set for a subnetwork.
     */
    "MaxAllowedUidsSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * DEPRECATED: max weight limit updates are no longer supported.
     */
    "MaxWeightLimitSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * the difficulty has been set for a subnet.
     */
    "DifficultySet": Anonymize<I4ojmnsk1dchql>;
    /**
     * the adjustment interval is set for a subnet.
     */
    "AdjustmentIntervalSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * registration per interval is set for a subnet.
     */
    "RegistrationPerIntervalSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * we set max registrations per block.
     */
    "MaxRegistrationsPerBlockSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * an activity cutoff is set for a subnet.
     */
    "ActivityCutoffSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * Rho value is set.
     */
    "RhoSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * steepness of the sigmoid used to compute alpha values.
     */
    "AlphaSigmoidSteepnessSet": Anonymize<I5g2vv0ckl2m8b>;
    /**
     * Kappa is set for a subnet.
     */
    "KappaSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * minimum allowed weight is set for a subnet.
     */
    "MinAllowedWeightSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * the validator pruning length has been set.
     */
    "ValidatorPruneLenSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * the scaling law power has been set for a subnet.
     */
    "ScalingLawPowerSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * weights set rate limit has been set for a subnet.
     */
    "WeightsSetRateLimitSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * immunity period is set for a subnet.
     */
    "ImmunityPeriodSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * bonds moving average is set for a subnet.
     */
    "BondsMovingAverageSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * bonds penalty is set for a subnet.
     */
    "BondsPenaltySet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * bonds reset is set for a subnet.
     */
    "BondsResetOnSet": Anonymize<I39p6ln31i4n46>;
    /**
     * setting the max number of allowed validators on a subnet.
     */
    "MaxAllowedValidatorsSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * the axon server information is added to the network.
     */
    "AxonServed": Anonymize<I7svnfko10tq2e>;
    /**
     * the prometheus server information is added to the network.
     */
    "PrometheusServed": Anonymize<I7svnfko10tq2e>;
    /**
     * a hotkey has become a delegate.
     */
    "DelegateAdded": Anonymize<I7svrbkiu01iec>;
    /**
     * the default take is set.
     */
    "DefaultTakeSet": number;
    /**
     * weights version key is set for a network.
     */
    "WeightsVersionKeySet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting min difficulty on a network.
     */
    "MinDifficultySet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting max difficulty on a network.
     */
    "MaxDifficultySet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting the prometheus serving rate limit.
     */
    "ServingRateLimitSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting burn on a network.
     */
    "BurnSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting max burn on a network.
     */
    "MaxBurnSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting min burn on a network.
     */
    "MinBurnSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * setting the transaction rate limit.
     */
    "TxRateLimitSet": bigint;
    /**
     * setting the delegate take transaction rate limit.
     */
    "TxDelegateTakeRateLimitSet": bigint;
    /**
     * setting the childkey take transaction rate limit.
     */
    "TxChildKeyTakeRateLimitSet": bigint;
    /**
     * setting the admin freeze window length (last N blocks of tempo)
     */
    "AdminFreezeWindowSet": number;
    /**
     * setting the owner hyperparameter rate limit in epochs
     */
    "OwnerHyperparamRateLimitSet": number;
    /**
     * minimum childkey take set
     */
    "MinChildKeyTakeSet": number;
    /**
     * maximum childkey take set
     */
    "MaxChildKeyTakeSet": number;
    /**
     * childkey take set
     */
    "ChildKeyTakeSet": Anonymize<I6ouflveob4eli>;
    /**
     * a sudo call is done.
     */
    "Sudid": Anonymize<Ibq6c27da62s2q>;
    /**
     * registration is allowed/disallowed for a subnet.
     */
    "RegistrationAllowed": Anonymize<I39p6ln31i4n46>;
    /**
     * POW registration is allowed/disallowed for a subnet.
     */
    "PowRegistrationAllowed": Anonymize<I39p6ln31i4n46>;
    /**
     * setting tempo on a network
     */
    "TempoSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * setting the RAO recycled for registration.
     */
    "RAORecycledForRegistrationSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * min stake is set for validators to set weights.
     */
    "StakeThresholdSet": bigint;
    /**
     * setting the adjustment alpha on a subnet.
     */
    "AdjustmentAlphaSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * the faucet it called on the test net.
     */
    "Faucet": Anonymize<I95l2k9b1re95f>;
    /**
     * the subnet owner cut is set.
     */
    "SubnetOwnerCutSet": number;
    /**
     * the network creation rate limit is set.
     */
    "NetworkRateLimitSet": bigint;
    /**
     * the network immunity period is set.
     */
    "NetworkImmunityPeriodSet": bigint;
    /**
     * the start call delay is set.
     */
    "StartCallDelaySet": bigint;
    /**
     * the network minimum locking cost is set.
     */
    "NetworkMinLockCostSet": bigint;
    /**
     * the maximum number of subnets is set
     */
    "SubnetLimitSet": number;
    /**
     * the lock cost reduction is set
     */
    "NetworkLockCostReductionIntervalSet": bigint;
    /**
     * the take for a delegate is decreased.
     */
    "TakeDecreased": Anonymize<I7svrbkiu01iec>;
    /**
     * the take for a delegate is increased.
     */
    "TakeIncreased": Anonymize<I7svrbkiu01iec>;
    /**
     * the hotkey is swapped
     */
    "HotkeySwapped": Anonymize<Ifkgc6cte1k96e>;
    /**
     * maximum delegate take is set by sudo/admin transaction
     */
    "MaxDelegateTakeSet": number;
    /**
     * minimum delegate take is set by sudo/admin transaction
     */
    "MinDelegateTakeSet": number;
    /**
     * A coldkey swap announcement has been made.
     */
    "ColdkeySwapAnnounced": Anonymize<I6kvs2mb8unk0t>;
    /**
     * A coldkey swap has been reset.
     */
    "ColdkeySwapReset": Anonymize<I4cbvqmqadhrea>;
    /**
     * A coldkey has been swapped.
     */
    "ColdkeySwapped": Anonymize<Idbuci3sr3i1f7>;
    /**
     * A coldkey swap has been disputed.
     */
    "ColdkeySwapDisputed": Anonymize<I375tmdui1ejfc>;
    /**
     * All balance of a hotkey has been unstaked and transferred to a new coldkey
     */
    "AllBalanceUnstakedAndTransferredToNewColdkey": Anonymize<I73drt1hl9e70v>;
    /**
     * The arbitration period has been extended
     */
    "ArbitrationPeriodExtended": Anonymize<I375tmdui1ejfc>;
    /**
     * Setting of children of a hotkey have been scheduled
     */
    "SetChildrenScheduled": Anonymize<I1dm4sip108q0g>;
    /**
     * The children of a hotkey have been set
     */
    "SetChildren": Anonymize<Iajgphfb1fka7l>;
    /**
     * The identity of a coldkey has been set
     */
    "ChainIdentitySet": SS58String;
    /**
     * The identity of a subnet has been set
     */
    "SubnetIdentitySet": number;
    /**
     * The identity of a subnet has been removed
     */
    "SubnetIdentityRemoved": number;
    /**
     * A dissolve network extrinsic scheduled.
     */
    "DissolveNetworkScheduled": Anonymize<I4hnmf90qkrer9>;
    /**
     * The coldkey swap announcement delay has been set.
     */
    "ColdkeySwapAnnouncementDelaySet": number;
    /**
     * The coldkey swap reannouncement delay has been set.
     */
    "ColdkeySwapReannouncementDelaySet": number;
    /**
     * The duration of dissolve network has been set
     */
    "DissolveNetworkScheduleDurationSet": number;
    /**
     * Commit-reveal v3 weights have been successfully committed.
     *
     * - **who**: The account ID of the user committing the weights.
     * - **netuid**: The network identifier.
     * - **commit_hash**: The hash representing the committed weights.
     */
    "CRV3WeightsCommitted": Anonymize<Ijsohbv0raf36>;
    /**
     * Weights have been successfully committed.
     *
     * - **who**: The account ID of the user committing the weights.
     * - **netuid**: The network identifier.
     * - **commit_hash**: The hash representing the committed weights.
     */
    "WeightsCommitted": Anonymize<Ijsohbv0raf36>;
    /**
     * Weights have been successfully revealed.
     *
     * - **who**: The account ID of the user revealing the weights.
     * - **netuid**: The network identifier.
     * - **commit_hash**: The hash of the revealed weights.
     */
    "WeightsRevealed": Anonymize<Ijsohbv0raf36>;
    /**
     * Weights have been successfully batch revealed.
     *
     * - **who**: The account ID of the user revealing the weights.
     * - **netuid**: The network identifier.
     * - **revealed_hashes**: A vector of hashes representing each revealed weight set.
     */
    "WeightsBatchRevealed": Anonymize<I4ga01hppthoe1>;
    /**
     * A batch of weights (or commits) have been force-set.
     *
     * - **netuids**: The netuids these weights were successfully set/committed for.
     * - **who**: The hotkey that set this batch.
     */
    "BatchWeightsCompleted": Anonymize<I4hckkcv10tcue>;
    /**
     * A batch extrinsic completed but with some errors.
     */
    "BatchCompletedWithErrors": undefined;
    /**
     * A weight set among a batch of weights failed.
     *
     * - **error**: The dispatch error emitted by the failed item.
     */
    "BatchWeightItemFailed": Anonymize<Ic871mj76419vm>;
    /**
     * Stake has been transferred from one coldkey to another on the same subnet.
     * Parameters:
     * (origin_coldkey, destination_coldkey, hotkey, origin_netuid, destination_netuid, amount)
     */
    "StakeTransferred": Anonymize<If2ieedn10ujdv>;
    /**
     * Stake has been swapped from one subnet to another for the same coldkey-hotkey pair.
     *
     * Parameters:
     * (coldkey, hotkey, origin_netuid, destination_netuid, amount)
     */
    "StakeSwapped": Anonymize<Iaseh340tnovdh>;
    /**
     * Event called when transfer is toggled on a subnet.
     *
     * Parameters:
     * (netuid, bool)
     */
    "TransferToggle": Anonymize<I39p6ln31i4n46>;
    /**
     * The owner hotkey for a subnet has been set.
     *
     * Parameters:
     * (netuid, new_hotkey)
     */
    "SubnetOwnerHotkeySet": Anonymize<I7svnfko10tq2e>;
    /**
     * FirstEmissionBlockNumber is set via start call extrinsic
     *
     * Parameters:
     * netuid
     * block number
     */
    "FirstEmissionBlockNumberSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * Alpha has been recycled, reducing AlphaOut on a subnet.
     *
     * Parameters:
     * (coldkey, hotkey, amount, subnet_id)
     */
    "AlphaRecycled": Anonymize<I8m5umt6snnmlj>;
    /**
     * Alpha have been burned without reducing AlphaOut.
     *
     * Parameters:
     * (coldkey, hotkey, amount, subnet_id)
     */
    "AlphaBurned": Anonymize<I8m5umt6snnmlj>;
    /**
     * An EVM key has been associated with a hotkey.
     */
    "EvmKeyAssociated": Anonymize<I5aeg4u9kpsp8o>;
    /**
     * CRV3 Weights have been successfully revealed.
     *
     * - **netuid**: The network identifier.
     * - **who**: The account ID of the user revealing the weights.
     */
    "CRV3WeightsRevealed": Anonymize<I7svnfko10tq2e>;
    /**
     * Commit-Reveal periods has been successfully set.
     *
     * - **netuid**: The network identifier.
     * - **periods**: The number of epochs before the reveal.
     */
    "CommitRevealPeriodsSet": Anonymize<I4ojmnsk1dchql>;
    /**
     * Commit-Reveal has been successfully toggled.
     *
     * - **netuid**: The network identifier.
     * - **Enabled**: Is Commit-Reveal enabled.
     */
    "CommitRevealEnabled": Anonymize<I39p6ln31i4n46>;
    /**
     * the hotkey is swapped
     */
    "HotkeySwappedOnSubnet": Anonymize<I3fsv5f1boeqf3>;
    /**
     * A subnet lease has been created.
     */
    "SubnetLeaseCreated": Anonymize<Ifoov68qt28nbm>;
    /**
     * A subnet lease has been terminated.
     */
    "SubnetLeaseTerminated": Anonymize<Ib937mhlbop6j7>;
    /**
     * The symbol for a subnet has been updated.
     */
    "SymbolUpdated": Anonymize<I62rrikn5vj0p5>;
    /**
     * Commit Reveal Weights version has been updated.
     *
     * - **version**: The required version.
     */
    "CommitRevealVersionSet": number;
    /**
     * Timelocked weights have been successfully committed.
     *
     * - **who**: The account ID of the user committing the weights.
     * - **netuid**: The network identifier.
     * - **commit_hash**: The hash representing the committed weights.
     * - **reveal_round**: The round at which weights can be revealed.
     */
    "TimelockedWeightsCommitted": Anonymize<I838gqvljm75tj>;
    /**
     * Timelocked Weights have been successfully revealed.
     *
     * - **netuid**: The network identifier.
     * - **who**: The account ID of the user revealing the weights.
     */
    "TimelockedWeightsRevealed": Anonymize<I7svnfko10tq2e>;
    /**
     * Auto-staking hotkey received stake
     */
    "AutoStakeAdded": Anonymize<I1cu36qostj5d8>;
    /**
     * End-of-epoch miner incentive alpha by UID
     */
    "IncentiveAlphaEmittedToMiners": Anonymize<I4r2ptfsrl017r>;
    /**
     * The minimum allowed UIDs for a subnet have been set.
     */
    "MinAllowedUidsSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * The auto stake destination has been set.
     *
     * - **coldkey**: The account ID of the coldkey.
     * - **netuid**: The network identifier.
     * - **hotkey**: The account ID of the hotkey.
     */
    "AutoStakeDestinationSet": Anonymize<Ielglukq9ekcit>;
    /**
     * The minimum allowed non-Immune UIDs has been set.
     */
    "MinNonImmuneUidsSet": Anonymize<I9jd27rnpm8ttv>;
    /**
     * Root emissions have been claimed for a coldkey on all subnets and hotkeys.
     * Parameters:
     * (coldkey)
     */
    "RootClaimed": Anonymize<I375tmdui1ejfc>;
    /**
     * Root claim type for a coldkey has been set.
     * Parameters:
     * (coldkey, u8)
     */
    "RootClaimTypeSet": Anonymize<I1clsdhcok4nle>;
    /**
     * Voting power tracking has been enabled for a subnet.
     */
    "VotingPowerTrackingEnabled": Anonymize<I6cm4c5a1euio9>;
    /**
     * Voting power tracking has been scheduled for disabling.
     * Tracking will continue until disable_at_block, then stop and clear entries.
     */
    "VotingPowerTrackingDisableScheduled": Anonymize<Iemddv6u2buvfn>;
    /**
     * Voting power tracking has been fully disabled and entries cleared.
     */
    "VotingPowerTrackingDisabled": Anonymize<I6cm4c5a1euio9>;
    /**
     * Voting power EMA alpha has been set for a subnet.
     */
    "VotingPowerEmaAlphaSet": Anonymize<I4guv8rii4s6je>;
    /**
     * Subnet lease dividends have been distributed.
     */
    "SubnetLeaseDividendsDistributed": Anonymize<Ic149bnrif7lpr>;
    /**
     * "Add stake and burn" event: alpha token was purchased and burned.
     */
    "AddStakeBurn": Anonymize<I89dsvf7sdo4ko>;
}>;
export type I9jd27rnpm8ttv = FixedSizeArray<2, number>;
export type Io45lnue7n40k = [SS58String, SS58String, bigint, bigint, number, bigint];
export type I83e4tgdv5ohg1 = [SS58String, SS58String, number, SS58String, number, bigint];
export type I6o6dmud53u1fj = [number, number, SS58String];
export type I4ojmnsk1dchql = [number, bigint];
export type I5g2vv0ckl2m8b = [number, number];
export type I39p6ln31i4n46 = [number, boolean];
export type I7svnfko10tq2e = [number, SS58String];
export type I7svrbkiu01iec = [SS58String, SS58String, number];
export type I6ouflveob4eli = [SS58String, number];
export type Ibq6c27da62s2q = ResultPayload<undefined, Anonymize<Ic871mj76419vm>>;
export type I95l2k9b1re95f = [SS58String, bigint];
export type Ifkgc6cte1k96e = {
    /**
     * the account ID of coldkey
     */
    "coldkey": SS58String;
    /**
     * the account ID of old hotkey
     */
    "old_hotkey": SS58String;
    /**
     * the account ID of new hotkey
     */
    "new_hotkey": SS58String;
};
export type I6kvs2mb8unk0t = {
    /**
     * The account ID of the coldkey that made the announcement.
     */
    "who": SS58String;
    /**
     * The hash of the new coldkey.
     */
    "new_coldkey_hash": FixedSizeBinary<32>;
};
export type Idbuci3sr3i1f7 = {
    /**
     * The account ID of old coldkey.
     */
    "old_coldkey": SS58String;
    /**
     * The account ID of new coldkey.
     */
    "new_coldkey": SS58String;
};
export type I375tmdui1ejfc = {
    /**
     * The account ID of the coldkey that was disputed.
     */
    "coldkey": SS58String;
};
export type I73drt1hl9e70v = {
    /**
     * The account ID of the current coldkey
     */
    "current_coldkey": SS58String;
    /**
     * The account ID of the new coldkey
     */
    "new_coldkey": SS58String;
    /**
     * The total balance of the hotkey
     */
    "total_balance": bigint;
};
export type I1dm4sip108q0g = [SS58String, number, bigint, Anonymize<I5n8gpu725k1nu>];
export type I5n8gpu725k1nu = Array<[bigint, SS58String]>;
export type Iajgphfb1fka7l = [SS58String, number, Anonymize<I5n8gpu725k1nu>];
export type I4hnmf90qkrer9 = {
    /**
     * The account ID schedule the dissolve network extrinsic
     */
    "account": SS58String;
    /**
     * network ID will be dissolved
     */
    "netuid": number;
    /**
     * extrinsic execution block number
     */
    "execution_block": number;
};
export type Ijsohbv0raf36 = [SS58String, number, FixedSizeBinary<32>];
export type I4ga01hppthoe1 = [SS58String, number, Anonymize<Ic5m5lp1oioo8r>];
export type Ic5m5lp1oioo8r = Array<FixedSizeBinary<32>>;
export type I4hckkcv10tcue = [Anonymize<Icgljjb6j82uhn>, SS58String];
export type Icgljjb6j82uhn = Array<number>;
export type If2ieedn10ujdv = [SS58String, SS58String, SS58String, number, number, bigint];
export type Iaseh340tnovdh = [SS58String, SS58String, number, number, bigint];
export type I8m5umt6snnmlj = [SS58String, SS58String, bigint, number];
export type I5aeg4u9kpsp8o = {
    /**
     * The subnet that the hotkey belongs to.
     */
    "netuid": number;
    /**
     * The hotkey associated with the EVM key.
     */
    "hotkey": SS58String;
    /**
     * The EVM key being associated with the hotkey.
     */
    "evm_key": FixedSizeBinary<20>;
    /**
     * The block where the association happened.
     */
    "block_associated": bigint;
};
export type I3fsv5f1boeqf3 = {
    /**
     * the account ID of coldkey
     */
    "coldkey": SS58String;
    /**
     * the account ID of old hotkey
     */
    "old_hotkey": SS58String;
    /**
     * the account ID of new hotkey
     */
    "new_hotkey": SS58String;
    /**
     * the subnet ID
     */
    "netuid": number;
};
export type Ifoov68qt28nbm = {
    /**
     * The beneficiary of the lease.
     */
    "beneficiary": SS58String;
    /**
     * The lease ID
     */
    "lease_id": number;
    /**
     * The subnet ID
     */
    "netuid": number;
    /**
     * The end block of the lease
     */
    "end_block"?: Anonymize<I4arjljr6dpflb>;
};
export type I4arjljr6dpflb = (number) | undefined;
export type Ib937mhlbop6j7 = {
    /**
     * The beneficiary of the lease.
     */
    "beneficiary": SS58String;
    /**
     * The subnet ID
     */
    "netuid": number;
};
export type I62rrikn5vj0p5 = {
    /**
     * The subnet ID
     */
    "netuid": number;
    /**
     * The symbol that has been updated.
     */
    "symbol": Binary;
};
export type I838gqvljm75tj = [SS58String, number, FixedSizeBinary<32>, bigint];
export type I1cu36qostj5d8 = {
    /**
     * Subnet identifier.
     */
    "netuid": number;
    /**
     * Destination account that received the auto-staked funds.
     */
    "destination": SS58String;
    /**
     * Hotkey account whose stake was auto-staked.
     */
    "hotkey": SS58String;
    /**
     * Owner (coldkey) account associated with the hotkey.
     */
    "owner": SS58String;
    /**
     * Amount of alpha auto-staked.
     */
    "incentive": bigint;
};
export type I4r2ptfsrl017r = {
    /**
     * Subnet identifier.
     */
    "netuid": number;
    /**
     * UID-indexed array of miner incentive alpha; index equals UID.
     */
    "emissions": Anonymize<Iafqnechp3omqg>;
};
export type Iafqnechp3omqg = Array<bigint>;
export type Ielglukq9ekcit = {
    /**
     * The account ID of the coldkey.
     */
    "coldkey": SS58String;
    /**
     * The network identifier.
     */
    "netuid": number;
    /**
     * The account ID of the hotkey.
     */
    "hotkey": SS58String;
};
export type I1clsdhcok4nle = {
    /**
     * Claim coldkey
     */
    "coldkey": SS58String;
    /**
     * Claim type
     */
    "root_claim_type": Anonymize<Iapm6e7vtp0l6r>;
};
export type Iapm6e7vtp0l6r = AnonymousEnum<{
    "Swap": undefined;
    "Keep": undefined;
    "KeepSubnets": Anonymize<I2t4b7068rtebl>;
}>;
export type I2t4b7068rtebl = {
    "subnets": Anonymize<Icgljjb6j82uhn>;
};
export type I6cm4c5a1euio9 = {
    /**
     * The subnet ID
     */
    "netuid": number;
};
export type Iemddv6u2buvfn = {
    /**
     * The subnet ID
     */
    "netuid": number;
    /**
     * Block at which tracking will be disabled
     */
    "disable_at_block": bigint;
};
export type I4guv8rii4s6je = {
    /**
     * The subnet ID
     */
    "netuid": number;
    /**
     * The new alpha value (u64 with 18 decimal precision)
     */
    "alpha": bigint;
};
export type Ic149bnrif7lpr = {
    /**
     * The lease ID
     */
    "lease_id": number;
    /**
     * The contributor
     */
    "contributor": SS58String;
    /**
     * The amount of alpha distributed
     */
    "alpha": bigint;
};
export type I89dsvf7sdo4ko = {
    /**
     * The subnet ID
     */
    "netuid": number;
    /**
     * hotky account ID
     */
    "hotkey": SS58String;
    /**
     * Tao provided
     */
    "amount": bigint;
    /**
     * Alpha burned
     */
    "alpha": bigint;
};
export type If1btu4npshog7 = AnonymousEnum<{
    /**
     * Batch of dispatches did not complete fully. Index of first failing dispatch given, as
     * well as the error.
     */
    "BatchInterrupted": Anonymize<I804q3c12638a0>;
    /**
     * Batch of dispatches completed fully with no error.
     */
    "BatchCompleted": undefined;
    /**
     * Batch of dispatches completed but has errors.
     */
    "BatchCompletedWithErrors": undefined;
    /**
     * A single item within a Batch of dispatches has completed with no error.
     */
    "ItemCompleted": undefined;
    /**
     * A single item within a Batch of dispatches has completed with error.
     */
    "ItemFailed": Anonymize<Idguve298jnare>;
    /**
     * A call was dispatched.
     */
    "DispatchedAs": Anonymize<Idi3fb8585u2lp>;
    /**
     * Main call was dispatched.
     */
    "IfElseMainSuccess": undefined;
    /**
     * The fallback call was dispatched.
     */
    "IfElseFallbackCalled": Anonymize<I1327b77famnt3>;
}>;
export type I804q3c12638a0 = {
    "index": number;
    "error": Anonymize<Ic871mj76419vm>;
};
export type Idguve298jnare = {
    "error": Anonymize<Ic871mj76419vm>;
};
export type Idi3fb8585u2lp = {
    "result": Anonymize<Ibq6c27da62s2q>;
};
export type I1327b77famnt3 = {
    "main_error": Anonymize<Ic871mj76419vm>;
};
export type I8gl492p92bk6l = AnonymousEnum<{
    /**
     * A sudo call just took place.
     */
    "Sudid": Anonymize<If58ibsptjm2at>;
    /**
     * The sudo key has been updated.
     */
    "KeyChanged": Anonymize<I5rtkmhm2dng4u>;
    /**
     * The key was permanently removed.
     */
    "KeyRemoved": undefined;
    /**
     * A [sudo_as](Pallet::sudo_as) call just took place.
     */
    "SudoAsDone": Anonymize<If58ibsptjm2at>;
}>;
export type If58ibsptjm2at = {
    /**
     * The result of the call made by the sudo user.
     */
    "sudo_result": Anonymize<Ibq6c27da62s2q>;
};
export type I5rtkmhm2dng4u = {
    /**
     * The old sudo key (if one was previously set).
     */
    "old"?: Anonymize<Ihfphjolmsqq1>;
    /**
     * The new sudo key (if one was set).
     */
    "new": SS58String;
};
export type Ihfphjolmsqq1 = (SS58String) | undefined;
export type Ibmokdg2doop8d = AnonymousEnum<{
    /**
     * A new multisig operation has begun.
     */
    "NewMultisig": Anonymize<Iep27ialq4a7o7>;
    /**
     * A multisig operation has been approved by someone.
     */
    "MultisigApproval": Anonymize<Iasu5jvoqr43mv>;
    /**
     * A multisig operation has been executed.
     */
    "MultisigExecuted": Anonymize<I88p4dmln8611r>;
    /**
     * A multisig operation has been cancelled.
     */
    "MultisigCancelled": Anonymize<I5qolde99acmd1>;
    /**
     * The deposit for a multisig operation has been updated/poked.
     */
    "DepositPoked": Anonymize<I8gtde5abn1g9a>;
}>;
export type Iep27ialq4a7o7 = {
    "approving": SS58String;
    "multisig": SS58String;
    "call_hash": FixedSizeBinary<32>;
};
export type Iasu5jvoqr43mv = {
    "approving": SS58String;
    "timepoint": Anonymize<Itvprrpb0nm3o>;
    "multisig": SS58String;
    "call_hash": FixedSizeBinary<32>;
};
export type Itvprrpb0nm3o = {
    "height": number;
    "index": number;
};
export type I88p4dmln8611r = {
    "approving": SS58String;
    "timepoint": Anonymize<Itvprrpb0nm3o>;
    "multisig": SS58String;
    "call_hash": FixedSizeBinary<32>;
    "result": Anonymize<Ibq6c27da62s2q>;
};
export type I5qolde99acmd1 = {
    "cancelling": SS58String;
    "timepoint": Anonymize<Itvprrpb0nm3o>;
    "multisig": SS58String;
    "call_hash": FixedSizeBinary<32>;
};
export type I8gtde5abn1g9a = {
    "who": SS58String;
    "call_hash": FixedSizeBinary<32>;
    "old_deposit": bigint;
    "new_deposit": bigint;
};
export type PreimageEvent = Enum<{
    /**
     * A preimage has been noted.
     */
    "Noted": Anonymize<I1jm8m1rh9e20v>;
    /**
     * A preimage has been requested.
     */
    "Requested": Anonymize<I1jm8m1rh9e20v>;
    /**
     * A preimage has ben cleared.
     */
    "Cleared": Anonymize<I1jm8m1rh9e20v>;
}>;
export declare const PreimageEvent: GetEnum<PreimageEvent>;
export type I1jm8m1rh9e20v = {
    "hash": FixedSizeBinary<32>;
};
export type Iav5po3qov3sjo = AnonymousEnum<{
    /**
     * Scheduled some task.
     */
    "Scheduled": Anonymize<I5n4sebgkfr760>;
    /**
     * Canceled some task.
     */
    "Canceled": Anonymize<I5n4sebgkfr760>;
    /**
     * Dispatched some task.
     */
    "Dispatched": Anonymize<I3dvon8akhmsut>;
    /**
     * Set a retry configuration for some task.
     */
    "RetrySet": Anonymize<Ia3c82eadg79bj>;
    /**
     * Cancel a retry configuration for some task.
     */
    "RetryCancelled": Anonymize<Ienusoeb625ftq>;
    /**
     * The call for the provided hash was not found so the task has been aborted.
     */
    "CallUnavailable": Anonymize<Ienusoeb625ftq>;
    /**
     * The given task was unable to be renewed since the agenda is full at that block.
     */
    "PeriodicFailed": Anonymize<Ienusoeb625ftq>;
    /**
     * The given task was unable to be retried since the agenda is full at that block or there
     * was not enough weight to reschedule it.
     */
    "RetryFailed": Anonymize<Ienusoeb625ftq>;
    /**
     * The given task can never be executed since it is overweight.
     */
    "PermanentlyOverweight": Anonymize<Ienusoeb625ftq>;
    /**
     * Agenda is incomplete from `when`.
     */
    "AgendaIncomplete": Anonymize<Ibtsa3docbr9el>;
}>;
export type I5n4sebgkfr760 = {
    "when": number;
    "index": number;
};
export type I3dvon8akhmsut = {
    "task": Anonymize<I9jd27rnpm8ttv>;
    "id"?: Anonymize<I4s6vifaf8k998>;
    "result": Anonymize<Ibq6c27da62s2q>;
};
export type I4s6vifaf8k998 = (FixedSizeBinary<32>) | undefined;
export type Ia3c82eadg79bj = {
    "task": Anonymize<I9jd27rnpm8ttv>;
    "id"?: Anonymize<I4s6vifaf8k998>;
    "period": number;
    "retries": number;
};
export type Ienusoeb625ftq = {
    "task": Anonymize<I9jd27rnpm8ttv>;
    "id"?: Anonymize<I4s6vifaf8k998>;
};
export type Ibtsa3docbr9el = {
    "when": number;
};
export type I65un9enf26o4o = AnonymousEnum<{
    /**
     * A proxy was executed correctly, with the given.
     */
    "ProxyExecuted": Anonymize<Idi3fb8585u2lp>;
    /**
     * A pure account has been created by new proxy with given
     * disambiguation index and proxy type.
     */
    "PureCreated": Anonymize<Iek6442ldi23n3>;
    /**
     * A pure proxy was killed by its spawner.
     */
    "PureKilled": Anonymize<Idpdo54rotesu2>;
    /**
     * An announcement was placed to make a call in the future.
     */
    "Announced": Anonymize<I2ur0oeqg495j8>;
    /**
     * A proxy was added.
     */
    "ProxyAdded": Anonymize<Ibco2bqthggul0>;
    /**
     * A proxy was removed.
     */
    "ProxyRemoved": Anonymize<Ibco2bqthggul0>;
    /**
     * A deposit stored for proxies or announcements was poked / updated.
     */
    "DepositPoked": Anonymize<I1bhd210c3phjj>;
}>;
export type Iek6442ldi23n3 = {
    "pure": SS58String;
    "who": SS58String;
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "disambiguation_index": number;
};
export type I8v1041j74kmaj = AnonymousEnum<{
    "Any": undefined;
    "Owner": undefined;
    "NonCritical": undefined;
    "NonTransfer": undefined;
    "Senate": undefined;
    "NonFungible": undefined;
    "Triumvirate": undefined;
    "Governance": undefined;
    "Staking": undefined;
    "Registration": undefined;
    "Transfer": undefined;
    "SmallTransfer": undefined;
    "RootWeights": undefined;
    "ChildKeys": undefined;
    "SudoUncheckedSetCode": undefined;
    "SwapHotkey": undefined;
    "SubnetLeaseBeneficiary": undefined;
    "RootClaim": undefined;
}>;
export type Idpdo54rotesu2 = {
    "pure": SS58String;
    "spawner": SS58String;
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "disambiguation_index": number;
};
export type I2ur0oeqg495j8 = {
    "real": SS58String;
    "proxy": SS58String;
    "call_hash": FixedSizeBinary<32>;
};
export type Ibco2bqthggul0 = {
    "delegator": SS58String;
    "delegatee": SS58String;
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "delay": number;
};
export type I1bhd210c3phjj = {
    "who": SS58String;
    "kind": Enum<{
        "Proxies": undefined;
        "Announcements": undefined;
    }>;
    "old_deposit": bigint;
    "new_deposit": bigint;
};
export type I626vh1cit09ni = AnonymousEnum<{
    /**
     * Emitted when a user registers an identity
     */
    "IdentitySet": Anonymize<I4cbvqmqadhrea>;
    /**
     * Emitted when a user dissolves an identity
     */
    "IdentityDissolved": Anonymize<I4cbvqmqadhrea>;
}>;
export type I5ohlg8gv4pe9g = AnonymousEnum<{
    /**
     * A commitment was set
     */
    "Commitment": Anonymize<Idcqgi2844k5he>;
    /**
     * A timelock-encrypted commitment was set
     */
    "TimelockCommitment": Anonymize<Iej2173ou338sm>;
    /**
     * A timelock-encrypted commitment was auto-revealed
     */
    "CommitmentRevealed": Anonymize<Idcqgi2844k5he>;
}>;
export type Idcqgi2844k5he = {
    /**
     * The netuid of the commitment
     */
    "netuid": number;
    /**
     * The account
     */
    "who": SS58String;
};
export type Iej2173ou338sm = {
    /**
     * The netuid of the commitment
     */
    "netuid": number;
    /**
     * The account
     */
    "who": SS58String;
    /**
     * The drand round to reveal
     */
    "reveal_round": bigint;
};
export type Ic1vmbif9o0nug = AnonymousEnum<{
    /**
     * Event emitted when a precompile operation is updated.
     */
    "PrecompileUpdated": Anonymize<I1sj8huj7of8mb>;
    /**
     * Event emitted when the Yuma3 enable is toggled.
     */
    "Yuma3EnableToggled": Anonymize<Ie31ro5s5e089f>;
    /**
     * Event emitted when Bonds Reset is toggled.
     */
    "BondsResetToggled": Anonymize<Ie31ro5s5e089f>;
}>;
export type I1sj8huj7of8mb = {
    /**
     * The type of precompile operation being updated.
     */
    "precompile_id": Anonymize<I8un1ap2r4hhbj>;
    /**
     * Indicates if the precompile operation is enabled or not.
     */
    "enabled": boolean;
};
export type I8un1ap2r4hhbj = AnonymousEnum<{
    "BalanceTransfer": undefined;
    "Staking": undefined;
    "Subnet": undefined;
    "Metagraph": undefined;
    "Neuron": undefined;
    "UidLookup": undefined;
    "Alpha": undefined;
    "Crowdloan": undefined;
    "Proxy": undefined;
    "Leasing": undefined;
    "AddressMapping": undefined;
    "VotingPower": undefined;
}>;
export type Ie31ro5s5e089f = {
    /**
     * The network identifier.
     */
    "netuid": number;
    /**
     * Indicates if the Yuma3 enable was enabled or disabled.
     */
    "enabled": boolean;
};
export type I3q8c83f5dvokp = AnonymousEnum<{
    /**
     * The safe-mode was entered until inclusively this block.
     */
    "Entered": Anonymize<I20e9ph536u7ti>;
    /**
     * The safe-mode was extended until inclusively this block.
     */
    "Extended": Anonymize<I20e9ph536u7ti>;
    /**
     * Exited the safe-mode for a specific reason.
     */
    "Exited": Anonymize<I8kcpmsh450rp>;
    /**
     * An account reserved funds for either entering or extending the safe-mode.
     */
    "DepositPlaced": Anonymize<Ic262ibdoec56a>;
    /**
     * An account had a reserve released that was reserved.
     */
    "DepositReleased": Anonymize<Ic262ibdoec56a>;
    /**
     * An account had reserve slashed that was reserved.
     */
    "DepositSlashed": Anonymize<Ic262ibdoec56a>;
    /**
     * Could not hold funds for entering or extending the safe-mode.
     *
     * This error comes from the underlying `Currency`.
     */
    "CannotDeposit": undefined;
    /**
     * Could not release funds for entering or extending the safe-mode.
     *
     * This error comes from the underlying `Currency`.
     */
    "CannotRelease": undefined;
}>;
export type I20e9ph536u7ti = {
    "until": number;
};
export type I8kcpmsh450rp = {
    "reason": Enum<{
        "Timeout": undefined;
        "Force": undefined;
    }>;
};
export type I510u4q1qqh897 = AnonymousEnum<{
    /**
     * An ethereum transaction was successfully executed.
     */
    "Executed": Anonymize<Iea4g5ovhnolus>;
}>;
export type Iea4g5ovhnolus = {
    "from": FixedSizeBinary<20>;
    "to": FixedSizeBinary<20>;
    "transaction_hash": FixedSizeBinary<32>;
    "exit_reason": Anonymize<Iag9iovb9j5ijo>;
    "extra_data": Binary;
};
export type Iag9iovb9j5ijo = AnonymousEnum<{
    "Succeed": Enum<{
        "Stopped": undefined;
        "Returned": undefined;
        "Suicided": undefined;
    }>;
    "Error": Anonymize<I5ksr7ru2gk4nh>;
    "Revert": Enum<{
        "Reverted": undefined;
    }>;
    "Fatal": Enum<{
        "NotSupported": undefined;
        "UnhandledInterrupt": undefined;
        "CallErrorAsFatal": Anonymize<I5ksr7ru2gk4nh>;
        "Other": string;
    }>;
}>;
export type I5ksr7ru2gk4nh = AnonymousEnum<{
    "StackUnderflow": undefined;
    "StackOverflow": undefined;
    "InvalidJump": undefined;
    "InvalidRange": undefined;
    "DesignatedInvalid": undefined;
    "CallTooDeep": undefined;
    "CreateCollision": undefined;
    "CreateContractLimit": undefined;
    "InvalidCode": number;
    "OutOfOffset": undefined;
    "OutOfGas": undefined;
    "OutOfFund": undefined;
    "PCUnderflow": undefined;
    "CreateEmpty": undefined;
    "Other": string;
    "MaxNonce": undefined;
}>;
export type I9k071kk4cn1u8 = AnonymousEnum<{
    /**
     * Ethereum events from contracts.
     */
    "Log": Anonymize<Ifmc9boeeia623>;
    /**
     * A contract has been created at given address.
     */
    "Created": Anonymize<Itmchvgqfl28g>;
    /**
     * A contract was attempted to be created, but the execution failed.
     */
    "CreatedFailed": Anonymize<Itmchvgqfl28g>;
    /**
     * A contract has been executed successfully with states applied.
     */
    "Executed": Anonymize<Itmchvgqfl28g>;
    /**
     * A contract has been executed with errors. States are reverted with only gas fees applied.
     */
    "ExecutedFailed": Anonymize<Itmchvgqfl28g>;
}>;
export type Ifmc9boeeia623 = {
    "log": Anonymize<I10qb03fpuk6em>;
};
export type I10qb03fpuk6em = {
    "address": FixedSizeBinary<20>;
    "topics": Anonymize<Ic5m5lp1oioo8r>;
    "data": Binary;
};
export type Itmchvgqfl28g = {
    "address": FixedSizeBinary<20>;
};
export type I3bmatomsds8j7 = AnonymousEnum<{
    "NewBaseFeePerGas": Anonymize<I7vi74gbubc8u5>;
    "BaseFeeOverflow": undefined;
    "NewElasticity": Anonymize<I3u0knmtb1ueq7>;
}>;
export type I7vi74gbubc8u5 = {
    "fee": Anonymize<I4totqt881mlti>;
};
export type I4totqt881mlti = FixedSizeArray<4, bigint>;
export type I3u0knmtb1ueq7 = {
    "elasticity": number;
};
export type Ibdlgbf9b95hbj = AnonymousEnum<{
    /**
     * Beacon Configuration has changed.
     */
    "BeaconConfigChanged": undefined;
    /**
     * Successfully set a new pulse(s).
     */
    "NewPulse": Anonymize<I5tf7b5o64mfpl>;
    /**
     * Oldest Stored Round has been set.
     */
    "SetOldestStoredRound": bigint;
}>;
export type I5tf7b5o64mfpl = {
    "rounds": Anonymize<Iafqnechp3omqg>;
};
export type Ifj1h07t3i0np9 = AnonymousEnum<{
    /**
     * A crowdloan was created.
     */
    "Created": Anonymize<If71d2q730qf6n>;
    /**
     * A contribution was made to an active crowdloan.
     */
    "Contributed": Anonymize<If0sk51c1n7ri8>;
    /**
     * A contribution was withdrawn from a failed crowdloan.
     */
    "Withdrew": Anonymize<If0sk51c1n7ri8>;
    /**
     * A refund was partially processed for a failed crowdloan.
     */
    "PartiallyRefunded": Anonymize<I5dueehi6i2dg9>;
    /**
     * A refund was fully processed for a failed crowdloan.
     */
    "AllRefunded": Anonymize<I5dueehi6i2dg9>;
    /**
     * A crowdloan was finalized, funds were transferred and the call was dispatched.
     */
    "Finalized": Anonymize<I5dueehi6i2dg9>;
    /**
     * A crowdloan was dissolved.
     */
    "Dissolved": Anonymize<I5dueehi6i2dg9>;
    /**
     * The minimum contribution was updated.
     */
    "MinContributionUpdated": Anonymize<I64ev05f6q10es>;
    /**
     * The end was updated.
     */
    "EndUpdated": Anonymize<Ikc5h15joooak>;
    /**
     * The cap was updated.
     */
    "CapUpdated": Anonymize<Ie8f436ua5fs59>;
}>;
export type If71d2q730qf6n = {
    "crowdloan_id": number;
    "creator": SS58String;
    "end": number;
    "cap": bigint;
};
export type If0sk51c1n7ri8 = {
    "crowdloan_id": number;
    "contributor": SS58String;
    "amount": bigint;
};
export type I5dueehi6i2dg9 = {
    "crowdloan_id": number;
};
export type I64ev05f6q10es = {
    "crowdloan_id": number;
    "new_min_contribution": bigint;
};
export type Ikc5h15joooak = {
    "crowdloan_id": number;
    "new_end": number;
};
export type Ie8f436ua5fs59 = {
    "crowdloan_id": number;
    "new_cap": bigint;
};
export type I6qodidnq3s4e1 = AnonymousEnum<{
    /**
     * Event emitted when the fee rate has been updated for a subnet
     */
    "FeeRateSet": Anonymize<I3mkis681qg30e>;
    /**
     * Event emitted when user liquidity operations are enabled for a subnet.
     * First enable even indicates a switch from V2 to V3 swap.
     */
    "UserLiquidityToggled": Anonymize<I2foqo7cbqf35v>;
    /**
     * Event emitted when a liquidity position is added to a subnet's liquidity pool.
     */
    "LiquidityAdded": Anonymize<I4b2eh3b1oi815>;
    /**
     * Event emitted when a liquidity position is removed from a subnet's liquidity pool.
     */
    "LiquidityRemoved": Anonymize<I57q620f4fu1bl>;
    /**
     * Event emitted when a liquidity position is modified in a subnet's liquidity pool.
     * Modifying causes the fees to be claimed.
     */
    "LiquidityModified": Anonymize<I57q620f4fu1bl>;
}>;
export type I3mkis681qg30e = {
    "netuid": number;
    "rate": number;
};
export type I2foqo7cbqf35v = {
    "netuid": number;
    "enable": boolean;
};
export type I4b2eh3b1oi815 = {
    /**
     * The coldkey account that owns the position
     */
    "coldkey": SS58String;
    /**
     * The hotkey account where Alpha comes from
     */
    "hotkey": SS58String;
    /**
     * The subnet identifier
     */
    "netuid": number;
    /**
     * Unique identifier for the liquidity position
     */
    "position_id": bigint;
    /**
     * The amount of liquidity added to the position
     */
    "liquidity": bigint;
    /**
     * The amount of TAO tokens committed to the position
     */
    "tao": bigint;
    /**
     * The amount of Alpha tokens committed to the position
     */
    "alpha": bigint;
    /**
     * the lower tick
     */
    "tick_low": number;
    /**
     * the upper tick
     */
    "tick_high": number;
};
export type I57q620f4fu1bl = {
    /**
     * The coldkey account that owns the position
     */
    "coldkey": SS58String;
    /**
     * The hotkey account where Alpha goes to
     */
    "hotkey": SS58String;
    /**
     * The subnet identifier
     */
    "netuid": number;
    /**
     * Unique identifier for the liquidity position
     */
    "position_id": bigint;
    /**
     * The amount of liquidity removed from the position
     */
    "liquidity": bigint;
    /**
     * The amount of TAO tokens returned to the user
     */
    "tao": bigint;
    /**
     * The amount of Alpha tokens returned to the user
     */
    "alpha": bigint;
    /**
     * The amount of TAO fees earned from the position
     */
    "fee_tao": bigint;
    /**
     * The amount of Alpha fees earned from the position
     */
    "fee_alpha": bigint;
    /**
     * the lower tick
     */
    "tick_low": number;
    /**
     * the upper tick
     */
    "tick_high": number;
};
export type I211sbjvh5hjqu = AnonymousEnum<{
    /**
     * Contract deployed by address at the specified address.
     */
    "Instantiated": Anonymize<Ie5222qfrr24ek>;
    /**
     * Contract has been removed.
     *
     * # Note
     *
     * The only way for a contract to be removed and emitting this event is by calling
     * `seal_terminate`.
     */
    "Terminated": Anonymize<I28g8sphdu312k>;
    /**
     * Code with the specified hash has been stored.
     */
    "CodeStored": Anonymize<Idqbjt2c6r46t6>;
    /**
     * A custom event emitted by the contract.
     */
    "ContractEmitted": Anonymize<I853aigjva3f0t>;
    /**
     * A code with the specified hash was removed.
     */
    "CodeRemoved": Anonymize<I9uehhems5hkqm>;
    /**
     * A contract's code was updated.
     */
    "ContractCodeUpdated": Anonymize<I7q5qk4uoanhof>;
    /**
     * A contract was called either by a plain account or another contract.
     *
     * # Note
     *
     * Please keep in mind that like all events this is only emitted for successful
     * calls. This is because on failure all storage changes including events are
     * rolled back.
     */
    "Called": Anonymize<Iehpbs40l3jkit>;
    /**
     * A contract delegate called a code hash.
     *
     * # Note
     *
     * Please keep in mind that like all events this is only emitted for successful
     * calls. This is because on failure all storage changes including events are
     * rolled back.
     */
    "DelegateCalled": Anonymize<Idht9upmipvd4j>;
    /**
     * Some funds have been transferred and held as storage deposit.
     */
    "StorageDepositTransferredAndHeld": Anonymize<Iflcfm9b6nlmdd>;
    /**
     * Some storage deposit funds have been transferred and released.
     */
    "StorageDepositTransferredAndReleased": Anonymize<Iflcfm9b6nlmdd>;
}>;
export type Ie5222qfrr24ek = {
    "deployer": SS58String;
    "contract": SS58String;
};
export type I28g8sphdu312k = {
    /**
     * The contract that was terminated.
     */
    "contract": SS58String;
    /**
     * The account that received the contracts remaining balance
     */
    "beneficiary": SS58String;
};
export type Idqbjt2c6r46t6 = {
    "code_hash": FixedSizeBinary<32>;
    "deposit_held": bigint;
    "uploader": SS58String;
};
export type I853aigjva3f0t = {
    /**
     * The contract that emitted the event.
     */
    "contract": SS58String;
    /**
     * Data supplied by the contract. Metadata generated during contract compilation
     * is needed to decode it.
     */
    "data": Binary;
};
export type I9uehhems5hkqm = {
    "code_hash": FixedSizeBinary<32>;
    "deposit_released": bigint;
    "remover": SS58String;
};
export type I7q5qk4uoanhof = {
    /**
     * The contract that has been updated.
     */
    "contract": SS58String;
    /**
     * New code hash that was set for the contract.
     */
    "new_code_hash": FixedSizeBinary<32>;
    /**
     * Previous code hash of the contract.
     */
    "old_code_hash": FixedSizeBinary<32>;
};
export type Iehpbs40l3jkit = {
    /**
     * The caller of the `contract`.
     */
    "caller": Enum<{
        "Root": undefined;
        "Signed": SS58String;
    }>;
    /**
     * The contract that was called.
     */
    "contract": SS58String;
};
export type Idht9upmipvd4j = {
    /**
     * The contract that performed the delegate call and hence in whose context
     * the `code_hash` is executed.
     */
    "contract": SS58String;
    /**
     * The code hash that was delegate called.
     */
    "code_hash": FixedSizeBinary<32>;
};
export type I70thgcbmbqm91 = AnonymousEnum<{
    /**
     * Encrypted wrapper accepted.
     */
    "EncryptedSubmitted": Anonymize<Icns2sqr5hp8s3>;
    /**
     * Decrypted call executed.
     */
    "DecryptedExecuted": Anonymize<I9n4hs8p3rlkag>;
    /**
     * Decrypted execution rejected.
     */
    "DecryptedRejected": Anonymize<I6a8j73186lfdf>;
    /**
     * Decryption failed - validator could not decrypt the submission.
     */
    "DecryptionFailed": Anonymize<I602p6mm30elei>;
}>;
export type Icns2sqr5hp8s3 = {
    "id": FixedSizeBinary<32>;
    "who": SS58String;
};
export type I9n4hs8p3rlkag = {
    "id": FixedSizeBinary<32>;
    "signer": SS58String;
};
export type I6a8j73186lfdf = {
    "id": FixedSizeBinary<32>;
    "reason": {
        "post_info": {
            "actual_weight"?: Anonymize<Iasb8k6ash5mjn>;
            "pays_fee": Anonymize<Iehg04bj71rkd>;
        };
        "error": Anonymize<Ic871mj76419vm>;
    };
};
export type Iasb8k6ash5mjn = (Anonymize<I4q39t5hn830vp>) | undefined;
export type I602p6mm30elei = {
    "id": FixedSizeBinary<32>;
    "reason": Binary;
};
export type I95g6i7ilua7lq = Array<Anonymize<I9jd27rnpm8ttv>>;
export type Ieniouoqkq4icf = {
    "spec_version": number;
    "spec_name": string;
};
export type GrandpaStoredState = Enum<{
    "Live": undefined;
    "PendingPause": {
        "scheduled_at": number;
        "delay": number;
    };
    "Paused": undefined;
    "PendingResume": {
        "scheduled_at": number;
        "delay": number;
    };
}>;
export declare const GrandpaStoredState: GetEnum<GrandpaStoredState>;
export type I7pe2me3i3vtn9 = {
    "scheduled_at": number;
    "delay": number;
    "next_authorities": Anonymize<I3geksg000c171>;
    "forced"?: Anonymize<I4arjljr6dpflb>;
};
export type I8ds64oj6581v0 = Array<{
    "id": FixedSizeBinary<8>;
    "amount": bigint;
    "reasons": BalancesTypesReasons;
}>;
export type BalancesTypesReasons = Enum<{
    "Fee": undefined;
    "Misc": undefined;
    "All": undefined;
}>;
export declare const BalancesTypesReasons: GetEnum<BalancesTypesReasons>;
export type Ia7pdug7cdsg8g = Array<{
    "id": FixedSizeBinary<8>;
    "amount": bigint;
}>;
export type I2hnk9r4ukuj1p = Array<{
    "id": Enum<{
        "Preimage": PreimagePalletHoldReason;
        "Registry": Enum<{
            "RegistryIdentity": undefined;
        }>;
        "SafeMode": Enum<{
            "EnterOrExtend": undefined;
        }>;
        "Contracts": Enum<{
            "CodeUploadDepositReserve": undefined;
            "StorageDepositReserve": undefined;
        }>;
    }>;
    "amount": bigint;
}>;
export type PreimagePalletHoldReason = Enum<{
    "Preimage": undefined;
}>;
export declare const PreimagePalletHoldReason: GetEnum<PreimagePalletHoldReason>;
export type I9bin2jc70qt6q = Array<Anonymize<I3qt1hgg4djhgb>>;
export type TransactionPaymentReleases = Enum<{
    "V1Ancient": undefined;
    "V2": undefined;
}>;
export declare const TransactionPaymentReleases: GetEnum<TransactionPaymentReleases>;
export type Idoeu5t0dum8va = [Anonymize<I5n8gpu725k1nu>, bigint];
export type Ia2lhg7l2hilo3 = Array<SS58String>;
export type I4p5t2krb1gmvp = [number, FixedSizeBinary<32>];
export type Iabpgqcjikia83 = (Binary) | undefined;
export type I2j729bmgsdiuo = [bigint, bigint];
export type Iakavvne152v30 = AnonymousEnum<{
    "SetSNOwnerHotkey": number;
    "OwnerHyperparamUpdate": [number, Enum<{
        "Unknown": undefined;
        "ServingRateLimit": undefined;
        "MaxDifficulty": undefined;
        "AdjustmentAlpha": undefined;
        "MaxWeightLimit": undefined;
        "ImmunityPeriod": undefined;
        "MinAllowedWeights": undefined;
        "Kappa": undefined;
        "Rho": undefined;
        "ActivityCutoff": undefined;
        "PowRegistrationAllowed": undefined;
        "MinBurn": undefined;
        "MaxBurn": undefined;
        "BondsMovingAverage": undefined;
        "BondsPenalty": undefined;
        "CommitRevealEnabled": undefined;
        "LiquidAlphaEnabled": undefined;
        "AlphaValues": undefined;
        "WeightCommitInterval": undefined;
        "TransferEnabled": undefined;
        "AlphaSigmoidSteepness": undefined;
        "Yuma3Enabled": undefined;
        "BondsResetEnabled": undefined;
        "ImmuneNeuronLimit": undefined;
        "RecycleOrBurn": undefined;
        "MaxAllowedUids": undefined;
    }>];
    "NetworkLastRegistered": undefined;
    "LastTxBlock": SS58String;
    "LastTxBlockChildKeyTake": SS58String;
    "LastTxBlockDelegateTake": SS58String;
    "AddStakeBurn": number;
}>;
export type Ib9tptuv3cggfs = AnonymousEnum<{
    "Burn": undefined;
    "Recycle": undefined;
}>;
export type I4h6ivgjtd51lv = Array<[SS58String, bigint, bigint]>;
export type I9eir063evtfb6 = Array<boolean>;
export type Ibc83gdj8hi3rc = {
    "block": bigint;
    "version": number;
    "ip": bigint;
    "port": number;
    "ip_type": number;
    "protocol": number;
    "placeholder1": number;
    "placeholder2": number;
};
export type I9lpjucl20l82d = {
    "public_key": Binary;
    "algorithm": number;
};
export type Iaap7oohdmr1sb = {
    "block": bigint;
    "version": number;
    "ip": bigint;
    "port": number;
    "ip_type": number;
};
export type Ifjlj958aeheic = {
    "name": Binary;
    "url": Binary;
    "github_repo": Binary;
    "image": Binary;
    "discord": Binary;
    "description": Binary;
    "additional": Binary;
};
export type I4tc54pa558g5n = {
    "subnet_name": Binary;
    "github_repo": Binary;
    "subnet_contact": Binary;
    "subnet_url": Binary;
    "discord": Binary;
    "description": Binary;
    "logo_url": Binary;
    "additional": Binary;
};
export type Id32h28hjj1tch = [SS58String, number, number];
export type Icrrf4uohj5gb0 = Array<[FixedSizeBinary<32>, bigint, bigint, bigint]>;
export type I76jd8kl1mtn5g = Array<[SS58String, bigint, Binary, bigint]>;
export type I4jqk5si14p5oi = Array<[SS58String, Binary, bigint]>;
export type I2na29tt2afp0j = FixedSizeArray<2, SS58String>;
export type If9jidduiuq7vv = Array<Anonymize<I4ojmnsk1dchql>>;
export type I2brm5b9jij1st = [number, SS58String, SS58String];
export type I7tof95tckt2r = [FixedSizeBinary<20>, bigint];
export type Ieruonr5pk2d7h = {
    "beneficiary": SS58String;
    "coldkey": SS58String;
    "hotkey": SS58String;
    "emissions_share": number;
    "end_block"?: Anonymize<I4arjljr6dpflb>;
    "netuid": number;
    "cost": bigint;
};
export type Iag146hmjgqfgj = {
    "when": Anonymize<Itvprrpb0nm3o>;
    "deposit": bigint;
    "depositor": SS58String;
    "approvals": Anonymize<Ia2lhg7l2hilo3>;
};
export type I8uo3fpd3bcc6f = [SS58String, FixedSizeBinary<32>];
export type PreimageOldRequestStatus = Enum<{
    "Unrequested": {
        "deposit": Anonymize<I95l2k9b1re95f>;
        "len": number;
    };
    "Requested": {
        "deposit"?: Anonymize<I92hdo1clkbp4g>;
        "count": number;
        "len"?: Anonymize<I4arjljr6dpflb>;
    };
}>;
export declare const PreimageOldRequestStatus: GetEnum<PreimageOldRequestStatus>;
export type I92hdo1clkbp4g = (Anonymize<I95l2k9b1re95f>) | undefined;
export type PreimageRequestStatus = Enum<{
    "Unrequested": {
        "ticket": Anonymize<I95l2k9b1re95f>;
        "len": number;
    };
    "Requested": {
        "maybe_ticket"?: Anonymize<I92hdo1clkbp4g>;
        "count": number;
        "maybe_len"?: Anonymize<I4arjljr6dpflb>;
    };
}>;
export declare const PreimageRequestStatus: GetEnum<PreimageRequestStatus>;
export type I4pact7n2e9a0i = [FixedSizeBinary<32>, number];
export type I11tetbe8ces3o = Array<({
    "maybe_id"?: Anonymize<I4s6vifaf8k998>;
    "priority": number;
    "call": PreimagesBounded;
    "maybe_periodic"?: Anonymize<Iep7au1720bm0e>;
    "origin": Anonymize<I32es0rp64745v>;
}) | undefined>;
export type PreimagesBounded = Enum<{
    "Legacy": Anonymize<I1jm8m1rh9e20v>;
    "Inline": Binary;
    "Lookup": {
        "hash": FixedSizeBinary<32>;
        "len": number;
    };
}>;
export declare const PreimagesBounded: GetEnum<PreimagesBounded>;
export type Iep7au1720bm0e = (Anonymize<I9jd27rnpm8ttv>) | undefined;
export type I32es0rp64745v = AnonymousEnum<{
    "system": Enum<{
        "Root": undefined;
        "Signed": SS58String;
        "None": undefined;
        "Authorized": undefined;
    }>;
    "Ethereum": Enum<{
        "EthereumTransaction": FixedSizeBinary<20>;
    }>;
}>;
export type I56u24ncejr5kt = {
    "total_retries": number;
    "remaining": number;
    "period": number;
};
export type I6tqrno2gaos08 = [Array<{
    "delegate": SS58String;
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "delay": number;
}>, bigint];
export type I9p9lq3rej5bhc = [Array<{
    "real": SS58String;
    "call_hash": FixedSizeBinary<32>;
    "height": number;
}>, bigint];
export type Ib6u9l1gtc5l4t = {
    "deposit": bigint;
    "info": Anonymize<Ifiu33afi2n7qs>;
};
export type Ifiu33afi2n7qs = {
    "additional": Array<FixedSizeArray<2, Anonymize<I2fomq92gvvqhc>>>;
    "display": Anonymize<I2fomq92gvvqhc>;
    "legal": Anonymize<I2fomq92gvvqhc>;
    "web": Anonymize<I2fomq92gvvqhc>;
    "riot": Anonymize<I2fomq92gvvqhc>;
    "email": Anonymize<I2fomq92gvvqhc>;
    "pgp_fingerprint"?: Anonymize<If7b8240vgt2q5>;
    "image": Anonymize<I2fomq92gvvqhc>;
    "twitter": Anonymize<I2fomq92gvvqhc>;
};
export type I2fomq92gvvqhc = AnonymousEnum<{
    "None": undefined;
    "Raw0": undefined;
    "Raw1": number;
    "Raw2": FixedSizeBinary<2>;
    "Raw3": FixedSizeBinary<3>;
    "Raw4": FixedSizeBinary<4>;
    "Raw5": FixedSizeBinary<5>;
    "Raw6": FixedSizeBinary<6>;
    "Raw7": FixedSizeBinary<7>;
    "Raw8": FixedSizeBinary<8>;
    "Raw9": FixedSizeBinary<9>;
    "Raw10": FixedSizeBinary<10>;
    "Raw11": FixedSizeBinary<11>;
    "Raw12": FixedSizeBinary<12>;
    "Raw13": FixedSizeBinary<13>;
    "Raw14": FixedSizeBinary<14>;
    "Raw15": FixedSizeBinary<15>;
    "Raw16": FixedSizeBinary<16>;
    "Raw17": FixedSizeBinary<17>;
    "Raw18": FixedSizeBinary<18>;
    "Raw19": FixedSizeBinary<19>;
    "Raw20": FixedSizeBinary<20>;
    "Raw21": FixedSizeBinary<21>;
    "Raw22": FixedSizeBinary<22>;
    "Raw23": FixedSizeBinary<23>;
    "Raw24": FixedSizeBinary<24>;
    "Raw25": FixedSizeBinary<25>;
    "Raw26": FixedSizeBinary<26>;
    "Raw27": FixedSizeBinary<27>;
    "Raw28": FixedSizeBinary<28>;
    "Raw29": FixedSizeBinary<29>;
    "Raw30": FixedSizeBinary<30>;
    "Raw31": FixedSizeBinary<31>;
    "Raw32": FixedSizeBinary<32>;
    "Raw33": FixedSizeBinary<33>;
    "Raw34": FixedSizeBinary<34>;
    "Raw35": FixedSizeBinary<35>;
    "Raw36": FixedSizeBinary<36>;
    "Raw37": FixedSizeBinary<37>;
    "Raw38": FixedSizeBinary<38>;
    "Raw39": FixedSizeBinary<39>;
    "Raw40": FixedSizeBinary<40>;
    "Raw41": FixedSizeBinary<41>;
    "Raw42": FixedSizeBinary<42>;
    "Raw43": FixedSizeBinary<43>;
    "Raw44": FixedSizeBinary<44>;
    "Raw45": FixedSizeBinary<45>;
    "Raw46": FixedSizeBinary<46>;
    "Raw47": FixedSizeBinary<47>;
    "Raw48": FixedSizeBinary<48>;
    "Raw49": FixedSizeBinary<49>;
    "Raw50": FixedSizeBinary<50>;
    "Raw51": FixedSizeBinary<51>;
    "Raw52": FixedSizeBinary<52>;
    "Raw53": FixedSizeBinary<53>;
    "Raw54": FixedSizeBinary<54>;
    "Raw55": FixedSizeBinary<55>;
    "Raw56": FixedSizeBinary<56>;
    "Raw57": FixedSizeBinary<57>;
    "Raw58": FixedSizeBinary<58>;
    "Raw59": FixedSizeBinary<59>;
    "Raw60": FixedSizeBinary<60>;
    "Raw61": FixedSizeBinary<61>;
    "Raw62": FixedSizeBinary<62>;
    "Raw63": FixedSizeBinary<63>;
    "Raw64": FixedSizeBinary<64>;
    "BlakeTwo256": FixedSizeBinary<32>;
    "Sha256": FixedSizeBinary<32>;
    "Keccak256": FixedSizeBinary<32>;
    "ShaThree256": FixedSizeBinary<32>;
}>;
export type If7b8240vgt2q5 = (FixedSizeBinary<20>) | undefined;
export type I7nkl7ntqohel8 = Array<Anonymize<I7svnfko10tq2e>>;
export type I3m6d7ohcp5n4v = {
    "deposit": bigint;
    "block": number;
    "info": Anonymize<I4122t6tpcniur>;
};
export type I4122t6tpcniur = Array<Enum<{
    "None": undefined;
    "Raw0": undefined;
    "Raw1": number;
    "Raw2": FixedSizeBinary<2>;
    "Raw3": FixedSizeBinary<3>;
    "Raw4": FixedSizeBinary<4>;
    "Raw5": FixedSizeBinary<5>;
    "Raw6": FixedSizeBinary<6>;
    "Raw7": FixedSizeBinary<7>;
    "Raw8": FixedSizeBinary<8>;
    "Raw9": FixedSizeBinary<9>;
    "Raw10": FixedSizeBinary<10>;
    "Raw11": FixedSizeBinary<11>;
    "Raw12": FixedSizeBinary<12>;
    "Raw13": FixedSizeBinary<13>;
    "Raw14": FixedSizeBinary<14>;
    "Raw15": FixedSizeBinary<15>;
    "Raw16": FixedSizeBinary<16>;
    "Raw17": FixedSizeBinary<17>;
    "Raw18": FixedSizeBinary<18>;
    "Raw19": FixedSizeBinary<19>;
    "Raw20": FixedSizeBinary<20>;
    "Raw21": FixedSizeBinary<21>;
    "Raw22": FixedSizeBinary<22>;
    "Raw23": FixedSizeBinary<23>;
    "Raw24": FixedSizeBinary<24>;
    "Raw25": FixedSizeBinary<25>;
    "Raw26": FixedSizeBinary<26>;
    "Raw27": FixedSizeBinary<27>;
    "Raw28": FixedSizeBinary<28>;
    "Raw29": FixedSizeBinary<29>;
    "Raw30": FixedSizeBinary<30>;
    "Raw31": FixedSizeBinary<31>;
    "Raw32": FixedSizeBinary<32>;
    "Raw33": FixedSizeBinary<33>;
    "Raw34": FixedSizeBinary<34>;
    "Raw35": FixedSizeBinary<35>;
    "Raw36": FixedSizeBinary<36>;
    "Raw37": FixedSizeBinary<37>;
    "Raw38": FixedSizeBinary<38>;
    "Raw39": FixedSizeBinary<39>;
    "Raw40": FixedSizeBinary<40>;
    "Raw41": FixedSizeBinary<41>;
    "Raw42": FixedSizeBinary<42>;
    "Raw43": FixedSizeBinary<43>;
    "Raw44": FixedSizeBinary<44>;
    "Raw45": FixedSizeBinary<45>;
    "Raw46": FixedSizeBinary<46>;
    "Raw47": FixedSizeBinary<47>;
    "Raw48": FixedSizeBinary<48>;
    "Raw49": FixedSizeBinary<49>;
    "Raw50": FixedSizeBinary<50>;
    "Raw51": FixedSizeBinary<51>;
    "Raw52": FixedSizeBinary<52>;
    "Raw53": FixedSizeBinary<53>;
    "Raw54": FixedSizeBinary<54>;
    "Raw55": FixedSizeBinary<55>;
    "Raw56": FixedSizeBinary<56>;
    "Raw57": FixedSizeBinary<57>;
    "Raw58": FixedSizeBinary<58>;
    "Raw59": FixedSizeBinary<59>;
    "Raw60": FixedSizeBinary<60>;
    "Raw61": FixedSizeBinary<61>;
    "Raw62": FixedSizeBinary<62>;
    "Raw63": FixedSizeBinary<63>;
    "Raw64": FixedSizeBinary<64>;
    "Raw65": FixedSizeBinary<65>;
    "Raw66": FixedSizeBinary<66>;
    "Raw67": FixedSizeBinary<67>;
    "Raw68": FixedSizeBinary<68>;
    "Raw69": FixedSizeBinary<69>;
    "Raw70": FixedSizeBinary<70>;
    "Raw71": FixedSizeBinary<71>;
    "Raw72": FixedSizeBinary<72>;
    "Raw73": FixedSizeBinary<73>;
    "Raw74": FixedSizeBinary<74>;
    "Raw75": FixedSizeBinary<75>;
    "Raw76": FixedSizeBinary<76>;
    "Raw77": FixedSizeBinary<77>;
    "Raw78": FixedSizeBinary<78>;
    "Raw79": FixedSizeBinary<79>;
    "Raw80": FixedSizeBinary<80>;
    "Raw81": FixedSizeBinary<81>;
    "Raw82": FixedSizeBinary<82>;
    "Raw83": FixedSizeBinary<83>;
    "Raw84": FixedSizeBinary<84>;
    "Raw85": FixedSizeBinary<85>;
    "Raw86": FixedSizeBinary<86>;
    "Raw87": FixedSizeBinary<87>;
    "Raw88": FixedSizeBinary<88>;
    "Raw89": FixedSizeBinary<89>;
    "Raw90": FixedSizeBinary<90>;
    "Raw91": FixedSizeBinary<91>;
    "Raw92": FixedSizeBinary<92>;
    "Raw93": FixedSizeBinary<93>;
    "Raw94": FixedSizeBinary<94>;
    "Raw95": FixedSizeBinary<95>;
    "Raw96": FixedSizeBinary<96>;
    "Raw97": FixedSizeBinary<97>;
    "Raw98": FixedSizeBinary<98>;
    "Raw99": FixedSizeBinary<99>;
    "Raw100": FixedSizeBinary<100>;
    "Raw101": FixedSizeBinary<101>;
    "Raw102": FixedSizeBinary<102>;
    "Raw103": FixedSizeBinary<103>;
    "Raw104": FixedSizeBinary<104>;
    "Raw105": FixedSizeBinary<105>;
    "Raw106": FixedSizeBinary<106>;
    "Raw107": FixedSizeBinary<107>;
    "Raw108": FixedSizeBinary<108>;
    "Raw109": FixedSizeBinary<109>;
    "Raw110": FixedSizeBinary<110>;
    "Raw111": FixedSizeBinary<111>;
    "Raw112": FixedSizeBinary<112>;
    "Raw113": FixedSizeBinary<113>;
    "Raw114": FixedSizeBinary<114>;
    "Raw115": FixedSizeBinary<115>;
    "Raw116": FixedSizeBinary<116>;
    "Raw117": FixedSizeBinary<117>;
    "Raw118": FixedSizeBinary<118>;
    "Raw119": FixedSizeBinary<119>;
    "Raw120": FixedSizeBinary<120>;
    "Raw121": FixedSizeBinary<121>;
    "Raw122": FixedSizeBinary<122>;
    "Raw123": FixedSizeBinary<123>;
    "Raw124": FixedSizeBinary<124>;
    "Raw125": FixedSizeBinary<125>;
    "Raw126": FixedSizeBinary<126>;
    "Raw127": FixedSizeBinary<127>;
    "Raw128": FixedSizeBinary<128>;
    "BlakeTwo256": FixedSizeBinary<32>;
    "Sha256": FixedSizeBinary<32>;
    "Keccak256": FixedSizeBinary<32>;
    "ShaThree256": FixedSizeBinary<32>;
    "TimelockEncrypted": {
        "encrypted": Binary;
        "reveal_round": bigint;
    };
    "ResetBondsFlag": undefined;
    "BigRaw": Binary;
}>>;
export type Ib9pv5dg6upo6t = Array<[Binary, bigint]>;
export type I27ub49plcvb4c = {
    "last_epoch": bigint;
    "used_space": bigint;
};
export type Ic3l568el19b24 = [Anonymize<Ibjuap2vk03rp6>, Anonymize<Ifoernv5r40rfc>, Anonymize<Ideko6oeomboa6>];
export type Ibjuap2vk03rp6 = AnonymousEnum<{
    "Legacy": {
        "nonce": Anonymize<I4totqt881mlti>;
        "gas_price": Anonymize<I4totqt881mlti>;
        "gas_limit": Anonymize<I4totqt881mlti>;
        "action": Anonymize<I2do93a3gr3ege>;
        "value": Anonymize<I4totqt881mlti>;
        "input": Binary;
        "signature": {
            "v": bigint;
            "r": FixedSizeBinary<32>;
            "s": FixedSizeBinary<32>;
        };
    };
    "EIP2930": {
        "chain_id": bigint;
        "nonce": Anonymize<I4totqt881mlti>;
        "gas_price": Anonymize<I4totqt881mlti>;
        "gas_limit": Anonymize<I4totqt881mlti>;
        "action": Anonymize<I2do93a3gr3ege>;
        "value": Anonymize<I4totqt881mlti>;
        "input": Binary;
        "access_list": Anonymize<Ieap15h2pjii9u>;
        "signature": Anonymize<I9veufneid0sta>;
    };
    "EIP1559": {
        "chain_id": bigint;
        "nonce": Anonymize<I4totqt881mlti>;
        "max_priority_fee_per_gas": Anonymize<I4totqt881mlti>;
        "max_fee_per_gas": Anonymize<I4totqt881mlti>;
        "gas_limit": Anonymize<I4totqt881mlti>;
        "action": Anonymize<I2do93a3gr3ege>;
        "value": Anonymize<I4totqt881mlti>;
        "input": Binary;
        "access_list": Anonymize<Ieap15h2pjii9u>;
        "signature": Anonymize<I9veufneid0sta>;
    };
    "EIP7702": {
        "chain_id": bigint;
        "nonce": Anonymize<I4totqt881mlti>;
        "max_priority_fee_per_gas": Anonymize<I4totqt881mlti>;
        "max_fee_per_gas": Anonymize<I4totqt881mlti>;
        "gas_limit": Anonymize<I4totqt881mlti>;
        "destination": Anonymize<I2do93a3gr3ege>;
        "value": Anonymize<I4totqt881mlti>;
        "data": Binary;
        "access_list": Anonymize<Ieap15h2pjii9u>;
        "authorization_list": Anonymize<Idg0qi60379vnh>;
        "signature": Anonymize<I9veufneid0sta>;
    };
}>;
export type I2do93a3gr3ege = AnonymousEnum<{
    "Call": FixedSizeBinary<20>;
    "Create": undefined;
}>;
export type Ieap15h2pjii9u = Array<{
    "address": FixedSizeBinary<20>;
    "storage_keys": Anonymize<Ic5m5lp1oioo8r>;
}>;
export type I9veufneid0sta = {
    "odd_y_parity": boolean;
    "r": FixedSizeBinary<32>;
    "s": FixedSizeBinary<32>;
};
export type Idg0qi60379vnh = Array<{
    "chain_id": bigint;
    "address": FixedSizeBinary<20>;
    "nonce": Anonymize<I4totqt881mlti>;
    "signature": Anonymize<I9veufneid0sta>;
}>;
export type Ifoernv5r40rfc = {
    "transaction_hash": FixedSizeBinary<32>;
    "transaction_index": number;
    "from": FixedSizeBinary<20>;
    "to"?: Anonymize<If7b8240vgt2q5>;
    "contract_address"?: Anonymize<If7b8240vgt2q5>;
    "logs": Anonymize<Ids7ng2qsv7snu>;
    "logs_bloom": FixedSizeBinary<256>;
};
export type Ids7ng2qsv7snu = Array<Anonymize<I10qb03fpuk6em>>;
export type Ideko6oeomboa6 = AnonymousEnum<{
    "Legacy": {
        "status_code": number;
        "used_gas": Anonymize<I4totqt881mlti>;
        "logs_bloom": FixedSizeBinary<256>;
        "logs": Anonymize<Ids7ng2qsv7snu>;
    };
    "EIP2930": {
        "status_code": number;
        "used_gas": Anonymize<I4totqt881mlti>;
        "logs_bloom": FixedSizeBinary<256>;
        "logs": Anonymize<Ids7ng2qsv7snu>;
    };
    "EIP1559": {
        "status_code": number;
        "used_gas": Anonymize<I4totqt881mlti>;
        "logs_bloom": FixedSizeBinary<256>;
        "logs": Anonymize<Ids7ng2qsv7snu>;
    };
    "EIP7702": {
        "status_code": number;
        "used_gas": Anonymize<I4totqt881mlti>;
        "logs_bloom": FixedSizeBinary<256>;
        "logs": Anonymize<Ids7ng2qsv7snu>;
    };
}>;
export type Ib0hfhkohlekcj = {
    "header": Anonymize<I4v962mnhj6j6r>;
    "transactions": Anonymize<Ie30stbbeaul1o>;
    "ommers": Array<Anonymize<I4v962mnhj6j6r>>;
};
export type I4v962mnhj6j6r = {
    "parent_hash": FixedSizeBinary<32>;
    "ommers_hash": FixedSizeBinary<32>;
    "beneficiary": FixedSizeBinary<20>;
    "state_root": FixedSizeBinary<32>;
    "transactions_root": FixedSizeBinary<32>;
    "receipts_root": FixedSizeBinary<32>;
    "logs_bloom": FixedSizeBinary<256>;
    "difficulty": Anonymize<I4totqt881mlti>;
    "number": Anonymize<I4totqt881mlti>;
    "gas_limit": Anonymize<I4totqt881mlti>;
    "gas_used": Anonymize<I4totqt881mlti>;
    "timestamp": bigint;
    "extra_data": Binary;
    "mix_hash": FixedSizeBinary<32>;
    "nonce": FixedSizeBinary<8>;
};
export type Ie30stbbeaul1o = Array<Anonymize<Ibjuap2vk03rp6>>;
export type I32lgu058i52q9 = Array<Anonymize<Ideko6oeomboa6>>;
export type Ie7atdsih6q14b = Array<Anonymize<Ifoernv5r40rfc>>;
export type I7jidl7qnnq87c = {
    "size": bigint;
    "hash": FixedSizeBinary<32>;
};
export type I82cps8ng2jtug = [FixedSizeBinary<20>, FixedSizeBinary<32>];
export type I4gqmlq9k6jlk3 = Array<FixedSizeBinary<20>>;
export type I494mq1ertfc9k = {
    "public_key": Binary;
    "period": number;
    "genesis_time": number;
    "hash": Binary;
    "group_hash": Binary;
    "scheme_id": Binary;
    "metadata": Binary;
};
export type Ialchst9lgd11u = {
    "round": bigint;
    "randomness": Binary;
    "signature": Binary;
};
export type If0p9hvn3kegj1 = {
    "creator": SS58String;
    "deposit": bigint;
    "min_contribution": bigint;
    "end": number;
    "cap": bigint;
    "funds_account": SS58String;
    "raised": bigint;
    "target_address"?: Anonymize<Ihfphjolmsqq1>;
    "call"?: (PreimagesBounded) | undefined;
    "finalized": boolean;
    "contributors_count": number;
};
export type I8ac0r18acljm6 = {
    "liquidity_net": bigint;
    "liquidity_gross": bigint;
    "fees_out_tao": bigint;
    "fees_out_alpha": bigint;
};
export type I5mi4kb05lrsa9 = {
    "id": bigint;
    "netuid": number;
    "tick_low": number;
    "tick_high": number;
    "liquidity": bigint;
    "fees_tao": bigint;
    "fees_alpha": bigint;
};
export type Icsknfl0f6r973 = [number, SS58String, bigint];
export type I1ptic1rnhda0n = [number, Enum<{
    "Top": undefined;
    "Middle": undefined;
    "Bottom": undefined;
}>, number];
export type I5kulbesqc1h1t = {
    "owner": SS58String;
    "deposit": bigint;
    "refcount": bigint;
    "determinism": Anonymize<I2dfliekq1ed7e>;
    "code_len": number;
};
export type I2dfliekq1ed7e = AnonymousEnum<{
    "Enforced": undefined;
    "Relaxed": undefined;
}>;
export type I36dvimehsh2tm = {
    "trie_id": Binary;
    "code_hash": FixedSizeBinary<32>;
    "storage_bytes": number;
    "storage_items": number;
    "storage_byte_deposit": bigint;
    "storage_item_deposit": bigint;
    "storage_base_deposit": bigint;
    "delegate_dependencies": Anonymize<I3geksg000c171>;
};
export type I8t4pajubp34g3 = {
    "insert_counter": number;
    "delete_counter": number;
};
export type Ifdiflqufkknl8 = {
    "author": SS58String;
    "commitment": FixedSizeBinary<32>;
    "ciphertext": Binary;
    "submitted_in": number;
};
export type In7a38730s6qs = {
    "base_block": Anonymize<I4q39t5hn830vp>;
    "max_block": Anonymize<I4q39t5hn830vp>;
    "per_class": {
        "normal": {
            "base_extrinsic": Anonymize<I4q39t5hn830vp>;
            "max_extrinsic"?: Anonymize<Iasb8k6ash5mjn>;
            "max_total"?: Anonymize<Iasb8k6ash5mjn>;
            "reserved"?: Anonymize<Iasb8k6ash5mjn>;
        };
        "operational": {
            "base_extrinsic": Anonymize<I4q39t5hn830vp>;
            "max_extrinsic"?: Anonymize<Iasb8k6ash5mjn>;
            "max_total"?: Anonymize<Iasb8k6ash5mjn>;
            "reserved"?: Anonymize<Iasb8k6ash5mjn>;
        };
        "mandatory": {
            "base_extrinsic": Anonymize<I4q39t5hn830vp>;
            "max_extrinsic"?: Anonymize<Iasb8k6ash5mjn>;
            "max_total"?: Anonymize<Iasb8k6ash5mjn>;
            "reserved"?: Anonymize<Iasb8k6ash5mjn>;
        };
    };
};
export type If15el53dd76v9 = {
    "normal": number;
    "operational": number;
    "mandatory": number;
};
export type I9s0ave7t0vnrk = {
    "read": bigint;
    "write": bigint;
};
export type I4fo08joqmcqnm = {
    "spec_name": string;
    "impl_name": string;
    "authoring_version": number;
    "spec_version": number;
    "impl_version": number;
    "apis": Array<[FixedSizeBinary<8>, number]>;
    "transaction_version": number;
    "system_version": number;
};
export type I35p85j063s0il = (bigint) | undefined;
export type Ijc5n210o8bbf = {
    "limits": {
        "event_topics": number;
        "memory_pages": number;
        "subject_len": number;
        "payload_len": number;
        "runtime_memory": number;
        "validator_runtime_memory": number;
        "event_ref_time": bigint;
    };
    "instruction_weights": number;
};
export type I3m5sq54sjdlso = {};
export type Iekve0i6djpd9f = AnonymousEnum<{
    /**
     * Make some on-chain remark.
     *
     * Can be executed by every `origin`.
     */
    "remark": Anonymize<I8ofcg5rbj0g2c>;
    /**
     * Set the number of pages in the WebAssembly environment's heap.
     */
    "set_heap_pages": Anonymize<I4adgbll7gku4i>;
    /**
     * Set the new runtime code.
     */
    "set_code": Anonymize<I6pjjpfvhvcfru>;
    /**
     * Set the new runtime code without doing any checks of the given `code`.
     *
     * Note that runtime upgrades will not run if this is called with a not-increasing spec
     * version!
     */
    "set_code_without_checks": Anonymize<I6pjjpfvhvcfru>;
    /**
     * Set some items of storage.
     */
    "set_storage": Anonymize<I9pj91mj79qekl>;
    /**
     * Kill some items from storage.
     */
    "kill_storage": Anonymize<I39uah9nss64h9>;
    /**
     * Kill all storage items with a key that starts with the given prefix.
     *
     * **NOTE:** We rely on the Root origin to provide us the number of subkeys under
     * the prefix we are removing to accurately calculate the weight of this function.
     */
    "kill_prefix": Anonymize<Ik64dknsq7k08>;
    /**
     * Make some on-chain remark and emit event.
     */
    "remark_with_event": Anonymize<I8ofcg5rbj0g2c>;
    /**
     * Authorize an upgrade to a given `code_hash` for the runtime. The runtime can be supplied
     * later.
     *
     * This call requires Root origin.
     */
    "authorize_upgrade": Anonymize<Ib51vk42m1po4n>;
    /**
     * Authorize an upgrade to a given `code_hash` for the runtime. The runtime can be supplied
     * later.
     *
     * WARNING: This authorizes an upgrade that will take place without any safety checks, for
     * example that the spec name remains the same and that the version number increases. Not
     * recommended for normal use. Use `authorize_upgrade` instead.
     *
     * This call requires Root origin.
     */
    "authorize_upgrade_without_checks": Anonymize<Ib51vk42m1po4n>;
    /**
     * Provide the preimage (runtime binary) `code` for an upgrade that has been authorized.
     *
     * If the authorization required a version check, this call will ensure the spec name
     * remains unchanged and that the spec version has increased.
     *
     * Depending on the runtime's `OnSetCode` configuration, this function may directly apply
     * the new `code` in the same block or attempt to schedule the upgrade.
     *
     * All origins are allowed.
     */
    "apply_authorized_upgrade": Anonymize<I6pjjpfvhvcfru>;
}>;
export type I8ofcg5rbj0g2c = {
    "remark": Binary;
};
export type I4adgbll7gku4i = {
    "pages": bigint;
};
export type I6pjjpfvhvcfru = {
    "code": Binary;
};
export type I9pj91mj79qekl = {
    "items": Array<FixedSizeArray<2, Binary>>;
};
export type I39uah9nss64h9 = {
    "keys": Anonymize<Itom7fk49o0c9>;
};
export type Itom7fk49o0c9 = Array<Binary>;
export type Ik64dknsq7k08 = {
    "prefix": Binary;
    "subkeys": number;
};
export type Ib51vk42m1po4n = {
    "code_hash": FixedSizeBinary<32>;
};
export type I7d75gqfg6jh9c = AnonymousEnum<{
    /**
     * Set the current time.
     *
     * This call should be invoked exactly once per block. It will panic at the finalization
     * phase, if this call hasn't been invoked by that time.
     *
     * The timestamp should be greater than the previous one by the amount specified by
     * [`Config::MinimumPeriod`].
     *
     * The dispatch origin for this call must be _None_.
     *
     * This dispatch class is _Mandatory_ to ensure it gets executed in the block. Be aware
     * that changing the complexity of this call could result exhausting the resources in a
     * block to execute any other calls.
     *
     * ## Complexity
     * - `O(1)` (Note that implementations of `OnTimestampSet` must also be `O(1)`)
     * - 1 storage read and 1 storage mutation (codec `O(1)` because of `DidUpdate::take` in
     * `on_finalize`)
     * - 1 event handler `on_timestamp_set`. Must be `O(1)`.
     */
    "set": Anonymize<Idcr6u6361oad9>;
}>;
export type Idcr6u6361oad9 = {
    "now": bigint;
};
export type Ibck9ekr2i96uj = AnonymousEnum<{
    /**
     * Report voter equivocation/misbehavior. This method will verify the
     * equivocation proof and validate the given key ownership proof
     * against the extracted offender. If both are valid, the offence
     * will be reported.
     */
    "report_equivocation": Anonymize<I3a5kuu5t5jj3g>;
    /**
     * Report voter equivocation/misbehavior. This method will verify the
     * equivocation proof and validate the given key ownership proof
     * against the extracted offender. If both are valid, the offence
     * will be reported.
     *
     * This extrinsic must be called unsigned and it is expected that only
     * block authors will call it (validated in `ValidateUnsigned`), as such
     * if the block author is defined it will be defined as the equivocation
     * reporter.
     */
    "report_equivocation_unsigned": Anonymize<I3a5kuu5t5jj3g>;
    /**
     * Note that the current authority set of the GRANDPA finality gadget has stalled.
     *
     * This will trigger a forced authority set change at the beginning of the next session, to
     * be enacted `delay` blocks after that. The `delay` should be high enough to safely assume
     * that the block signalling the forced change will not be re-orged e.g. 1000 blocks.
     * The block production rate (which may be slowed down because of finality lagging) should
     * be taken into account when choosing the `delay`. The GRANDPA voters based on the new
     * authority will start voting on top of `best_finalized_block_number` for new finalized
     * blocks. `best_finalized_block_number` should be the highest of the latest finalized
     * block of all validators of the new authority set.
     *
     * Only callable by root.
     */
    "note_stalled": Anonymize<I2hviml3snvhhn>;
}>;
export type I3a5kuu5t5jj3g = {
    "equivocation_proof": Anonymize<I9puqgoda8ofk4>;
};
export type I9puqgoda8ofk4 = {
    "set_id": bigint;
    "equivocation": GrandpaEquivocation;
};
export type GrandpaEquivocation = Enum<{
    "Prevote": {
        "round_number": bigint;
        "identity": FixedSizeBinary<32>;
        "first": [{
            "target_hash": FixedSizeBinary<32>;
            "target_number": number;
        }, FixedSizeBinary<64>];
        "second": [{
            "target_hash": FixedSizeBinary<32>;
            "target_number": number;
        }, FixedSizeBinary<64>];
    };
    "Precommit": {
        "round_number": bigint;
        "identity": FixedSizeBinary<32>;
        "first": [{
            "target_hash": FixedSizeBinary<32>;
            "target_number": number;
        }, FixedSizeBinary<64>];
        "second": [{
            "target_hash": FixedSizeBinary<32>;
            "target_number": number;
        }, FixedSizeBinary<64>];
    };
}>;
export declare const GrandpaEquivocation: GetEnum<GrandpaEquivocation>;
export type I2hviml3snvhhn = {
    "delay": number;
    "best_finalized_block_number": number;
};
export type I9svldsp29mh87 = AnonymousEnum<{
    /**
     * Transfer some liquid free balance to another account.
     *
     * `transfer_allow_death` will set the `FreeBalance` of the sender and receiver.
     * If the sender's account is below the existential deposit as a result
     * of the transfer, the account will be reaped.
     *
     * The dispatch origin for this call must be `Signed` by the transactor.
     */
    "transfer_allow_death": Anonymize<I4ktuaksf5i1gk>;
    /**
     * Exactly as `transfer_allow_death`, except the origin must be root and the source account
     * may be specified.
     */
    "force_transfer": Anonymize<I9bqtpv2ii35mp>;
    /**
     * Same as the [`transfer_allow_death`] call, but with a check that the transfer will not
     * kill the origin account.
     *
     * 99% of the time you want [`transfer_allow_death`] instead.
     *
     * [`transfer_allow_death`]: struct.Pallet.html#method.transfer
     */
    "transfer_keep_alive": Anonymize<I4ktuaksf5i1gk>;
    /**
     * Transfer the entire transferable balance from the caller account.
     *
     * NOTE: This function only attempts to transfer _transferable_ balances. This means that
     * any locked, reserved, or existential deposits (when `keep_alive` is `true`), will not be
     * transferred by this function. To ensure that this function results in a killed account,
     * you might need to prepare the account by removing any reference counters, storage
     * deposits, etc...
     *
     * The dispatch origin of this call must be Signed.
     *
     * - `dest`: The recipient of the transfer.
     * - `keep_alive`: A boolean to determine if the `transfer_all` operation should send all
     * of the funds the account has, causing the sender account to be killed (false), or
     * transfer everything except at least the existential deposit, which will guarantee to
     * keep the sender account alive (true).
     */
    "transfer_all": Anonymize<I9j7pagd6d4bda>;
    /**
     * Unreserve some balance from a user by force.
     *
     * Can only be called by ROOT.
     */
    "force_unreserve": Anonymize<I2h9pmio37r7fb>;
    /**
     * Upgrade a specified account.
     *
     * - `origin`: Must be `Signed`.
     * - `who`: The account to be upgraded.
     *
     * This will waive the transaction fee if at least all but 10% of the accounts needed to
     * be upgraded. (We let some not have to be upgraded just in order to allow for the
     * possibility of churn).
     */
    "upgrade_accounts": Anonymize<Ibmr18suc9ikh9>;
    /**
     * Set the regular balance of a given account.
     *
     * The dispatch origin for this call is `root`.
     */
    "force_set_balance": Anonymize<I9iq22t0burs89>;
    /**
     * Adjust the total issuance in a saturating way.
     *
     * Can only be called by root and always needs a positive `delta`.
     *
     * # Example
     */
    "force_adjust_total_issuance": Anonymize<I5u8olqbbvfnvf>;
    /**
     * Burn the specified liquid free balance from the origin account.
     *
     * If the origin's account ends up below the existential deposit as a result
     * of the burn and `keep_alive` is false, the account will be reaped.
     *
     * Unlike sending funds to a _burn_ address, which merely makes the funds inaccessible,
     * this `burn` operation will reduce total issuance by the amount _burned_.
     */
    "burn": Anonymize<I5utcetro501ir>;
}>;
export type I4ktuaksf5i1gk = {
    "dest": MultiAddress;
    "value": bigint;
};
export type MultiAddress = Enum<{
    "Id": SS58String;
    "Index": undefined;
    "Raw": Binary;
    "Address32": FixedSizeBinary<32>;
    "Address20": FixedSizeBinary<20>;
}>;
export declare const MultiAddress: GetEnum<MultiAddress>;
export type I9bqtpv2ii35mp = {
    "source": MultiAddress;
    "dest": MultiAddress;
    "value": bigint;
};
export type I9j7pagd6d4bda = {
    "dest": MultiAddress;
    "keep_alive": boolean;
};
export type I2h9pmio37r7fb = {
    "who": MultiAddress;
    "amount": bigint;
};
export type Ibmr18suc9ikh9 = {
    "who": Anonymize<Ia2lhg7l2hilo3>;
};
export type I9iq22t0burs89 = {
    "who": MultiAddress;
    "new_free": bigint;
};
export type I5u8olqbbvfnvf = {
    "direction": BalancesAdjustmentDirection;
    "delta": bigint;
};
export type BalancesAdjustmentDirection = Enum<{
    "Increase": undefined;
    "Decrease": undefined;
}>;
export declare const BalancesAdjustmentDirection: GetEnum<BalancesAdjustmentDirection>;
export type I5utcetro501ir = {
    "value": bigint;
    "keep_alive": boolean;
};
export type I70bgb5j7tau9l = AnonymousEnum<{
    /**
     * --- Sets the caller weights for the incentive mechanism. The call can be
     * made from the hotkey account so is potentially insecure, however, the damage
     * of changing weights is minimal if caught early. This function includes all the
     * checks that the passed weights meet the requirements. Stored as u16s they represent
     * rational values in the range [0,1] which sum to 1 and can be interpreted as
     * probabilities. The specific weights determine how inflation propagates outward
     * from this peer.
     *
     * Note: The 16 bit integers weights should represent 1.0 as the max u16.
     * However, the function normalizes all integers to u16_max anyway. This means that if the sum of all
     * elements is larger or smaller than the amount of elements * u16_max, all elements
     * will be corrected for this deviation.
     *
     * # Args:
     * * `origin`: (<T as frame_system::Config>Origin):
     * - The caller, a hotkey who wishes to set their weights.
     *
     * * `netuid` (u16):
     * - The network uid we are setting these weights on.
     *
     * * `dests` (Vec<u16>):
     * - The edge endpoint for the weight, i.e. j for w_ij.
     *
     * * 'weights' (Vec<u16>):
     * - The u16 integer encoded weights. Interpreted as rational
     * values in the range [0,1]. They must sum to in32::MAX.
     *
     * * 'version_key' ( u64 ):
     * - The network version key to check if the validator is up to date.
     *
     * # Event:
     * * WeightsSet;
     * - On successfully setting the weights on chain.
     *
     * # Raises:
     * * 'MechanismDoesNotExist':
     * - Attempting to set weights on a non-existent network.
     *
     * * 'NotRegistered':
     * - Attempting to set weights from a non registered account.
     *
     * * 'WeightVecNotEqualSize':
     * - Attempting to set weights with uids not of same length.
     *
     * * 'DuplicateUids':
     * - Attempting to set weights with duplicate uids.
     *
     * * 'UidsLengthExceedUidsInSubNet':
     * - Attempting to set weights above the max allowed uids.
     *
     * * 'UidVecContainInvalidOne':
     * - Attempting to set weights with invalid uids.
     *
     * * 'WeightVecLengthIsLow':
     * - Attempting to set weights with fewer weights than min.
     *
     * * 'MaxWeightExceeded':
     * - Attempting to set weights with max value exceeding limit.
     */
    "set_weights": Anonymize<Icv6ofu4lqekr4>;
    /**
     * --- Sets the caller weights for the incentive mechanism for mechanisms. The call
     * can be made from the hotkey account so is potentially insecure, however, the damage
     * of changing weights is minimal if caught early. This function includes all the
     * checks that the passed weights meet the requirements. Stored as u16s they represent
     * rational values in the range [0,1] which sum to 1 and can be interpreted as
     * probabilities. The specific weights determine how inflation propagates outward
     * from this peer.
     *
     * Note: The 16 bit integers weights should represent 1.0 as the max u16.
     * However, the function normalizes all integers to u16_max anyway. This means that if the sum of all
     * elements is larger or smaller than the amount of elements * u16_max, all elements
     * will be corrected for this deviation.
     *
     * # Args:
     * * `origin`: (<T as frame_system::Config>Origin):
     * - The caller, a hotkey who wishes to set their weights.
     *
     * * `netuid` (u16):
     * - The network uid we are setting these weights on.
     *
     * * `mecid` (`u8`):
     * - The u8 mechnism identifier.
     *
     * * `dests` (Vec<u16>):
     * - The edge endpoint for the weight, i.e. j for w_ij.
     *
     * * 'weights' (Vec<u16>):
     * - The u16 integer encoded weights. Interpreted as rational
     * values in the range [0,1]. They must sum to in32::MAX.
     *
     * * 'version_key' ( u64 ):
     * - The network version key to check if the validator is up to date.
     *
     * # Event:
     * * WeightsSet;
     * - On successfully setting the weights on chain.
     *
     * # Raises:
     * * 'MechanismDoesNotExist':
     * - Attempting to set weights on a non-existent network.
     *
     * * 'NotRegistered':
     * - Attempting to set weights from a non registered account.
     *
     * * 'WeightVecNotEqualSize':
     * - Attempting to set weights with uids not of same length.
     *
     * * 'DuplicateUids':
     * - Attempting to set weights with duplicate uids.
     *
     * * 'UidsLengthExceedUidsInSubNet':
     * - Attempting to set weights above the max allowed uids.
     *
     * * 'UidVecContainInvalidOne':
     * - Attempting to set weights with invalid uids.
     *
     * * 'WeightVecLengthIsLow':
     * - Attempting to set weights with fewer weights than min.
     *
     * * 'MaxWeightExceeded':
     * - Attempting to set weights with max value exceeding limit.
     */
    "set_mechanism_weights": Anonymize<I48embv0n659kj>;
    /**
     * --- Allows a hotkey to set weights for multiple netuids as a batch.
     *
     * # Args:
     * * `origin`: (<T as frame_system::Config>Origin):
     * - The caller, a hotkey who wishes to set their weights.
     *
     * * `netuids` (Vec<Compact<u16>>):
     * - The network uids we are setting these weights on.
     *
     * * `weights` (Vec<Vec<(Compact<u16>, Compact<u16>)>):
     * - The weights to set for each network. [(uid, weight), ...]
     *
     * * `version_keys` (Vec<Compact<u64>>):
     * - The network version keys to check if the validator is up to date.
     *
     * # Event:
     * * WeightsSet;
     * - On successfully setting the weights on chain.
     * * BatchWeightsCompleted;
     * - On success of the batch.
     * * BatchCompletedWithErrors;
     * - On failure of any of the weights in the batch.
     * * BatchWeightItemFailed;
     * - On failure for each failed item in the batch.
     *
     */
    "batch_set_weights": Anonymize<I8l6dbd18t5aja>;
    /**
     * ---- Used to commit a hash of your weight values to later be revealed.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The signature of the committing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `commit_hash` (`H256`):
     * - The hash representing the committed weights.
     *
     * # Raises:
     * * `CommitRevealDisabled`:
     * - Attempting to commit when the commit-reveal mechanism is disabled.
     *
     * * `TooManyUnrevealedCommits`:
     * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
     *
     */
    "commit_weights": Anonymize<I513du23unvan>;
    /**
     * ---- Used to commit a hash of your weight values to later be revealed for mechanisms.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The signature of the committing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `mecid` (`u8`):
     * - The u8 mechanism identifier.
     *
     * * `commit_hash` (`H256`):
     * - The hash representing the committed weights.
     *
     * # Raises:
     * * `CommitRevealDisabled`:
     * - Attempting to commit when the commit-reveal mechanism is disabled.
     *
     * * `TooManyUnrevealedCommits`:
     * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
     *
     */
    "commit_mechanism_weights": Anonymize<I36o6oho99gjm8>;
    /**
     * --- Allows a hotkey to commit weight hashes for multiple netuids as a batch.
     *
     * # Args:
     * * `origin`: (<T as frame_system::Config>Origin):
     * - The caller, a hotkey who wishes to set their weights.
     *
     * * `netuids` (Vec<Compact<u16>>):
     * - The network uids we are setting these weights on.
     *
     * * `commit_hashes` (Vec<H256>):
     * - The commit hashes to commit.
     *
     * # Event:
     * * WeightsSet;
     * - On successfully setting the weights on chain.
     * * BatchWeightsCompleted;
     * - On success of the batch.
     * * BatchCompletedWithErrors;
     * - On failure of any of the weights in the batch.
     * * BatchWeightItemFailed;
     * - On failure for each failed item in the batch.
     *
     */
    "batch_commit_weights": Anonymize<If3mvus4cmnb7l>;
    /**
     * ---- Used to reveal the weights for a previously committed hash.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The signature of the revealing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `uids` (`Vec<u16>`):
     * - The uids for the weights being revealed.
     *
     * * `values` (`Vec<u16>`):
     * - The values of the weights being revealed.
     *
     * * `salt` (`Vec<u16>`):
     * - The salt used to generate the commit hash.
     *
     * * `version_key` (`u64`):
     * - The network version key.
     *
     * # Raises:
     * * `CommitRevealDisabled`:
     * - Attempting to reveal weights when the commit-reveal mechanism is disabled.
     *
     * * `NoWeightsCommitFound`:
     * - Attempting to reveal weights without an existing commit.
     *
     * * `ExpiredWeightCommit`:
     * - Attempting to reveal a weight commit that has expired.
     *
     * * `RevealTooEarly`:
     * - Attempting to reveal weights outside the valid reveal period.
     *
     * * `InvalidRevealCommitHashNotMatch`:
     * - The revealed hash does not match any committed hash.
     *
     */
    "reveal_weights": Anonymize<I3qrhi1ua10nnf>;
    /**
     * ---- Used to reveal the weights for a previously committed hash for mechanisms.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The signature of the revealing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `mecid` (`u8`):
     * - The u8 mechanism identifier.
     *
     * * `uids` (`Vec<u16>`):
     * - The uids for the weights being revealed.
     *
     * * `values` (`Vec<u16>`):
     * - The values of the weights being revealed.
     *
     * * `salt` (`Vec<u16>`):
     * - The salt used to generate the commit hash.
     *
     * * `version_key` (`u64`):
     * - The network version key.
     *
     * # Raises:
     * * `CommitRevealDisabled`:
     * - Attempting to reveal weights when the commit-reveal mechanism is disabled.
     *
     * * `NoWeightsCommitFound`:
     * - Attempting to reveal weights without an existing commit.
     *
     * * `ExpiredWeightCommit`:
     * - Attempting to reveal a weight commit that has expired.
     *
     * * `RevealTooEarly`:
     * - Attempting to reveal weights outside the valid reveal period.
     *
     * * `InvalidRevealCommitHashNotMatch`:
     * - The revealed hash does not match any committed hash.
     *
     */
    "reveal_mechanism_weights": Anonymize<I2hpc4ev2drsf2>;
    /**
     * ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The committing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `commit` (`Vec<u8>`):
     * - The encrypted compressed commit.
     * The steps for this are:
     * 1. Instantiate [`WeightsTlockPayload`]
     * 2. Serialize it using the `parity_scale_codec::Encode` trait
     * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
     * to produce a [`TLECiphertext<TinyBLS381>`] type.
     * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
     *
     * * reveal_round (`u64`):
     * - The drand reveal round which will be avaliable during epoch `n+1` from the current
     * epoch.
     *
     * # Raises:
     * * `CommitRevealV3Disabled`:
     * - Attempting to commit when the commit-reveal mechanism is disabled.
     *
     * * `TooManyUnrevealedCommits`:
     * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
     *
     * ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed for mechanisms.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The committing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `mecid` (`u8`):
     * - The u8 mechanism identifier.
     *
     * * `commit` (`Vec<u8>`):
     * - The encrypted compressed commit.
     * The steps for this are:
     * 1. Instantiate [`WeightsTlockPayload`]
     * 2. Serialize it using the `parity_scale_codec::Encode` trait
     * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
     * to produce a [`TLECiphertext<TinyBLS381>`] type.
     * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
     *
     * * reveal_round (`u64`):
     * - The drand reveal round which will be avaliable during epoch `n+1` from the current
     * epoch.
     *
     * # Raises:
     * * `CommitRevealV3Disabled`:
     * - Attempting to commit when the commit-reveal mechanism is disabled.
     *
     * * `TooManyUnrevealedCommits`:
     * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
     *
     */
    "commit_crv3_mechanism_weights": Anonymize<I73q6qh9ckhm04>;
    /**
     * ---- The implementation for batch revealing committed weights.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The signature of the revealing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `uids_list` (`Vec<Vec<u16>>`):
     * - A list of uids for each set of weights being revealed.
     *
     * * `values_list` (`Vec<Vec<u16>>`):
     * - A list of values for each set of weights being revealed.
     *
     * * `salts_list` (`Vec<Vec<u16>>`):
     * - A list of salts used to generate the commit hashes.
     *
     * * `version_keys` (`Vec<u64>`):
     * - A list of network version keys.
     *
     * # Raises:
     * * `CommitRevealDisabled`:
     * - Attempting to reveal weights when the commit-reveal mechanism is disabled.
     *
     * * `NoWeightsCommitFound`:
     * - Attempting to reveal weights without an existing commit.
     *
     * * `ExpiredWeightCommit`:
     * - Attempting to reveal a weight commit that has expired.
     *
     * * `RevealTooEarly`:
     * - Attempting to reveal weights outside the valid reveal period.
     *
     * * `InvalidRevealCommitHashNotMatch`:
     * - The revealed hash does not match any committed hash.
     *
     * * `InvalidInputLengths`:
     * - The input vectors are of mismatched lengths.
     */
    "batch_reveal_weights": Anonymize<Idia8cmqvul6et>;
    /**
     * --- Allows delegates to decrease its take value.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * 'hotkey' (T::AccountId):
     * - The hotkey we are delegating (must be owned by the coldkey.)
     *
     * * 'netuid' (u16):
     * - Subnet ID to decrease take for
     *
     * * 'take' (u16):
     * - The new stake proportion that this hotkey takes from delegations.
     * The new value can be between 0 and 11_796 and should be strictly
     * lower than the previous value. It T is the new value (rational number),
     * the the parameter is calculated as [65535 * T]. For example, 1% would be
     * [0.01 * 65535] = [655.35] = 655
     *
     * # Event:
     * * TakeDecreased;
     * - On successfully setting a decreased take for this hotkey.
     *
     * # Raises:
     * * 'NotRegistered':
     * - The hotkey we are delegating is not registered on the network.
     *
     * * 'NonAssociatedColdKey':
     * - The hotkey we are delegating is not owned by the calling coldkey.
     *
     * * 'DelegateTakeTooLow':
     * - The delegate is setting a take which is not lower than the previous.
     *
     */
    "decrease_take": Anonymize<Idardmhchnv8aa>;
    /**
     * --- Allows delegates to increase its take value. This call is rate-limited.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * 'hotkey' (T::AccountId):
     * - The hotkey we are delegating (must be owned by the coldkey.)
     *
     * * 'take' (u16):
     * - The new stake proportion that this hotkey takes from delegations.
     * The new value can be between 0 and 11_796 and should be strictly
     * greater than the previous value. T is the new value (rational number),
     * the the parameter is calculated as [65535 * T]. For example, 1% would be
     * [0.01 * 65535] = [655.35] = 655
     *
     * # Event:
     * * TakeIncreased;
     * - On successfully setting a increased take for this hotkey.
     *
     * # Raises:
     * * 'NotRegistered':
     * - The hotkey we are delegating is not registered on the network.
     *
     * * 'NonAssociatedColdKey':
     * - The hotkey we are delegating is not owned by the calling coldkey.
     *
     * * 'DelegateTakeTooHigh':
     * - The delegate is setting a take which is not greater than the previous.
     *
     */
    "increase_take": Anonymize<Idardmhchnv8aa>;
    /**
     * --- Adds stake to a hotkey. The call is made from a coldkey account.
     * This delegates stake to the hotkey.
     *
     * Note: the coldkey account may own the hotkey, in which case they are
     * delegating to themselves.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller's coldkey.
     *
     * * 'hotkey' (T::AccountId):
     * - The associated hotkey account.
     *
     * * 'netuid' (u16):
     * - Subnetwork UID
     *
     * * 'amount_staked' (u64):
     * - The amount of stake to be added to the hotkey staking account.
     *
     * # Event:
     * * StakeAdded;
     * - On the successfully adding stake to a global account.
     *
     * # Raises:
     * * 'NotEnoughBalanceToStake':
     * - Not enough balance on the coldkey to add onto the global account.
     *
     * * 'NonAssociatedColdKey':
     * - The calling coldkey is not associated with this hotkey.
     *
     * * 'BalanceWithdrawalError':
     * - Errors stemming from transaction pallet.
     *
     */
    "add_stake": Anonymize<Icud5m8j0nlgtj>;
    /**
     * Remove stake from the staking account. The call must be made
     * from the coldkey account attached to the neuron metadata. Only this key
     * has permission to make staking and unstaking requests.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller's coldkey.
     *
     * * 'hotkey' (T::AccountId):
     * - The associated hotkey account.
     *
     * * 'netuid' (u16):
     * - Subnetwork UID
     *
     * * 'amount_unstaked' (u64):
     * - The amount of stake to be added to the hotkey staking account.
     *
     * # Event:
     * * StakeRemoved;
     * - On the successfully removing stake from the hotkey account.
     *
     * # Raises:
     * * 'NotRegistered':
     * - Thrown if the account we are attempting to unstake from is non existent.
     *
     * * 'NonAssociatedColdKey':
     * - Thrown if the coldkey does not own the hotkey we are unstaking from.
     *
     * * 'NotEnoughStakeToWithdraw':
     * - Thrown if there is not enough stake on the hotkey to withdwraw this amount.
     *
     */
    "remove_stake": Anonymize<I850u7ir5o34um>;
    /**
     * Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is
     * already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller.
     *
     * * 'netuid' (u16):
     * - The u16 network identifier.
     *
     * * 'version' (u64):
     * - The bittensor version identifier.
     *
     * * 'ip' (u64):
     * - The endpoint ip information as a u128 encoded integer.
     *
     * * 'port' (u16):
     * - The endpoint port information as a u16 encoded integer.
     *
     * * 'ip_type' (u8):
     * - The endpoint ip version as a u8, 4 or 6.
     *
     * * 'protocol' (u8):
     * - UDP:1 or TCP:0
     *
     * * 'placeholder1' (u8):
     * - Placeholder for further extra params.
     *
     * * 'placeholder2' (u8):
     * - Placeholder for further extra params.
     *
     * # Event:
     * * AxonServed;
     * - On successfully serving the axon info.
     *
     * # Raises:
     * * 'MechanismDoesNotExist':
     * - Attempting to set weights on a non-existent network.
     *
     * * 'NotRegistered':
     * - Attempting to set weights from a non registered account.
     *
     * * 'InvalidIpType':
     * - The ip type is not 4 or 6.
     *
     * * 'InvalidIpAddress':
     * - The numerically encoded ip address does not resolve to a proper ip.
     *
     * * 'ServingRateLimitExceeded':
     * - Attempting to set prometheus information withing the rate limit min.
     *
     */
    "serve_axon": Anonymize<Ica88a899k1afk>;
    /**
     * Same as `serve_axon` but takes a certificate as an extra optional argument.
     * Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is
     * already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller.
     *
     * * 'netuid' (u16):
     * - The u16 network identifier.
     *
     * * 'version' (u64):
     * - The bittensor version identifier.
     *
     * * 'ip' (u64):
     * - The endpoint ip information as a u128 encoded integer.
     *
     * * 'port' (u16):
     * - The endpoint port information as a u16 encoded integer.
     *
     * * 'ip_type' (u8):
     * - The endpoint ip version as a u8, 4 or 6.
     *
     * * 'protocol' (u8):
     * - UDP:1 or TCP:0
     *
     * * 'placeholder1' (u8):
     * - Placeholder for further extra params.
     *
     * * 'placeholder2' (u8):
     * - Placeholder for further extra params.
     *
     * * 'certificate' (Vec<u8>):
     * - TLS certificate for inter neuron communitation.
     *
     * # Event:
     * * AxonServed;
     * - On successfully serving the axon info.
     *
     * # Raises:
     * * 'MechanismDoesNotExist':
     * - Attempting to set weights on a non-existent network.
     *
     * * 'NotRegistered':
     * - Attempting to set weights from a non registered account.
     *
     * * 'InvalidIpType':
     * - The ip type is not 4 or 6.
     *
     * * 'InvalidIpAddress':
     * - The numerically encoded ip address does not resolve to a proper ip.
     *
     * * 'ServingRateLimitExceeded':
     * - Attempting to set prometheus information withing the rate limit min.
     *
     */
    "serve_axon_tls": Anonymize<I4tfn6eb3ekqt2>;
    /**
     * ---- Set prometheus information for the neuron.
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the calling hotkey.
     *
     * * 'netuid' (u16):
     * - The u16 network identifier.
     *
     * * 'version' (u16):
     * -  The bittensor version identifier.
     *
     * * 'ip' (u128):
     * - The prometheus ip information as a u128 encoded integer.
     *
     * * 'port' (u16):
     * - The prometheus port information as a u16 encoded integer.
     *
     * * 'ip_type' (u8):
     * - The ip type v4 or v6.
     *
     */
    "serve_prometheus": Anonymize<Ia5r6mm7trbg6a>;
    /**
     * ---- Registers a new neuron to the subnetwork.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the calling hotkey.
     *
     * * 'netuid' (u16):
     * - The u16 network identifier.
     *
     * * 'block_number' ( u64 ):
     * - Block hash used to prove work done.
     *
     * * 'nonce' ( u64 ):
     * - Positive integer nonce used in POW.
     *
     * * 'work' ( Vec<u8> ):
     * - Vector encoded bytes representing work done.
     *
     * * 'hotkey' ( T::AccountId ):
     * - Hotkey to be registered to the network.
     *
     * * 'coldkey' ( T::AccountId ):
     * - Associated coldkey account.
     *
     * # Event:
     * * NeuronRegistered;
     * - On successfully registering a uid to a neuron slot on a subnetwork.
     *
     * # Raises:
     * * 'MechanismDoesNotExist':
     * - Attempting to register to a non existent network.
     *
     * * 'TooManyRegistrationsThisBlock':
     * - This registration exceeds the total allowed on this network this block.
     *
     * * 'HotKeyAlreadyRegisteredInSubNet':
     * - The hotkey is already registered on this network.
     *
     * * 'InvalidWorkBlock':
     * - The work has been performed on a stale, future, or non existent block.
     *
     * * 'InvalidDifficulty':
     * - The work does not match the difficulty.
     *
     * * 'InvalidSeal':
     * - The seal is incorrect.
     *
     */
    "register": Anonymize<I27gr0ss2ikvqh>;
    /**
     * Register the hotkey to root network
     */
    "root_register": Anonymize<Ie7hipi75c7vn0>;
    /**
     * User register a new subnetwork via burning token
     */
    "burned_register": Anonymize<I7f38r2vt6r9k1>;
    /**
     * The extrinsic for user to change its hotkey in subnet or all subnets.
     */
    "swap_hotkey": Anonymize<I6b53cjq4m9nsr>;
    /**
     * Performs an arbitrary coldkey swap for any coldkey.
     *
     * Only callable by root as it doesn't require an announcement and can be used to swap any coldkey.
     */
    "swap_coldkey": Anonymize<I216fvnrl9nq6l>;
    /**
     * Sets the childkey take for a given hotkey.
     *
     * This function allows a coldkey to set the childkey take for a given hotkey.
     * The childkey take determines the proportion of stake that the hotkey keeps for itself
     * when distributing stake to its children.
     *
     * # Arguments:
     * * `origin` (<T as frame_system::Config>::RuntimeOrigin):
     * - The signature of the calling coldkey. Setting childkey take can only be done by the coldkey.
     *
     * * `hotkey` (T::AccountId):
     * - The hotkey for which the childkey take will be set.
     *
     * * `take` (u16):
     * - The new childkey take value. This is a percentage represented as a value between 0 and 10000,
     * where 10000 represents 100%.
     *
     * # Events:
     * * `ChildkeyTakeSet`:
     * - On successfully setting the childkey take for a hotkey.
     *
     * # Errors:
     * * `NonAssociatedColdKey`:
     * - The coldkey does not own the hotkey.
     * * `InvalidChildkeyTake`:
     * - The provided take value is invalid (greater than the maximum allowed take).
     * * `TxChildkeyTakeRateLimitExceeded`:
     * - The rate limit for changing childkey take has been exceeded.
     *
     */
    "set_childkey_take": Anonymize<I9n4d52k0luroe>;
    /**
     * Sets the transaction rate limit for changing childkey take.
     *
     * This function can only be called by the root origin.
     *
     * # Arguments:
     * * `origin` - The origin of the call, must be root.
     * * `tx_rate_limit` - The new rate limit in blocks.
     *
     * # Errors:
     * * `BadOrigin` - If the origin is not root.
     *
     */
    "sudo_set_tx_childkey_take_rate_limit": Anonymize<I3gk6eeddm0hsd>;
    /**
     * Sets the minimum allowed childkey take.
     *
     * This function can only be called by the root origin.
     *
     * # Arguments:
     * * `origin` - The origin of the call, must be root.
     * * `take` - The new minimum childkey take value.
     *
     * # Errors:
     * * `BadOrigin` - If the origin is not root.
     *
     */
    "sudo_set_min_childkey_take": Anonymize<I6ue7qc27uhiev>;
    /**
     * Sets the maximum allowed childkey take.
     *
     * This function can only be called by the root origin.
     *
     * # Arguments:
     * * `origin` - The origin of the call, must be root.
     * * `take` - The new maximum childkey take value.
     *
     * # Errors:
     * * `BadOrigin` - If the origin is not root.
     *
     */
    "sudo_set_max_childkey_take": Anonymize<I6ue7qc27uhiev>;
    /**
     * User register a new subnetwork
     */
    "register_network": Anonymize<Ie7hipi75c7vn0>;
    /**
     * Facility extrinsic for user to get taken from faucet
     * It is only available when pow-faucet feature enabled
     * Just deployed in testnet and devnet for testing purpose
     */
    "faucet": Anonymize<Ifp8lgrkla2dig>;
    /**
     * Remove a user's subnetwork
     * The caller must be the owner of the network
     */
    "dissolve_network": Anonymize<I30l38oi9ed9dj>;
    /**
     * Set a single child for a given hotkey on a specified network.
     *
     * This function allows a coldkey to set a single child for a given hotkey on a specified network.
     * The proportion of the hotkey's stake to be allocated to the child is also specified.
     *
     * # Arguments:
     * * `origin` (<T as frame_system::Config>::RuntimeOrigin):
     * - The signature of the calling coldkey. Setting a hotkey child can only be done by the coldkey.
     *
     * * `hotkey` (T::AccountId):
     * - The hotkey which will be assigned the child.
     *
     * * `child` (T::AccountId):
     * - The child which will be assigned to the hotkey.
     *
     * * `netuid` (u16):
     * - The u16 network identifier where the childkey will exist.
     *
     * * `proportion` (u64):
     * - Proportion of the hotkey's stake to be given to the child, the value must be u64 normalized.
     *
     * # Events:
     * * `ChildAddedSingular`:
     * - On successfully registering a child to a hotkey.
     *
     * # Errors:
     * * `MechanismDoesNotExist`:
     * - Attempting to register to a non-existent network.
     * * `RegistrationNotPermittedOnRootSubnet`:
     * - Attempting to register a child on the root network.
     * * `NonAssociatedColdKey`:
     * - The coldkey does not own the hotkey or the child is the same as the hotkey.
     * * `HotKeyAccountNotExists`:
     * - The hotkey account does not exist.
     *
     * # Detailed Explanation of Checks:
     * 1. **Signature Verification**: Ensures that the caller has signed the transaction, verifying the coldkey.
     * 2. **Root Network Check**: Ensures that the delegation is not on the root network, as child hotkeys are not valid on the root.
     * 3. **Network Existence Check**: Ensures that the specified network exists.
     * 4. **Ownership Verification**: Ensures that the coldkey owns the hotkey.
     * 5. **Hotkey Account Existence Check**: Ensures that the hotkey account already exists.
     * 6. **Child-Hotkey Distinction**: Ensures that the child is not the same as the hotkey.
     * 7. **Old Children Cleanup**: Removes the hotkey from the parent list of its old children.
     * 8. **New Children Assignment**: Assigns the new child to the hotkey and updates the parent list for the new child.
     */
    "set_children": Anonymize<Ifj9gf4ekq9snm>;
    /**
     * Schedules a coldkey swap operation to be executed at a future block.
     *
     * WARNING: This function is deprecated, please migrate to `announce_coldkey_swap`/`coldkey_swap`
     */
    "schedule_swap_coldkey": Anonymize<If2k69ql8jgivj>;
    /**
     * ---- Set prometheus information for the neuron.
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the calling hotkey.
     *
     * * 'netuid' (u16):
     * - The u16 network identifier.
     *
     * * 'version' (u16):
     * -  The bittensor version identifier.
     *
     * * 'ip' (u128):
     * - The prometheus ip information as a u128 encoded integer.
     *
     * * 'port' (u16):
     * - The prometheus port information as a u16 encoded integer.
     *
     * * 'ip_type' (u8):
     * - The ip type v4 or v6.
     *
     */
    "set_identity": Anonymize<Ifjlj958aeheic>;
    /**
     * ---- Set the identity information for a subnet.
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the calling coldkey, which must be the owner of the subnet.
     *
     * * `netuid` (u16):
     * - The unique network identifier of the subnet.
     *
     * * `subnet_name` (Vec<u8>):
     * - The name of the subnet.
     *
     * * `github_repo` (Vec<u8>):
     * - The GitHub repository associated with the subnet identity.
     *
     * * `subnet_contact` (Vec<u8>):
     * - The contact information for the subnet.
     */
    "set_subnet_identity": Anonymize<I4378ieh1uba9u>;
    /**
     * User register a new subnetwork
     */
    "register_network_with_identity": Anonymize<I8e6f7r9dtk9c1>;
    /**
     * ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
     *
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * `hotkey` (T::AccountId):
     * - The associated hotkey account.
     *
     * # Event:
     * * StakeRemoved;
     * - On the successfully removing stake from the hotkey account.
     *
     * # Raises:
     * * `NotRegistered`:
     * - Thrown if the account we are attempting to unstake from is non existent.
     *
     * * `NonAssociatedColdKey`:
     * - Thrown if the coldkey does not own the hotkey we are unstaking from.
     *
     * * `NotEnoughStakeToWithdraw`:
     * - Thrown if there is not enough stake on the hotkey to withdraw this amount.
     *
     * * `TxRateLimitExceeded`:
     * - Thrown if key has hit transaction rate limit
     */
    "unstake_all": Anonymize<Ie7hipi75c7vn0>;
    /**
     * ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
     *
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * `hotkey` (T::AccountId):
     * - The associated hotkey account.
     *
     * # Event:
     * * StakeRemoved;
     * - On the successfully removing stake from the hotkey account.
     *
     * # Raises:
     * * `NotRegistered`:
     * - Thrown if the account we are attempting to unstake from is non existent.
     *
     * * `NonAssociatedColdKey`:
     * - Thrown if the coldkey does not own the hotkey we are unstaking from.
     *
     * * `NotEnoughStakeToWithdraw`:
     * - Thrown if there is not enough stake on the hotkey to withdraw this amount.
     *
     * * `TxRateLimitExceeded`:
     * - Thrown if key has hit transaction rate limit
     */
    "unstake_all_alpha": Anonymize<Ie7hipi75c7vn0>;
    /**
     * ---- The implementation for the extrinsic move_stake: Moves specified amount of stake from a hotkey to another across subnets.
     *
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * `origin_hotkey` (T::AccountId):
     * - The hotkey account to move stake from.
     *
     * * `destination_hotkey` (T::AccountId):
     * - The hotkey account to move stake to.
     *
     * * `origin_netuid` (T::AccountId):
     * - The subnet ID to move stake from.
     *
     * * `destination_netuid` (T::AccountId):
     * - The subnet ID to move stake to.
     *
     * * `alpha_amount` (T::AccountId):
     * - The alpha stake amount to move.
     *
     */
    "move_stake": Anonymize<I9d117ni3tprb>;
    /**
     * Transfers a specified amount of stake from one coldkey to another, optionally across subnets,
     * while keeping the same hotkey.
     *
     * # Arguments
     * * `origin` - The origin of the transaction, which must be signed by the `origin_coldkey`.
     * * `destination_coldkey` - The coldkey to which the stake is transferred.
     * * `hotkey` - The hotkey associated with the stake.
     * * `origin_netuid` - The network/subnet ID to move stake from.
     * * `destination_netuid` - The network/subnet ID to move stake to (for cross-subnet transfer).
     * * `alpha_amount` - The amount of stake to transfer.
     *
     * # Errors
     * Returns an error if:
     * * The origin is not signed by the correct coldkey.
     * * Either subnet does not exist.
     * * The hotkey does not exist.
     * * There is insufficient stake on `(origin_coldkey, hotkey, origin_netuid)`.
     * * The transfer amount is below the minimum stake requirement.
     *
     * # Events
     * May emit a `StakeTransferred` event on success.
     */
    "transfer_stake": Anonymize<I340k0hbj1hc6r>;
    /**
     * Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.
     *
     * # Arguments
     * * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
     * * `hotkey` - The hotkey whose stake is being swapped.
     * * `origin_netuid` - The network/subnet ID from which stake is removed.
     * * `destination_netuid` - The network/subnet ID to which stake is added.
     * * `alpha_amount` - The amount of stake to swap.
     *
     * # Errors
     * Returns an error if:
     * * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails).
     * * Either `origin_netuid` or `destination_netuid` does not exist.
     * * The hotkey does not exist.
     * * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`.
     * * The swap amount is below the minimum stake requirement.
     *
     * # Events
     * May emit a `StakeSwapped` event on success.
     */
    "swap_stake": Anonymize<Ibapoov2fa817a>;
    /**
     * --- Adds stake to a hotkey on a subnet with a price limit.
     * This extrinsic allows to specify the limit price for alpha token
     * at which or better (lower) the staking should execute.
     *
     * In case if slippage occurs and the price shall move beyond the limit
     * price, the staking order may execute only partially or not execute
     * at all.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller's coldkey.
     *
     * * 'hotkey' (T::AccountId):
     * - The associated hotkey account.
     *
     * * 'netuid' (u16):
     * - Subnetwork UID
     *
     * * 'amount_staked' (u64):
     * - The amount of stake to be added to the hotkey staking account.
     *
     * * 'limit_price' (u64):
     * - The limit price expressed in units of RAO per one Alpha.
     *
     * * 'allow_partial' (bool):
     * - Allows partial execution of the amount. If set to false, this becomes
     * fill or kill type or order.
     *
     * # Event:
     * * StakeAdded;
     * - On the successfully adding stake to a global account.
     *
     * # Raises:
     * * 'NotEnoughBalanceToStake':
     * - Not enough balance on the coldkey to add onto the global account.
     *
     * * 'NonAssociatedColdKey':
     * - The calling coldkey is not associated with this hotkey.
     *
     * * 'BalanceWithdrawalError':
     * - Errors stemming from transaction pallet.
     *
     */
    "add_stake_limit": Anonymize<I2eon60c4gde7f>;
    /**
     * --- Removes stake from a hotkey on a subnet with a price limit.
     * This extrinsic allows to specify the limit price for alpha token
     * at which or better (higher) the staking should execute.
     *
     * In case if slippage occurs and the price shall move beyond the limit
     * price, the staking order may execute only partially or not execute
     * at all.
     *
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller's coldkey.
     *
     * * 'hotkey' (T::AccountId):
     * - The associated hotkey account.
     *
     * * 'netuid' (u16):
     * - Subnetwork UID
     *
     * * 'amount_unstaked' (u64):
     * - The amount of stake to be added to the hotkey staking account.
     *
     * * 'limit_price' (u64):
     * - The limit price expressed in units of RAO per one Alpha.
     *
     * * 'allow_partial' (bool):
     * - Allows partial execution of the amount. If set to false, this becomes
     * fill or kill type or order.
     *
     * # Event:
     * * StakeRemoved;
     * - On the successfully removing stake from the hotkey account.
     *
     * # Raises:
     * * 'NotRegistered':
     * - Thrown if the account we are attempting to unstake from is non existent.
     *
     * * 'NonAssociatedColdKey':
     * - Thrown if the coldkey does not own the hotkey we are unstaking from.
     *
     * * 'NotEnoughStakeToWithdraw':
     * - Thrown if there is not enough stake on the hotkey to withdwraw this amount.
     *
     */
    "remove_stake_limit": Anonymize<I7egr0053sjpci>;
    /**
     * Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.
     *
     * # Arguments
     * * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
     * * `hotkey` - The hotkey whose stake is being swapped.
     * * `origin_netuid` - The network/subnet ID from which stake is removed.
     * * `destination_netuid` - The network/subnet ID to which stake is added.
     * * `alpha_amount` - The amount of stake to swap.
     * * `limit_price` - The limit price expressed in units of RAO per one Alpha.
     * * `allow_partial` - Allows partial execution of the amount. If set to false, this becomes fill or kill type or order.
     *
     * # Errors
     * Returns an error if:
     * * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails).
     * * Either `origin_netuid` or `destination_netuid` does not exist.
     * * The hotkey does not exist.
     * * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`.
     * * The swap amount is below the minimum stake requirement.
     *
     * # Events
     * May emit a `StakeSwapped` event on success.
     */
    "swap_stake_limit": Anonymize<I6r22p9usi2mkl>;
    /**
     * Attempts to associate a hotkey with a coldkey.
     *
     * # Arguments
     * * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
     * * `hotkey` - The hotkey to associate with the coldkey.
     *
     * # Note
     * Will charge based on the weight even if the hotkey is already associated with a coldkey.
     */
    "try_associate_hotkey": Anonymize<Ie7hipi75c7vn0>;
    /**
     * Initiates a call on a subnet.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be signed by the subnet owner.
     * * `netuid` - The unique identifier of the subnet on which the call is being initiated.
     *
     * # Events
     * Emits a `FirstEmissionBlockNumberSet` event on success.
     */
    "start_call": Anonymize<I6cm4c5a1euio9>;
    /**
     * Attempts to associate a hotkey with an EVM key.
     *
     * The signature will be checked to see if the recovered public key matches the `evm_key` provided.
     *
     * The EVM key is expected to sign the message according to this formula to produce the signature:
     * ```text
     * keccak_256(hotkey ++ keccak_256(block_number))
     * ```
     *
     * # Arguments
     * * `origin` - The origin of the transaction, which must be signed by the `hotkey`.
     * * `netuid` - The netuid that the `hotkey` belongs to.
     * * `evm_key` - The EVM key to associate with the `hotkey`.
     * * `block_number` - The block number used in the `signature`.
     * * `signature` - A signed message by the `evm_key` containing the `hotkey` and the hashed `block_number`.
     *
     * # Errors
     * Returns an error if:
     * * The transaction is not signed.
     * * The hotkey does not belong to the subnet identified by the netuid.
     * * The EVM key cannot be recovered from the signature.
     * * The EVM key recovered from the signature does not match the given EVM key.
     *
     * # Events
     * May emit a `EvmKeyAssociated` event on success
     */
    "associate_evm_key": Anonymize<I96k3nrdjfd63k>;
    /**
     * Recycles alpha from a cold/hot key pair, reducing AlphaOut on a subnet
     *
     * # Arguments
     * * `origin` - The origin of the call (must be signed by the coldkey)
     * * `hotkey` - The hotkey account
     * * `amount` - The amount of alpha to recycle
     * * `netuid` - The subnet ID
     *
     * # Events
     * Emits a `TokensRecycled` event on success.
     */
    "recycle_alpha": Anonymize<Ibg3cp8vjl5u55>;
    /**
     * Burns alpha from a cold/hot key pair without reducing `AlphaOut`
     *
     * # Arguments
     * * `origin` - The origin of the call (must be signed by the coldkey)
     * * `hotkey` - The hotkey account
     * * `amount` - The amount of alpha to burn
     * * `netuid` - The subnet ID
     *
     * # Events
     * Emits a `TokensBurned` event on success.
     */
    "burn_alpha": Anonymize<Ibg3cp8vjl5u55>;
    /**
     * Sets the pending childkey cooldown (in blocks). Root only.
     */
    "set_pending_childkey_cooldown": Anonymize<Ibtu1gfmdnou5k>;
    /**
     * Removes all stake from a hotkey on a subnet with a price limit.
     * This extrinsic allows to specify the limit price for alpha token
     * at which or better (higher) the staking should execute.
     * Without limit_price it remove all the stake similar to `remove_stake` extrinsic
     */
    "remove_stake_full_limit": Anonymize<Iaoomvri5btde>;
    /**
     * Register a new leased network.
     *
     * The crowdloan's contributions are used to compute the share of the emissions that the contributors
     * will receive as dividends.
     *
     * The leftover cap is refunded to the contributors and the beneficiary.
     *
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * `emissions_share` (Percent):
     * - The share of the emissions that the contributors will receive as dividends.
     *
     * * `end_block` (Option<BlockNumberFor<T>>):
     * - The block at which the lease will end. If not defined, the lease is perpetual.
     */
    "register_leased_network": Anonymize<Ic80igo4eds6rq>;
    /**
     * Terminate a lease.
     *
     * The beneficiary can terminate the lease after the end block has passed and get the subnet ownership.
     * The subnet is transferred to the beneficiary and the lease is removed from storage.
     *
     * **The hotkey must be owned by the beneficiary coldkey.**
     *
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * `lease_id` (LeaseId):
     * - The ID of the lease to terminate.
     *
     * * `hotkey` (T::AccountId):
     * - The hotkey of the beneficiary to mark as subnet owner hotkey.
     */
    "terminate_lease": Anonymize<Iflrm8un6aibtn>;
    /**
     * Updates the symbol for a subnet.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the subnet owner or root.
     * * `netuid` - The unique identifier of the subnet on which the symbol is being set.
     * * `symbol` - The symbol to set for the subnet.
     *
     * # Errors
     * Returns an error if:
     * * The transaction is not signed by the subnet owner.
     * * The symbol does not exist.
     * * The symbol is already in use by another subnet.
     *
     * # Events
     * Emits a `SymbolUpdated` event on success.
     */
    "update_symbol": Anonymize<I62rrikn5vj0p5>;
    /**
     * ---- Used to commit timelock encrypted commit-reveal weight values to later be revealed.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The committing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `commit` (`Vec<u8>`):
     * - The encrypted compressed commit.
     * The steps for this are:
     * 1. Instantiate [`WeightsTlockPayload`]
     * 2. Serialize it using the `parity_scale_codec::Encode` trait
     * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
     * to produce a [`TLECiphertext<TinyBLS381>`] type.
     * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
     *
     * * reveal_round (`u64`):
     * - The drand reveal round which will be avaliable during epoch `n+1` from the current
     * epoch.
     *
     * * commit_reveal_version (`u16`):
     * - The client (bittensor-drand) version
     */
    "commit_timelocked_weights": Anonymize<Ietm4rjshhu7sf>;
    /**
     * Set the autostake destination hotkey for a coldkey.
     *
     * The caller selects a hotkey where all future rewards
     * will be automatically staked.
     *
     * # Args:
     * * `origin` - (<T as frame_system::Config>::Origin):
     * - The signature of the caller's coldkey.
     *
     * * `hotkey` (T::AccountId):
     * - The hotkey account to designate as the autostake destination.
     */
    "set_coldkey_auto_stake_hotkey": Anonymize<I7f38r2vt6r9k1>;
    /**
     * ---- Used to commit timelock encrypted commit-reveal weight values to later be revealed for
     * a mechanism.
     *
     * # Args:
     * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
     * - The committing hotkey.
     *
     * * `netuid` (`u16`):
     * - The u16 network identifier.
     *
     * * `mecid` (`u8`):
     * - The u8 mechanism identifier.
     *
     * * `commit` (`Vec<u8>`):
     * - The encrypted compressed commit.
     * The steps for this are:
     * 1. Instantiate [`WeightsTlockPayload`]
     * 2. Serialize it using the `parity_scale_codec::Encode` trait
     * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
     * to produce a [`TLECiphertext<TinyBLS381>`] type.
     * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
     *
     * * reveal_round (`u64`):
     * - The drand reveal round which will be avaliable during epoch `n+1` from the current
     * epoch.
     *
     * * commit_reveal_version (`u16`):
     * - The client (bittensor-drand) version
     */
    "commit_timelocked_mechanism_weights": Anonymize<I1v9m3ms1elitm>;
    /**
     * Remove a subnetwork
     * The caller must be root
     */
    "root_dissolve_network": Anonymize<I6cm4c5a1euio9>;
    /**
     * --- Claims the root emissions for a coldkey.
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller's coldkey.
     *
     * # Event:
     * * RootClaimed;
     * - On the successfully claiming the root emissions for a coldkey.
     *
     * # Raises:
     *
     */
    "claim_root": Anonymize<I2t4b7068rtebl>;
    /**
     * --- Sets the root claim type for the coldkey.
     * # Args:
     * * 'origin': (<T as frame_system::Config>Origin):
     * - The signature of the caller's coldkey.
     *
     * # Event:
     * * RootClaimTypeSet;
     * - On the successfully setting the root claim type for the coldkey.
     *
     */
    "set_root_claim_type": Anonymize<I7a99hd3nbic2l>;
    /**
     * --- Sets root claim number (sudo extrinsic). Zero disables auto-claim.
     */
    "sudo_set_num_root_claims": Anonymize<Ie8hpsm3jhsvo3>;
    /**
     * --- Sets root claim threshold for subnet (sudo or owner origin).
     */
    "sudo_set_root_claim_threshold": Anonymize<Ifcj247vgfdg56>;
    /**
     * Announces a coldkey swap using BlakeTwo256 hash of the new coldkey.
     *
     * This is required before the coldkey swap can be performed
     * after the delay period.
     *
     * It can be reannounced after a delay of `ColdkeySwapReannouncementDelay` following
     * the first valid execution block of the original announcement.
     *
     * The dispatch origin of this call must be the original coldkey that made the announcement.
     *
     * - `new_coldkey_hash`: The hash of the new coldkey using BlakeTwo256.
     *
     * The `ColdkeySwapAnnounced` event is emitted on successful announcement.
     *
     */
    "announce_coldkey_swap": Anonymize<Ic21uicfit5vcu>;
    /**
     * Performs a coldkey swap if an announcement has been made.
     *
     * The dispatch origin of this call must be the original coldkey that made the announcement.
     *
     * - `new_coldkey`: The new coldkey to swap to. The BlakeTwo256 hash of the new coldkey must be
     * the same as the announced coldkey hash.
     *
     * The `ColdkeySwapped` event is emitted on successful swap.
     */
    "swap_coldkey_announced": Anonymize<If2k69ql8jgivj>;
    /**
     * Dispute a coldkey swap.
     *
     * This will prevent any further actions on the coldkey swap
     * until triumvirate step in to resolve the issue.
     *
     * - `coldkey`: The coldkey to dispute the swap for.
     *
     */
    "dispute_coldkey_swap": undefined;
    /**
     * Reset a coldkey swap by clearing the announcement and dispute status.
     *
     * The dispatch origin of this call must be root.
     *
     * - `coldkey`: The coldkey to reset the swap for.
     *
     */
    "reset_coldkey_swap": Anonymize<I375tmdui1ejfc>;
    /**
     * Enables voting power tracking for a subnet.
     *
     * This function can be called by the subnet owner or root.
     * When enabled, voting power EMA is updated every epoch for all validators.
     * Voting power starts at 0 and increases over epochs.
     *
     * # Arguments:
     * * `origin` - The origin of the call, must be subnet owner or root.
     * * `netuid` - The subnet to enable voting power tracking for.
     *
     * # Errors:
     * * `SubnetNotExist` - If the subnet does not exist.
     * * `NotSubnetOwner` - If the caller is not the subnet owner or root.
     */
    "enable_voting_power_tracking": Anonymize<I6cm4c5a1euio9>;
    /**
     * Schedules disabling of voting power tracking for a subnet.
     *
     * This function can be called by the subnet owner or root.
     * Voting power tracking will continue for 14 days (grace period) after this call,
     * then automatically disable and clear all VotingPower entries for the subnet.
     *
     * # Arguments:
     * * `origin` - The origin of the call, must be subnet owner or root.
     * * `netuid` - The subnet to schedule disabling voting power tracking for.
     *
     * # Errors:
     * * `SubnetNotExist` - If the subnet does not exist.
     * * `NotSubnetOwner` - If the caller is not the subnet owner or root.
     * * `VotingPowerTrackingNotEnabled` - If voting power tracking is not enabled.
     */
    "disable_voting_power_tracking": Anonymize<I6cm4c5a1euio9>;
    /**
     * Sets the EMA alpha value for voting power calculation on a subnet.
     *
     * This function can only be called by root (sudo).
     * Higher alpha = faster response to stake changes.
     * Alpha is stored as u64 with 18 decimal precision (1.0 = 10^18).
     *
     * # Arguments:
     * * `origin` - The origin of the call, must be root.
     * * `netuid` - The subnet to set the alpha for.
     * * `alpha` - The new alpha value (u64 with 18 decimal precision).
     *
     * # Errors:
     * * `BadOrigin` - If the origin is not root.
     * * `SubnetNotExist` - If the subnet does not exist.
     * * `InvalidVotingPowerEmaAlpha` - If alpha is greater than 10^18 (1.0).
     */
    "sudo_set_voting_power_ema_alpha": Anonymize<I4guv8rii4s6je>;
    /**
     * --- The extrinsic is a combination of add_stake(add_stake_limit) and burn_alpha. We buy
     * alpha token first and immediately burn the acquired amount of alpha (aka Subnet buyback).
     */
    "add_stake_burn": Anonymize<I2t2h3sjr2mdj0>;
}>;
export type Icv6ofu4lqekr4 = {
    "netuid": number;
    "dests": Anonymize<Icgljjb6j82uhn>;
    "weights": Anonymize<Icgljjb6j82uhn>;
    "version_key": bigint;
};
export type I48embv0n659kj = {
    "netuid": number;
    "mecid": number;
    "dests": Anonymize<Icgljjb6j82uhn>;
    "weights": Anonymize<Icgljjb6j82uhn>;
    "version_key": bigint;
};
export type I8l6dbd18t5aja = {
    "netuids": Anonymize<Icgljjb6j82uhn>;
    "weights": Array<Anonymize<I95g6i7ilua7lq>>;
    "version_keys": Anonymize<Iafqnechp3omqg>;
};
export type I513du23unvan = {
    "netuid": number;
    "commit_hash": FixedSizeBinary<32>;
};
export type I36o6oho99gjm8 = {
    "netuid": number;
    "mecid": number;
    "commit_hash": FixedSizeBinary<32>;
};
export type If3mvus4cmnb7l = {
    "netuids": Anonymize<Icgljjb6j82uhn>;
    "commit_hashes": Anonymize<Ic5m5lp1oioo8r>;
};
export type I3qrhi1ua10nnf = {
    "netuid": number;
    "uids": Anonymize<Icgljjb6j82uhn>;
    "values": Anonymize<Icgljjb6j82uhn>;
    "salt": Anonymize<Icgljjb6j82uhn>;
    "version_key": bigint;
};
export type I2hpc4ev2drsf2 = {
    "netuid": number;
    "mecid": number;
    "uids": Anonymize<Icgljjb6j82uhn>;
    "values": Anonymize<Icgljjb6j82uhn>;
    "salt": Anonymize<Icgljjb6j82uhn>;
    "version_key": bigint;
};
export type I73q6qh9ckhm04 = {
    "netuid": number;
    "mecid": number;
    "commit": Binary;
    "reveal_round": bigint;
};
export type Idia8cmqvul6et = {
    "netuid": number;
    "uids_list": Array<Anonymize<Icgljjb6j82uhn>>;
    "values_list": Array<Anonymize<Icgljjb6j82uhn>>;
    "salts_list": Array<Anonymize<Icgljjb6j82uhn>>;
    "version_keys": Anonymize<Iafqnechp3omqg>;
};
export type Idardmhchnv8aa = {
    "hotkey": SS58String;
    "take": number;
};
export type Icud5m8j0nlgtj = {
    "hotkey": SS58String;
    "netuid": number;
    "amount_staked": bigint;
};
export type I850u7ir5o34um = {
    "hotkey": SS58String;
    "netuid": number;
    "amount_unstaked": bigint;
};
export type Ica88a899k1afk = {
    "netuid": number;
    "version": number;
    "ip": bigint;
    "port": number;
    "ip_type": number;
    "protocol": number;
    "placeholder1": number;
    "placeholder2": number;
};
export type I4tfn6eb3ekqt2 = {
    "netuid": number;
    "version": number;
    "ip": bigint;
    "port": number;
    "ip_type": number;
    "protocol": number;
    "placeholder1": number;
    "placeholder2": number;
    "certificate": Binary;
};
export type Ia5r6mm7trbg6a = {
    "netuid": number;
    "version": number;
    "ip": bigint;
    "port": number;
    "ip_type": number;
};
export type I27gr0ss2ikvqh = {
    "netuid": number;
    "block_number": bigint;
    "nonce": bigint;
    "work": Binary;
    "hotkey": SS58String;
    "coldkey": SS58String;
};
export type Ie7hipi75c7vn0 = {
    "hotkey": SS58String;
};
export type I7f38r2vt6r9k1 = {
    "netuid": number;
    "hotkey": SS58String;
};
export type I6b53cjq4m9nsr = {
    "hotkey": SS58String;
    "new_hotkey": SS58String;
    "netuid"?: Anonymize<I4arjljr6dpflb>;
};
export type I216fvnrl9nq6l = {
    "old_coldkey": SS58String;
    "new_coldkey": SS58String;
    "swap_cost": bigint;
};
export type I9n4d52k0luroe = {
    "hotkey": SS58String;
    "netuid": number;
    "take": number;
};
export type I3gk6eeddm0hsd = {
    "tx_rate_limit": bigint;
};
export type I6ue7qc27uhiev = {
    "take": number;
};
export type Ifp8lgrkla2dig = {
    "block_number": bigint;
    "nonce": bigint;
    "work": Binary;
};
export type I30l38oi9ed9dj = {
    "coldkey": SS58String;
    "netuid": number;
};
export type Ifj9gf4ekq9snm = {
    "hotkey": SS58String;
    "netuid": number;
    "children": Anonymize<I5n8gpu725k1nu>;
};
export type If2k69ql8jgivj = {
    "new_coldkey": SS58String;
};
export type I4378ieh1uba9u = {
    "netuid": number;
    "subnet_name": Binary;
    "github_repo": Binary;
    "subnet_contact": Binary;
    "subnet_url": Binary;
    "discord": Binary;
    "description": Binary;
    "logo_url": Binary;
    "additional": Binary;
};
export type I8e6f7r9dtk9c1 = {
    "hotkey": SS58String;
    "identity"?: Anonymize<I3m38saj8mvtpv>;
};
export type I3m38saj8mvtpv = (Anonymize<I4tc54pa558g5n>) | undefined;
export type I9d117ni3tprb = {
    "origin_hotkey": SS58String;
    "destination_hotkey": SS58String;
    "origin_netuid": number;
    "destination_netuid": number;
    "alpha_amount": bigint;
};
export type I340k0hbj1hc6r = {
    "destination_coldkey": SS58String;
    "hotkey": SS58String;
    "origin_netuid": number;
    "destination_netuid": number;
    "alpha_amount": bigint;
};
export type Ibapoov2fa817a = {
    "hotkey": SS58String;
    "origin_netuid": number;
    "destination_netuid": number;
    "alpha_amount": bigint;
};
export type I2eon60c4gde7f = {
    "hotkey": SS58String;
    "netuid": number;
    "amount_staked": bigint;
    "limit_price": bigint;
    "allow_partial": boolean;
};
export type I7egr0053sjpci = {
    "hotkey": SS58String;
    "netuid": number;
    "amount_unstaked": bigint;
    "limit_price": bigint;
    "allow_partial": boolean;
};
export type I6r22p9usi2mkl = {
    "hotkey": SS58String;
    "origin_netuid": number;
    "destination_netuid": number;
    "alpha_amount": bigint;
    "limit_price": bigint;
    "allow_partial": boolean;
};
export type I96k3nrdjfd63k = {
    "netuid": number;
    "evm_key": FixedSizeBinary<20>;
    "block_number": bigint;
    "signature": FixedSizeBinary<65>;
};
export type Ibg3cp8vjl5u55 = {
    "hotkey": SS58String;
    "amount": bigint;
    "netuid": number;
};
export type Ibtu1gfmdnou5k = {
    "cooldown": bigint;
};
export type Iaoomvri5btde = {
    "hotkey": SS58String;
    "netuid": number;
    "limit_price"?: Anonymize<I35p85j063s0il>;
};
export type Ic80igo4eds6rq = {
    "emissions_share": number;
    "end_block"?: Anonymize<I4arjljr6dpflb>;
};
export type Iflrm8un6aibtn = {
    "lease_id": number;
    "hotkey": SS58String;
};
export type Ietm4rjshhu7sf = {
    "netuid": number;
    "commit": Binary;
    "reveal_round": bigint;
    "commit_reveal_version": number;
};
export type I1v9m3ms1elitm = {
    "netuid": number;
    "mecid": number;
    "commit": Binary;
    "reveal_round": bigint;
    "commit_reveal_version": number;
};
export type I7a99hd3nbic2l = {
    "new_root_claim_type": Anonymize<Iapm6e7vtp0l6r>;
};
export type Ie8hpsm3jhsvo3 = {
    "new_value": bigint;
};
export type Ifcj247vgfdg56 = {
    "netuid": number;
    "new_value": bigint;
};
export type Ic21uicfit5vcu = {
    "new_coldkey_hash": FixedSizeBinary<32>;
};
export type I2t2h3sjr2mdj0 = {
    "hotkey": SS58String;
    "netuid": number;
    "amount": bigint;
    "limit"?: Anonymize<I35p85j063s0il>;
};
export type If4ks1adspb2ai = AnonymousEnum<{
    /**
     * Send a batch of dispatch calls.
     *
     * May be called from any origin except `None`.
     *
     * - `calls`: The calls to be dispatched from the same origin. The number of call must not
     * exceed the constant: `batched_calls_limit` (available in constant metadata).
     *
     * If origin is root then the calls are dispatched without checking origin filter. (This
     * includes bypassing `frame_system::Config::BaseCallFilter`).
     *
     * ## Complexity
     * - O(C) where C is the number of calls to be batched.
     *
     * This will return `Ok` in all circumstances. To determine the success of the batch, an
     * event is deposited. If a call failed and the batch was interrupted, then the
     * `BatchInterrupted` event is deposited, along with the number of successful calls made
     * and the error of the failed call. If all were successful, then the `BatchCompleted`
     * event is deposited.
     */
    "batch": Anonymize<I80tnmsfsu19sl>;
    /**
     * Send a call through an indexed pseudonym of the sender.
     *
     * Filter from origin are passed along. The call will be dispatched with an origin which
     * use the same filter as the origin of this call.
     *
     * NOTE: If you need to ensure that any account-based filtering is not honored (i.e.
     * because you expect `proxy` to have been used prior in the call stack and you do not want
     * the call restrictions to apply to any sub-accounts), then use `as_multi_threshold_1`
     * in the Multisig pallet instead.
     *
     * NOTE: Prior to version *12, this was called `as_limited_sub`.
     *
     * The dispatch origin for this call must be _Signed_.
     */
    "as_derivative": Anonymize<Ib7nn1mns0usdp>;
    /**
     * Send a batch of dispatch calls and atomically execute them.
     * The whole transaction will rollback and fail if any of the calls failed.
     *
     * May be called from any origin except `None`.
     *
     * - `calls`: The calls to be dispatched from the same origin. The number of call must not
     * exceed the constant: `batched_calls_limit` (available in constant metadata).
     *
     * If origin is root then the calls are dispatched without checking origin filter. (This
     * includes bypassing `frame_system::Config::BaseCallFilter`).
     *
     * ## Complexity
     * - O(C) where C is the number of calls to be batched.
     */
    "batch_all": Anonymize<I80tnmsfsu19sl>;
    /**
     * Dispatches a function call with a provided origin.
     *
     * The dispatch origin for this call must be _Root_.
     *
     * ## Complexity
     * - O(1).
     */
    "dispatch_as": Anonymize<I4fivl1mrn0hhc>;
    /**
     * Send a batch of dispatch calls.
     * Unlike `batch`, it allows errors and won't interrupt.
     *
     * May be called from any origin except `None`.
     *
     * - `calls`: The calls to be dispatched from the same origin. The number of call must not
     * exceed the constant: `batched_calls_limit` (available in constant metadata).
     *
     * If origin is root then the calls are dispatch without checking origin filter. (This
     * includes bypassing `frame_system::Config::BaseCallFilter`).
     *
     * ## Complexity
     * - O(C) where C is the number of calls to be batched.
     */
    "force_batch": Anonymize<I80tnmsfsu19sl>;
    /**
     * Dispatch a function call with a specified weight.
     *
     * This function does not check the weight of the call, and instead allows the
     * Root origin to specify the weight of the call.
     *
     * The dispatch origin for this call must be _Root_.
     */
    "with_weight": Anonymize<I2ead8rm0h16hm>;
    /**
     * Dispatch a fallback call in the event the main call fails to execute.
     * May be called from any origin except `None`.
     *
     * This function first attempts to dispatch the `main` call.
     * If the `main` call fails, the `fallback` is attemted.
     * if the fallback is successfully dispatched, the weights of both calls
     * are accumulated and an event containing the main call error is deposited.
     *
     * In the event of a fallback failure the whole call fails
     * with the weights returned.
     *
     * - `main`: The main call to be dispatched. This is the primary action to execute.
     * - `fallback`: The fallback call to be dispatched in case the `main` call fails.
     *
     * ## Dispatch Logic
     * - If the origin is `root`, both the main and fallback calls are executed without
     * applying any origin filters.
     * - If the origin is not `root`, the origin filter is applied to both the `main` and
     * `fallback` calls.
     *
     * ## Use Case
     * - Some use cases might involve submitting a `batch` type call in either main, fallback
     * or both.
     */
    "if_else": Anonymize<I25l72483lbgf9>;
    /**
     * Dispatches a function call with a provided origin.
     *
     * Almost the same as [`Pallet::dispatch_as`] but forwards any error of the inner call.
     *
     * The dispatch origin for this call must be _Root_.
     */
    "dispatch_as_fallible": Anonymize<I4fivl1mrn0hhc>;
}>;
export type I80tnmsfsu19sl = {
    "calls": Array<TxCallData>;
};
export type Ib7nn1mns0usdp = {
    "index": number;
    "call": TxCallData;
};
export type I4fivl1mrn0hhc = {
    "as_origin": Anonymize<I32es0rp64745v>;
    "call": TxCallData;
};
export type I2ead8rm0h16hm = {
    "call": TxCallData;
    "weight": Anonymize<I4q39t5hn830vp>;
};
export type I25l72483lbgf9 = {
    "main": TxCallData;
    "fallback": TxCallData;
};
export type Ic3n0u1krodt07 = AnonymousEnum<{
    /**
     * Authenticates the sudo key and dispatches a function call with `Root` origin.
     */
    "sudo": Anonymize<I9okvr56cd7277>;
    /**
     * Authenticates the sudo key and dispatches a function call with `Root` origin.
     * This function does not check the weight of the call, and instead allows the
     * Sudo user to specify the weight of the call.
     *
     * The dispatch origin for this call must be _Signed_.
     */
    "sudo_unchecked_weight": Anonymize<I2ead8rm0h16hm>;
    /**
     * Authenticates the current sudo key and sets the given AccountId (`new`) as the new sudo
     * key.
     */
    "set_key": Anonymize<I8k3rnvpeeh4hv>;
    /**
     * Authenticates the sudo key and dispatches a function call with `Signed` origin from
     * a given account.
     *
     * The dispatch origin for this call must be _Signed_.
     */
    "sudo_as": Anonymize<I56sht7incdimf>;
    /**
     * Permanently removes the sudo key.
     *
     * **This cannot be un-done.**
     */
    "remove_key": undefined;
}>;
export type I9okvr56cd7277 = {
    "call": TxCallData;
};
export type I8k3rnvpeeh4hv = {
    "new": MultiAddress;
};
export type I56sht7incdimf = {
    "who": MultiAddress;
    "call": TxCallData;
};
export type I92oc1s48b79mg = AnonymousEnum<{
    /**
     * Immediately dispatch a multi-signature call using a single approval from the caller.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * - `other_signatories`: The accounts (other than the sender) who are part of the
     * multi-signature, but do not participate in the approval process.
     * - `call`: The call to be executed.
     *
     * Result is equivalent to the dispatched result.
     *
     * ## Complexity
     * O(Z + C) where Z is the length of the call and C its execution weight.
     */
    "as_multi_threshold_1": Anonymize<I8hge8nrufr05f>;
    /**
     * Register approval for a dispatch to be made from a deterministic composite account if
     * approved by a total of `threshold - 1` of `other_signatories`.
     *
     * If there are enough, then dispatch the call.
     *
     * Payment: `DepositBase` will be reserved if this is the first approval, plus
     * `threshold` times `DepositFactor`. It is returned once this dispatch happens or
     * is cancelled.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * - `threshold`: The total number of approvals for this dispatch before it is executed.
     * - `other_signatories`: The accounts (other than the sender) who can approve this
     * dispatch. May not be empty.
     * - `maybe_timepoint`: If this is the first approval, then this must be `None`. If it is
     * not the first approval, then it must be `Some`, with the timepoint (block number and
     * transaction index) of the first approval transaction.
     * - `call`: The call to be executed.
     *
     * NOTE: Unless this is the final approval, you will generally want to use
     * `approve_as_multi` instead, since it only requires a hash of the call.
     *
     * Result is equivalent to the dispatched result if `threshold` is exactly `1`. Otherwise
     * on success, result is `Ok` and the result from the interior call, if it was executed,
     * may be found in the deposited `MultisigExecuted` event.
     *
     * ## Complexity
     * - `O(S + Z + Call)`.
     * - Up to one balance-reserve or unreserve operation.
     * - One passthrough operation, one insert, both `O(S)` where `S` is the number of
     * signatories. `S` is capped by `MaxSignatories`, with weight being proportional.
     * - One call encode & hash, both of complexity `O(Z)` where `Z` is tx-len.
     * - One encode & hash, both of complexity `O(S)`.
     * - Up to one binary search and insert (`O(logS + S)`).
     * - I/O: 1 read `O(S)`, up to 1 mutate `O(S)`. Up to one remove.
     * - One event.
     * - The weight of the `call`.
     * - Storage: inserts one item, value size bounded by `MaxSignatories`, with a deposit
     * taken for its lifetime of `DepositBase + threshold * DepositFactor`.
     */
    "as_multi": Anonymize<I5v0mk7rggegmh>;
    /**
     * Register approval for a dispatch to be made from a deterministic composite account if
     * approved by a total of `threshold - 1` of `other_signatories`.
     *
     * Payment: `DepositBase` will be reserved if this is the first approval, plus
     * `threshold` times `DepositFactor`. It is returned once this dispatch happens or
     * is cancelled.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * - `threshold`: The total number of approvals for this dispatch before it is executed.
     * - `other_signatories`: The accounts (other than the sender) who can approve this
     * dispatch. May not be empty.
     * - `maybe_timepoint`: If this is the first approval, then this must be `None`. If it is
     * not the first approval, then it must be `Some`, with the timepoint (block number and
     * transaction index) of the first approval transaction.
     * - `call_hash`: The hash of the call to be executed.
     *
     * NOTE: If this is the final approval, you will want to use `as_multi` instead.
     *
     * ## Complexity
     * - `O(S)`.
     * - Up to one balance-reserve or unreserve operation.
     * - One passthrough operation, one insert, both `O(S)` where `S` is the number of
     * signatories. `S` is capped by `MaxSignatories`, with weight being proportional.
     * - One encode & hash, both of complexity `O(S)`.
     * - Up to one binary search and insert (`O(logS + S)`).
     * - I/O: 1 read `O(S)`, up to 1 mutate `O(S)`. Up to one remove.
     * - One event.
     * - Storage: inserts one item, value size bounded by `MaxSignatories`, with a deposit
     * taken for its lifetime of `DepositBase + threshold * DepositFactor`.
     */
    "approve_as_multi": Anonymize<Ideaemvoneh309>;
    /**
     * Cancel a pre-existing, on-going multisig transaction. Any deposit reserved previously
     * for this operation will be unreserved on success.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * - `threshold`: The total number of approvals for this dispatch before it is executed.
     * - `other_signatories`: The accounts (other than the sender) who can approve this
     * dispatch. May not be empty.
     * - `timepoint`: The timepoint (block number and transaction index) of the first approval
     * transaction for this dispatch.
     * - `call_hash`: The hash of the call to be executed.
     *
     * ## Complexity
     * - `O(S)`.
     * - Up to one balance-reserve or unreserve operation.
     * - One passthrough operation, one insert, both `O(S)` where `S` is the number of
     * signatories. `S` is capped by `MaxSignatories`, with weight being proportional.
     * - One encode & hash, both of complexity `O(S)`.
     * - One event.
     * - I/O: 1 read `O(S)`, one remove.
     * - Storage: removes one item.
     */
    "cancel_as_multi": Anonymize<I3d9o9d7epp66v>;
    /**
     * Poke the deposit reserved for an existing multisig operation.
     *
     * The dispatch origin for this call must be _Signed_ and must be the original depositor of
     * the multisig operation.
     *
     * The transaction fee is waived if the deposit amount has changed.
     *
     * - `threshold`: The total number of approvals needed for this multisig.
     * - `other_signatories`: The accounts (other than the sender) who are part of the
     * multisig.
     * - `call_hash`: The hash of the call this deposit is reserved for.
     *
     * Emits `DepositPoked` if successful.
     */
    "poke_deposit": Anonymize<I6lqh1vgb4mcja>;
}>;
export type I8hge8nrufr05f = {
    "other_signatories": Anonymize<Ia2lhg7l2hilo3>;
    "call": TxCallData;
};
export type I5v0mk7rggegmh = {
    "threshold": number;
    "other_signatories": Anonymize<Ia2lhg7l2hilo3>;
    "maybe_timepoint"?: Anonymize<I95jfd8j5cr5eh>;
    "call": TxCallData;
    "max_weight": Anonymize<I4q39t5hn830vp>;
};
export type I95jfd8j5cr5eh = (Anonymize<Itvprrpb0nm3o>) | undefined;
export type Ideaemvoneh309 = {
    "threshold": number;
    "other_signatories": Anonymize<Ia2lhg7l2hilo3>;
    "maybe_timepoint"?: Anonymize<I95jfd8j5cr5eh>;
    "call_hash": FixedSizeBinary<32>;
    "max_weight": Anonymize<I4q39t5hn830vp>;
};
export type I3d9o9d7epp66v = {
    "threshold": number;
    "other_signatories": Anonymize<Ia2lhg7l2hilo3>;
    "timepoint": Anonymize<Itvprrpb0nm3o>;
    "call_hash": FixedSizeBinary<32>;
};
export type I6lqh1vgb4mcja = {
    "threshold": number;
    "other_signatories": Anonymize<Ia2lhg7l2hilo3>;
    "call_hash": FixedSizeBinary<32>;
};
export type If81ks88t5mpk5 = AnonymousEnum<{
    /**
     * Register a preimage on-chain.
     *
     * If the preimage was previously requested, no fees or deposits are taken for providing
     * the preimage. Otherwise, a deposit is taken proportional to the size of the preimage.
     */
    "note_preimage": Anonymize<I82nfqfkd48n10>;
    /**
     * Clear an unrequested preimage from the runtime storage.
     *
     * If `len` is provided, then it will be a much cheaper operation.
     *
     * - `hash`: The hash of the preimage to be removed from the store.
     * - `len`: The length of the preimage of `hash`.
     */
    "unnote_preimage": Anonymize<I1jm8m1rh9e20v>;
    /**
     * Request a preimage be uploaded to the chain without paying any fees or deposits.
     *
     * If the preimage requests has already been provided on-chain, we unreserve any deposit
     * a user may have paid, and take the control of the preimage out of their hands.
     */
    "request_preimage": Anonymize<I1jm8m1rh9e20v>;
    /**
     * Clear a previously made request for a preimage.
     *
     * NOTE: THIS MUST NOT BE CALLED ON `hash` MORE TIMES THAN `request_preimage`.
     */
    "unrequest_preimage": Anonymize<I1jm8m1rh9e20v>;
    /**
     * Ensure that the bulk of pre-images is upgraded.
     *
     * The caller pays no fee if at least 90% of pre-images were successfully updated.
     */
    "ensure_updated": Anonymize<I3o5j3bli1pd8e>;
}>;
export type I82nfqfkd48n10 = {
    "bytes": Binary;
};
export type I3o5j3bli1pd8e = {
    "hashes": Anonymize<Ic5m5lp1oioo8r>;
};
export type I8joseop2odch3 = AnonymousEnum<{
    /**
     * Anonymously schedule a task.
     */
    "schedule": Anonymize<Ivqkjqsbgj1dj>;
    /**
     * Cancel an anonymously scheduled task.
     */
    "cancel": Anonymize<I5n4sebgkfr760>;
    /**
     * Schedule a named task.
     */
    "schedule_named": Anonymize<Ib6bm2ug64rldc>;
    /**
     * Cancel a named scheduled task.
     */
    "cancel_named": Anonymize<Ifs1i5fk9cqvr6>;
    /**
     * Anonymously schedule a task after a delay.
     */
    "schedule_after": Anonymize<I5q3t0hm83a58h>;
    /**
     * Schedule a named task after a delay.
     */
    "schedule_named_after": Anonymize<I2gnaqoj2eimi0>;
    /**
     * Set a retry configuration for a task so that, in case its scheduled run fails, it will
     * be retried after `period` blocks, for a total amount of `retries` retries or until it
     * succeeds.
     *
     * Tasks which need to be scheduled for a retry are still subject to weight metering and
     * agenda space, same as a regular task. If a periodic task fails, it will be scheduled
     * normally while the task is retrying.
     *
     * Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic
     * clones of the original task. Their retry configuration will be derived from the
     * original task's configuration, but will have a lower value for `remaining` than the
     * original `total_retries`.
     */
    "set_retry": Anonymize<Ieg3fd8p4pkt10>;
    /**
     * Set a retry configuration for a named task so that, in case its scheduled run fails, it
     * will be retried after `period` blocks, for a total amount of `retries` retries or until
     * it succeeds.
     *
     * Tasks which need to be scheduled for a retry are still subject to weight metering and
     * agenda space, same as a regular task. If a periodic task fails, it will be scheduled
     * normally while the task is retrying.
     *
     * Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic
     * clones of the original task. Their retry configuration will be derived from the
     * original task's configuration, but will have a lower value for `remaining` than the
     * original `total_retries`.
     */
    "set_retry_named": Anonymize<I8kg5ll427kfqq>;
    /**
     * Removes the retry configuration of a task.
     */
    "cancel_retry": Anonymize<I467333262q1l9>;
    /**
     * Cancel the retry configuration of a named task.
     */
    "cancel_retry_named": Anonymize<Ifs1i5fk9cqvr6>;
}>;
export type Ivqkjqsbgj1dj = {
    "when": number;
    "maybe_periodic"?: Anonymize<Iep7au1720bm0e>;
    "priority": number;
    "call": TxCallData;
};
export type Ib6bm2ug64rldc = {
    "id": FixedSizeBinary<32>;
    "when": number;
    "maybe_periodic"?: Anonymize<Iep7au1720bm0e>;
    "priority": number;
    "call": TxCallData;
};
export type Ifs1i5fk9cqvr6 = {
    "id": FixedSizeBinary<32>;
};
export type I5q3t0hm83a58h = {
    "after": number;
    "maybe_periodic"?: Anonymize<Iep7au1720bm0e>;
    "priority": number;
    "call": TxCallData;
};
export type I2gnaqoj2eimi0 = {
    "id": FixedSizeBinary<32>;
    "after": number;
    "maybe_periodic"?: Anonymize<Iep7au1720bm0e>;
    "priority": number;
    "call": TxCallData;
};
export type Ieg3fd8p4pkt10 = {
    "task": Anonymize<I9jd27rnpm8ttv>;
    "retries": number;
    "period": number;
};
export type I8kg5ll427kfqq = {
    "id": FixedSizeBinary<32>;
    "retries": number;
    "period": number;
};
export type I467333262q1l9 = {
    "task": Anonymize<I9jd27rnpm8ttv>;
};
export type Ia2ee5fnr4ukkf = AnonymousEnum<{
    /**
     * Dispatch the given `call` from an account that the sender is authorised for through
     * `add_proxy`.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `real`: The account that the proxy will make a call on behalf of.
     * - `force_proxy_type`: Specify the exact proxy type to be used and checked for this call.
     * - `call`: The call to be made by the `real` account.
     */
    "proxy": Anonymize<Idlqs144rc48hk>;
    /**
     * Register a proxy account for the sender that is able to make calls on its behalf.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `proxy`: The account that the `caller` would like to make a proxy.
     * - `proxy_type`: The permissions allowed for this proxy account.
     * - `delay`: The announcement period required of the initial proxy. Will generally be
     * zero.
     */
    "add_proxy": Anonymize<It11trpppbc3l>;
    /**
     * Unregister a proxy account for the sender.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `proxy`: The account that the `caller` would like to remove as a proxy.
     * - `proxy_type`: The permissions currently enabled for the removed proxy account.
     */
    "remove_proxy": Anonymize<It11trpppbc3l>;
    /**
     * Unregister all proxy accounts for the sender.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * WARNING: This may be called on accounts created by `create_pure`, however if done, then
     * the unreserved fees will be inaccessible. **All access to this account will be lost.**
     */
    "remove_proxies": undefined;
    /**
     * Spawn a fresh new account that is guaranteed to be otherwise inaccessible, and
     * initialize it with a proxy of `proxy_type` for `origin` sender.
     *
     * Requires a `Signed` origin.
     *
     * - `proxy_type`: The type of the proxy that the sender will be registered as over the
     * new account. This will almost always be the most permissive `ProxyType` possible to
     * allow for maximum flexibility.
     * - `index`: A disambiguation index, in case this is called multiple times in the same
     * transaction (e.g. with `utility::batch`). Unless you're using `batch` you probably just
     * want to use `0`.
     * - `delay`: The announcement period required of the initial proxy. Will generally be
     * zero.
     *
     * Fails with `Duplicate` if this has already been called in this transaction, from the
     * same sender, with the same parameters.
     *
     * Fails if there are insufficient funds to pay for deposit.
     */
    "create_pure": Anonymize<Ietml13sclqs1q>;
    /**
     * Removes a previously spawned pure proxy.
     *
     * WARNING: **All access to this account will be lost.** Any funds held in it will be
     * inaccessible.
     *
     * Requires a `Signed` origin, and the sender account must have been created by a call to
     * `create_pure` with corresponding parameters.
     *
     * - `spawner`: The account that originally called `create_pure` to create this account.
     * - `index`: The disambiguation index originally passed to `create_pure`. Probably `0`.
     * - `proxy_type`: The proxy type originally passed to `create_pure`.
     * - `height`: The height of the chain when the call to `create_pure` was processed.
     * - `ext_index`: The extrinsic index in which the call to `create_pure` was processed.
     *
     * Fails with `NoPermission` in case the caller is not a previously created pure
     * account whose `create_pure` call has corresponding parameters.
     */
    "kill_pure": Anonymize<Iftfic7p3uban2>;
    /**
     * Publish the hash of a proxy-call that will be made in the future.
     *
     * This must be called some number of blocks before the corresponding `proxy` is attempted
     * if the delay associated with the proxy relationship is greater than zero.
     *
     * No more than `MaxPending` announcements may be made at any one time.
     *
     * This will take a deposit of `AnnouncementDepositFactor` as well as
     * `AnnouncementDepositBase` if there are no other pending announcements.
     *
     * The dispatch origin for this call must be _Signed_ and a proxy of `real`.
     *
     * Parameters:
     * - `real`: The account that the proxy will make a call on behalf of.
     * - `call_hash`: The hash of the call to be made by the `real` account.
     */
    "announce": Anonymize<I2eb501t8s6hsq>;
    /**
     * Remove a given announcement.
     *
     * May be called by a proxy account to remove a call they previously announced and return
     * the deposit.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `real`: The account that the proxy will make a call on behalf of.
     * - `call_hash`: The hash of the call to be made by the `real` account.
     */
    "remove_announcement": Anonymize<I2eb501t8s6hsq>;
    /**
     * Remove the given announcement of a delegate.
     *
     * May be called by a target (proxied) account to remove a call that one of their delegates
     * (`delegate`) has announced they want to execute. The deposit is returned.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `delegate`: The account that previously announced the call.
     * - `call_hash`: The hash of the call to be made.
     */
    "reject_announcement": Anonymize<Ianmuoljk2sk1u>;
    /**
     * Dispatch the given `call` from an account that the sender is authorized for through
     * `add_proxy`.
     *
     * Removes any corresponding announcement(s).
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `real`: The account that the proxy will make a call on behalf of.
     * - `force_proxy_type`: Specify the exact proxy type to be used and checked for this call.
     * - `call`: The call to be made by the `real` account.
     */
    "proxy_announced": Anonymize<I7hgtlnpelk0fc>;
    /**
     * Poke / Adjust deposits made for proxies and announcements based on current values.
     * This can be used by accounts to possibly lower their locked amount.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * The transaction fee is waived if the deposit amount has changed.
     *
     * Emits `DepositPoked` if successful.
     */
    "poke_deposit": undefined;
}>;
export type Idlqs144rc48hk = {
    "real": MultiAddress;
    "force_proxy_type"?: Anonymize<Iccd9gbcgdpjso>;
    "call": TxCallData;
};
export type Iccd9gbcgdpjso = (Anonymize<I8v1041j74kmaj>) | undefined;
export type It11trpppbc3l = {
    "delegate": MultiAddress;
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "delay": number;
};
export type Ietml13sclqs1q = {
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "delay": number;
    "index": number;
};
export type Iftfic7p3uban2 = {
    "spawner": MultiAddress;
    "proxy_type": Anonymize<I8v1041j74kmaj>;
    "index": number;
    "height": number;
    "ext_index": number;
};
export type I2eb501t8s6hsq = {
    "real": MultiAddress;
    "call_hash": FixedSizeBinary<32>;
};
export type Ianmuoljk2sk1u = {
    "delegate": MultiAddress;
    "call_hash": FixedSizeBinary<32>;
};
export type I7hgtlnpelk0fc = {
    "delegate": MultiAddress;
    "real": MultiAddress;
    "force_proxy_type"?: Anonymize<Iccd9gbcgdpjso>;
    "call": TxCallData;
};
export type Ifml9odtov51l3 = AnonymousEnum<{
    /**
     * Register an identity for an account. This will overwrite any existing identity.
     */
    "set_identity": Anonymize<I3p6khp3nv37cu>;
    /**
     * Clear the identity of an account.
     */
    "clear_identity": Anonymize<I6pnnj50tnq448>;
}>;
export type I3p6khp3nv37cu = {
    "identified": SS58String;
    "info": Anonymize<Ifiu33afi2n7qs>;
};
export type I6pnnj50tnq448 = {
    "identified": SS58String;
};
export type I5bqhvupj937er = AnonymousEnum<{
    /**
     * Set the commitment for a given netuid
     */
    "set_commitment": Anonymize<I57v1t6776pl3a>;
    /**
     * Sudo-set MaxSpace
     */
    "set_max_space": Anonymize<I1il5mj68vvsms>;
}>;
export type I57v1t6776pl3a = {
    "netuid": number;
    "info": Anonymize<I4122t6tpcniur>;
};
export type I1il5mj68vvsms = {
    "new_limit": number;
};
export type Iemvun0dttbcqs = AnonymousEnum<{
    /**
     * The extrinsic sets the new authorities for Aura consensus.
     * It is only callable by the root account.
     * The extrinsic will call the Aura pallet to change the authorities.
     */
    "swap_authorities": Anonymize<I42mob3hqe6j7h>;
    /**
     * The extrinsic sets the default take for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the default take.
     */
    "sudo_set_default_take": Anonymize<Icdbq0j31b3g9c>;
    /**
     * The extrinsic sets the transaction rate limit for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the transaction rate limit.
     */
    "sudo_set_tx_rate_limit": Anonymize<I3gk6eeddm0hsd>;
    /**
     * The extrinsic sets the serving rate limit for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the serving rate limit.
     */
    "sudo_set_serving_rate_limit": Anonymize<I2t2rlclb0ce3e>;
    /**
     * The extrinsic sets the minimum difficulty for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the minimum difficulty.
     */
    "sudo_set_min_difficulty": Anonymize<Iar87gdqmug5o7>;
    /**
     * The extrinsic sets the maximum difficulty for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the maximum difficulty.
     */
    "sudo_set_max_difficulty": Anonymize<I3oullii9p80a1>;
    /**
     * The extrinsic sets the weights version key for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the weights version key.
     */
    "sudo_set_weights_version_key": Anonymize<I8t8ta6lfbia9e>;
    /**
     * The extrinsic sets the weights set rate limit for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the weights set rate limit.
     */
    "sudo_set_weights_set_rate_limit": Anonymize<I3akfmjle982qg>;
    /**
     * The extrinsic sets the adjustment interval for a subnet.
     * It is only callable by the root account, not changeable by the subnet owner.
     * The extrinsic will call the Subtensor pallet to set the adjustment interval.
     */
    "sudo_set_adjustment_interval": Anonymize<Ibaje86kdit7s6>;
    /**
     * The extrinsic sets the adjustment alpha for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the adjustment alpha.
     */
    "sudo_set_adjustment_alpha": Anonymize<I90lra4vl5j4db>;
    /**
     * The extrinsic sets the immunity period for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the immunity period.
     */
    "sudo_set_immunity_period": Anonymize<I1q480m57ftcms>;
    /**
     * The extrinsic sets the minimum allowed weights for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the minimum allowed weights.
     */
    "sudo_set_min_allowed_weights": Anonymize<Ie2bjglo51atf6>;
    /**
     * The extrinsic sets the maximum allowed UIDs for a subnet.
     * It is only callable by the root account and subnet owner.
     * The extrinsic will call the Subtensor pallet to set the maximum allowed UIDs for a subnet.
     */
    "sudo_set_max_allowed_uids": Anonymize<Ievma38tc25kil>;
    /**
     * The extrinsic sets the kappa for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the kappa.
     */
    "sudo_set_kappa": Anonymize<I2er75v4akf5cc>;
    /**
     * The extrinsic sets the rho for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the rho.
     */
    "sudo_set_rho": Anonymize<I5pldh0j0v0u4l>;
    /**
     * The extrinsic sets the activity cutoff for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the activity cutoff.
     */
    "sudo_set_activity_cutoff": Anonymize<Ifhou5p0slv68r>;
    /**
     * The extrinsic sets the network registration allowed for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the network registration allowed.
     */
    "sudo_set_network_registration_allowed": Anonymize<I9m89dnau2i4tt>;
    /**
     * The extrinsic sets the network PoW registration allowed for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the network PoW registration allowed.
     */
    "sudo_set_network_pow_registration_allowed": Anonymize<I9m89dnau2i4tt>;
    /**
     * The extrinsic sets the target registrations per interval for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the target registrations per interval.
     */
    "sudo_set_target_registrations_per_interval": Anonymize<Ifunpjbsc4jrrr>;
    /**
     * The extrinsic sets the minimum burn for a subnet.
     * It is only callable by root and subnet owner.
     * The extrinsic will call the Subtensor pallet to set the minimum burn.
     */
    "sudo_set_min_burn": Anonymize<I85uujfpnu8gum>;
    /**
     * The extrinsic sets the maximum burn for a subnet.
     * It is only callable by root and subnet owner.
     * The extrinsic will call the Subtensor pallet to set the maximum burn.
     */
    "sudo_set_max_burn": Anonymize<I7bl5t0it6ck2m>;
    /**
     * The extrinsic sets the difficulty for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the difficulty.
     */
    "sudo_set_difficulty": Anonymize<I4iope0tjiqgu4>;
    /**
     * The extrinsic sets the maximum allowed validators for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the maximum allowed validators.
     */
    "sudo_set_max_allowed_validators": Anonymize<Iptqa236frcvo>;
    /**
     * The extrinsic sets the bonds moving average for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the bonds moving average.
     */
    "sudo_set_bonds_moving_average": Anonymize<I8hbi1vrve1i2>;
    /**
     * The extrinsic sets the bonds penalty for a subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the bonds penalty.
     */
    "sudo_set_bonds_penalty": Anonymize<I1v9a50gjqk26k>;
    /**
     * The extrinsic sets the maximum registrations per block for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the maximum registrations per block.
     */
    "sudo_set_max_registrations_per_block": Anonymize<Idv4d3rktbigfh>;
    /**
     * The extrinsic sets the subnet owner cut for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the subnet owner cut.
     */
    "sudo_set_subnet_owner_cut": Anonymize<I56j1e9gqlq602>;
    /**
     * The extrinsic sets the network rate limit for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the network rate limit.
     */
    "sudo_set_network_rate_limit": Anonymize<Ib6k4vik9ruq8h>;
    /**
     * The extrinsic sets the tempo for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the tempo.
     */
    "sudo_set_tempo": Anonymize<I9u9gu9aa92l5m>;
    /**
     * The extrinsic sets the total issuance for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the issuance for the network.
     */
    "sudo_set_total_issuance": Anonymize<Idmd4tos09qd68>;
    /**
     * The extrinsic sets the immunity period for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the immunity period for the network.
     */
    "sudo_set_network_immunity_period": Anonymize<Ia0sp2p68e9k16>;
    /**
     * The extrinsic sets the min lock cost for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the min lock cost for the network.
     */
    "sudo_set_network_min_lock_cost": Anonymize<Ie318529rgoagk>;
    /**
     * The extrinsic sets the subnet limit for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the subnet limit.
     */
    "sudo_set_subnet_limit": Anonymize<Iam4iou8r3isc1>;
    /**
     * The extrinsic sets the lock reduction interval for the network.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the lock reduction interval.
     */
    "sudo_set_lock_reduction_interval": Anonymize<I21ajnsdtbutjh>;
    /**
     * The extrinsic sets the recycled RAO for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the recycled RAO.
     */
    "sudo_set_rao_recycled": Anonymize<I203rofi4rpmo4>;
    /**
     * The extrinsic sets the weights min stake.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the weights min stake.
     */
    "sudo_set_stake_threshold": Anonymize<I1e290fmo892vi>;
    /**
     * The extrinsic sets the minimum stake required for nominators.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the minimum stake required for nominators.
     */
    "sudo_set_nominator_min_required_stake": Anonymize<I1e290fmo892vi>;
    /**
     * The extrinsic sets the rate limit for delegate take transactions.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the rate limit for delegate take transactions.
     */
    "sudo_set_tx_delegate_take_rate_limit": Anonymize<I3gk6eeddm0hsd>;
    /**
     * The extrinsic sets the minimum delegate take.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the minimum delegate take.
     */
    "sudo_set_min_delegate_take": Anonymize<I6ue7qc27uhiev>;
    /**
     * The extrinsic enabled/disables commit/reaveal for a given subnet.
     * It is only callable by the root account or subnet owner.
     * The extrinsic will call the Subtensor pallet to set the value.
     */
    "sudo_set_commit_reveal_weights_enabled": Anonymize<Ie31ro5s5e089f>;
    /**
     * Enables or disables Liquid Alpha for a given subnet.
     *
     * # Parameters
     * - `origin`: The origin of the call, which must be the root account or subnet owner.
     * - `netuid`: The unique identifier for the subnet.
     * - `enabled`: A boolean flag to enable or disable Liquid Alpha.
     *
     * # Weight
     * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
     */
    "sudo_set_liquid_alpha_enabled": Anonymize<Ie31ro5s5e089f>;
    /**
     * Sets values for liquid alpha
     */
    "sudo_set_alpha_values": Anonymize<I71lu4gpn88cf0>;
    /**
     * Sets the duration of the dissolve network schedule.
     *
     * This extrinsic allows the root account to set the duration for the dissolve network schedule.
     * The dissolve network schedule determines how long it takes for a network dissolution operation to complete.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the root account.
     * * `duration` - The new duration for the dissolve network schedule, in number of blocks.
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_dissolve_network_schedule_duration": Anonymize<I98iornf3ajrp9>;
    /**
     * Sets the commit-reveal weights periods for a specific subnet.
     *
     * This extrinsic allows the subnet owner or root account to set the duration (in epochs) during which committed weights must be revealed.
     * The commit-reveal mechanism ensures that users commit weights in advance and reveal them only within a specified period.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the subnet owner or the root account.
     * * `netuid` - The unique identifier of the subnet for which the periods are being set.
     * * `periods` - The number of epochs that define the commit-reveal period.
     *
     * # Errors
     * * `BadOrigin` - If the caller is neither the subnet owner nor the root account.
     * * `SubnetDoesNotExist` - If the specified subnet does not exist.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_commit_reveal_weights_interval": Anonymize<I9893mbk9nh201>;
    /**
     * Sets the EVM ChainID.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the subnet owner or the root account.
     * * `chainId` - The u64 chain ID
     *
     * # Errors
     * * `BadOrigin` - If the caller is neither the subnet owner nor the root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_evm_chain_id": Anonymize<I623eo8t3jrbeo>;
    /**
     * A public interface for `pallet_grandpa::Pallet::schedule_grandpa_change`.
     *
     * Schedule a change in the authorities.
     *
     * The change will be applied at the end of execution of the block `in_blocks` after the
     * current block. This value may be 0, in which case the change is applied at the end of
     * the current block.
     *
     * If the `forced` parameter is defined, this indicates that the current set has been
     * synchronously determined to be offline and that after `in_blocks` the given change
     * should be applied. The given block number indicates the median last finalized block
     * number and it should be used as the canon block when starting the new grandpa voter.
     *
     * No change should be signaled while any change is pending. Returns an error if a change
     * is already pending.
     */
    "schedule_grandpa_change": Anonymize<Ieo8qamskgm4dk>;
    /**
     * Enable or disable atomic alpha transfers for a given subnet.
     *
     * # Parameters
     * - `origin`: The origin of the call, which must be the root account or subnet owner.
     * - `netuid`: The unique identifier for the subnet.
     * - `enabled`: A boolean flag to enable or disable Liquid Alpha.
     *
     * # Weight
     * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
     */
    "sudo_set_toggle_transfer": Anonymize<Ift1efpssa32g2>;
    /**
     * Set the behaviour of the "burn" UID(s) for a given subnet.
     * If set to `Burn`, the miner emission sent to the burn UID(s) will be burned.
     * If set to `Recycle`, the miner emission sent to the burn UID(s) will be recycled.
     *
     * # Parameters
     * - `origin`: The origin of the call, which must be the root account or subnet owner.
     * - `netuid`: The unique identifier for the subnet.
     * - `recycle_or_burn`: The desired behaviour of the "burn" UID(s) for the subnet.
     *
     */
    "sudo_set_recycle_or_burn": Anonymize<Ibk3v0rrpo1bio>;
    /**
     * Toggles the enablement of an EVM precompile.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the root account.
     * * `precompile_id` - The identifier of the EVM precompile to toggle.
     * * `enabled` - The new enablement state of the precompile.
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_toggle_evm_precompile": Anonymize<I1sj8huj7of8mb>;
    /**
     *
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the root account.
     * * `alpha` - The new moving alpha value for the SubnetMovingAlpha.
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_subnet_moving_alpha": Anonymize<I6av3sq9jkhmm3>;
    /**
     * Change the SubnetOwnerHotkey for a given subnet.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the subnet owner.
     * * `netuid` - The unique identifier for the subnet.
     * * `hotkey` - The new hotkey for the subnet owner.
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the subnet owner or root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_subnet_owner_hotkey": Anonymize<I7f38r2vt6r9k1>;
    /**
     *
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the root account.
     * * `ema_alpha_period` - Number of blocks for EMA price to halve
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_ema_price_halving_period": Anonymize<I70cd7doki8rme>;
    /**
     *
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the root account.
     * * `netuid` - The unique identifier for the subnet.
     * * `steepness` - The Steepness for the alpha sigmoid function. (range is 0-int16::MAX,
     * negative values are reserved for future use)
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the root account.
     * * `SubnetDoesNotExist` - If the specified subnet does not exist.
     * * `NegativeSigmoidSteepness` - If the steepness is negative and the caller is
     * root.
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_alpha_sigmoid_steepness": Anonymize<Iam7j42j9f1go6>;
    /**
     * Enables or disables Yuma3 for a given subnet.
     *
     * # Parameters
     * - `origin`: The origin of the call, which must be the root account or subnet owner.
     * - `netuid`: The unique identifier for the subnet.
     * - `enabled`: A boolean flag to enable or disable Yuma3.
     *
     * # Weight
     * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
     */
    "sudo_set_yuma3_enabled": Anonymize<Ie31ro5s5e089f>;
    /**
     * Enables or disables Bonds Reset for a given subnet.
     *
     * # Parameters
     * - `origin`: The origin of the call, which must be the root account or subnet owner.
     * - `netuid`: The unique identifier for the subnet.
     * - `enabled`: A boolean flag to enable or disable Bonds Reset.
     *
     * # Weight
     * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
     */
    "sudo_set_bonds_reset_enabled": Anonymize<Ie31ro5s5e089f>;
    /**
     * Sets or updates the hotkey account associated with the owner of a specific subnet.
     *
     * This function allows either the root origin or the current subnet owner to set or update
     * the hotkey for a given subnet. The subnet must already exist. To prevent abuse, the call is
     * rate-limited to once per configured interval (default: one week) per subnet.
     *
     * # Parameters
     * - `origin`: The dispatch origin of the call. Must be either root or the current owner of the subnet.
     * - `netuid`: The unique identifier of the subnet whose owner hotkey is being set.
     * - `hotkey`: The new hotkey account to associate with the subnet owner.
     *
     * # Returns
     * - `DispatchResult`: Returns `Ok(())` if the hotkey was successfully set, or an appropriate error otherwise.
     *
     * # Errors
     * - `Error::SubnetNotExists`: If the specified subnet does not exist.
     * - `Error::TxRateLimitExceeded`: If the function is called more frequently than the allowed rate limit.
     *
     * # Access Control
     * Only callable by:
     * - Root origin, or
     * - The coldkey account that owns the subnet.
     *
     * # Storage
     * - Updates [`SubnetOwnerHotkey`] for the given `netuid`.
     * - Reads and updates [`LastRateLimitedBlock`] for rate-limiting.
     * - Reads [`DefaultSetSNOwnerHotkeyRateLimit`] to determine the interval between allowed updates.
     *
     * # Rate Limiting
     * This function is rate-limited to one call per subnet per interval (e.g., one week).
     */
    "sudo_set_sn_owner_hotkey": Anonymize<I7f38r2vt6r9k1>;
    /**
     * Enables or disables subtoken trading for a given subnet.
     *
     * # Arguments
     * * `origin` - The origin of the call, which must be the root account.
     * * `netuid` - The unique identifier of the subnet.
     * * `subtoken_enabled` - A boolean indicating whether subtoken trading should be enabled or disabled.
     *
     * # Errors
     * * `BadOrigin` - If the caller is not the root account.
     *
     * # Weight
     * Weight is handled by the `#[pallet::weight]` attribute.
     */
    "sudo_set_subtoken_enabled": Anonymize<Idco9ambhipg4i>;
    /**
     * Sets the commit-reveal weights version for all subnets
     */
    "sudo_set_commit_reveal_version": Anonymize<I6s1nbislhk619>;
    /**
     * Sets the number of immune owner neurons
     */
    "sudo_set_owner_immune_neuron_limit": Anonymize<I9jtu7slb30qvs>;
    /**
     * Sets the childkey burn for a subnet.
     * It is only callable by the root account.
     * The extrinsic will call the Subtensor pallet to set the childkey burn.
     */
    "sudo_set_ck_burn": Anonymize<Idv3j6a15pjc16>;
    /**
     * Sets the admin freeze window length (in blocks) at the end of a tempo.
     * Only callable by root.
     */
    "sudo_set_admin_freeze_window": Anonymize<I206qvjkjun95i>;
    /**
     * Sets the owner hyperparameter rate limit in epochs (global multiplier).
     * Only callable by root.
     */
    "sudo_set_owner_hparam_rate_limit": Anonymize<I4qhb3plq4ifmq>;
    /**
     * Sets the desired number of mechanisms in a subnet
     */
    "sudo_set_mechanism_count": Anonymize<Ic58lhlh1ocpm1>;
    /**
     * Sets the emission split between mechanisms in a subnet
     */
    "sudo_set_mechanism_emission_split": Anonymize<I6uopd4b2os90n>;
    /**
     * Trims the maximum number of UIDs for a subnet.
     *
     * The trimming is done by sorting the UIDs by emission descending and then trimming
     * the lowest emitters while preserving temporally and owner immune UIDs. The UIDs are
     * then compressed to the left and storage is migrated to the new compressed UIDs.
     */
    "sudo_trim_to_max_allowed_uids": Anonymize<I6idbvi8v00o5j>;
    /**
     * The extrinsic sets the minimum allowed UIDs for a subnet.
     * It is only callable by the root account.
     */
    "sudo_set_min_allowed_uids": Anonymize<Ifbgbhkj74b35k>;
    /**
     * Sets TAO flow cutoff value (A)
     */
    "sudo_set_tao_flow_cutoff": Anonymize<Ibt4a800kb7frq>;
    /**
     * Sets TAO flow normalization exponent (p)
     */
    "sudo_set_tao_flow_normalization_exponent": Anonymize<Icb4un8h4cokoo>;
    /**
     * Sets TAO flow smoothing factor (alpha)
     */
    "sudo_set_tao_flow_smoothing_factor": Anonymize<I1up607q6ce947>;
    /**
     * Sets the global maximum number of mechanisms in a subnet
     */
    "sudo_set_max_mechanism_count": Anonymize<I7hktg5sccf8op>;
    /**
     * Sets the minimum number of non-immortal & non-immune UIDs that must remain in a subnet
     */
    "sudo_set_min_non_immune_uids": Anonymize<Ib1d0bomkbrqv1>;
    /**
     * Sets the delay before a subnet can call start
     */
    "sudo_set_start_call_delay": Anonymize<Iaflrold1ds0nq>;
    /**
     * Sets the announcement delay for coldkey swap.
     */
    "sudo_set_coldkey_swap_announcement_delay": Anonymize<I98iornf3ajrp9>;
    /**
     * Sets the coldkey swap reannouncement delay.
     */
    "sudo_set_coldkey_swap_reannouncement_delay": Anonymize<I98iornf3ajrp9>;
}>;
export type I42mob3hqe6j7h = {
    "new_authorities": Anonymize<Ic5m5lp1oioo8r>;
};
export type Icdbq0j31b3g9c = {
    "default_take": number;
};
export type I2t2rlclb0ce3e = {
    "netuid": number;
    "serving_rate_limit": bigint;
};
export type Iar87gdqmug5o7 = {
    "netuid": number;
    "min_difficulty": bigint;
};
export type I3oullii9p80a1 = {
    "netuid": number;
    "max_difficulty": bigint;
};
export type I8t8ta6lfbia9e = {
    "netuid": number;
    "weights_version_key": bigint;
};
export type I3akfmjle982qg = {
    "netuid": number;
    "weights_set_rate_limit": bigint;
};
export type Ibaje86kdit7s6 = {
    "netuid": number;
    "adjustment_interval": number;
};
export type I90lra4vl5j4db = {
    "netuid": number;
    "adjustment_alpha": bigint;
};
export type I1q480m57ftcms = {
    "netuid": number;
    "immunity_period": number;
};
export type Ie2bjglo51atf6 = {
    "netuid": number;
    "min_allowed_weights": number;
};
export type Ievma38tc25kil = {
    "netuid": number;
    "max_allowed_uids": number;
};
export type I2er75v4akf5cc = {
    "netuid": number;
    "kappa": number;
};
export type I5pldh0j0v0u4l = {
    "netuid": number;
    "rho": number;
};
export type Ifhou5p0slv68r = {
    "netuid": number;
    "activity_cutoff": number;
};
export type I9m89dnau2i4tt = {
    "netuid": number;
    "registration_allowed": boolean;
};
export type Ifunpjbsc4jrrr = {
    "netuid": number;
    "target_registrations_per_interval": number;
};
export type I85uujfpnu8gum = {
    "netuid": number;
    "min_burn": bigint;
};
export type I7bl5t0it6ck2m = {
    "netuid": number;
    "max_burn": bigint;
};
export type I4iope0tjiqgu4 = {
    "netuid": number;
    "difficulty": bigint;
};
export type Iptqa236frcvo = {
    "netuid": number;
    "max_allowed_validators": number;
};
export type I8hbi1vrve1i2 = {
    "netuid": number;
    "bonds_moving_average": bigint;
};
export type I1v9a50gjqk26k = {
    "netuid": number;
    "bonds_penalty": number;
};
export type Idv4d3rktbigfh = {
    "netuid": number;
    "max_registrations_per_block": number;
};
export type I56j1e9gqlq602 = {
    "subnet_owner_cut": number;
};
export type Ib6k4vik9ruq8h = {
    "rate_limit": bigint;
};
export type I9u9gu9aa92l5m = {
    "netuid": number;
    "tempo": number;
};
export type Idmd4tos09qd68 = {
    "total_issuance": bigint;
};
export type Ia0sp2p68e9k16 = {
    "immunity_period": bigint;
};
export type Ie318529rgoagk = {
    "lock_cost": bigint;
};
export type Iam4iou8r3isc1 = {
    "max_subnets": number;
};
export type I21ajnsdtbutjh = {
    "interval": bigint;
};
export type I203rofi4rpmo4 = {
    "netuid": number;
    "rao_recycled": bigint;
};
export type I1e290fmo892vi = {
    "min_stake": bigint;
};
export type I71lu4gpn88cf0 = {
    "netuid": number;
    "alpha_low": number;
    "alpha_high": number;
};
export type I98iornf3ajrp9 = {
    "duration": number;
};
export type I9893mbk9nh201 = {
    "netuid": number;
    "interval": bigint;
};
export type I623eo8t3jrbeo = {
    "chain_id": bigint;
};
export type Ieo8qamskgm4dk = {
    "next_authorities": Anonymize<I3geksg000c171>;
    "in_blocks": number;
    "forced"?: Anonymize<I4arjljr6dpflb>;
};
export type Ift1efpssa32g2 = {
    "netuid": number;
    "toggle": boolean;
};
export type Ibk3v0rrpo1bio = {
    "netuid": number;
    "recycle_or_burn": Anonymize<Ib9tptuv3cggfs>;
};
export type I6av3sq9jkhmm3 = {
    "alpha": bigint;
};
export type I70cd7doki8rme = {
    "netuid": number;
    "ema_halving": bigint;
};
export type Iam7j42j9f1go6 = {
    "netuid": number;
    "steepness": number;
};
export type Idco9ambhipg4i = {
    "netuid": number;
    "subtoken_enabled": boolean;
};
export type I6s1nbislhk619 = {
    "version": number;
};
export type I9jtu7slb30qvs = {
    "netuid": number;
    "immune_neurons": number;
};
export type Idv3j6a15pjc16 = {
    "burn": bigint;
};
export type I206qvjkjun95i = {
    "window": number;
};
export type I4qhb3plq4ifmq = {
    "epochs": number;
};
export type Ic58lhlh1ocpm1 = {
    "netuid": number;
    "mechanism_count": number;
};
export type I6uopd4b2os90n = {
    "netuid": number;
    "maybe_split"?: Anonymize<I35lk2003i8c8g>;
};
export type I35lk2003i8c8g = (Anonymize<Icgljjb6j82uhn>) | undefined;
export type I6idbvi8v00o5j = {
    "netuid": number;
    "max_n": number;
};
export type Ifbgbhkj74b35k = {
    "netuid": number;
    "min_allowed_uids": number;
};
export type Ibt4a800kb7frq = {
    "flow_cutoff": bigint;
};
export type Icb4un8h4cokoo = {
    "exponent": bigint;
};
export type I1up607q6ce947 = {
    "smoothing_factor": bigint;
};
export type I7hktg5sccf8op = {
    "max_mechanism_count": number;
};
export type Ib1d0bomkbrqv1 = {
    "netuid": number;
    "min": number;
};
export type Iaflrold1ds0nq = {
    "delay": bigint;
};
export type I48eehof2eias5 = AnonymousEnum<{
    /**
     * Enter safe-mode permissionlessly for [`Config::EnterDuration`] blocks.
     *
     * Reserves [`Config::EnterDepositAmount`] from the caller's account.
     * Emits an [`Event::Entered`] event on success.
     * Errors with [`Error::Entered`] if the safe-mode is already entered.
     * Errors with [`Error::NotConfigured`] if the deposit amount is `None`.
     */
    "enter": undefined;
    /**
     * Enter safe-mode by force for a per-origin configured number of blocks.
     *
     * Emits an [`Event::Entered`] event on success.
     * Errors with [`Error::Entered`] if the safe-mode is already entered.
     *
     * Can only be called by the [`Config::ForceEnterOrigin`] origin.
     */
    "force_enter": undefined;
    /**
     * Extend the safe-mode permissionlessly for [`Config::ExtendDuration`] blocks.
     *
     * This accumulates on top of the current remaining duration.
     * Reserves [`Config::ExtendDepositAmount`] from the caller's account.
     * Emits an [`Event::Extended`] event on success.
     * Errors with [`Error::Exited`] if the safe-mode is entered.
     * Errors with [`Error::NotConfigured`] if the deposit amount is `None`.
     *
     * This may be called by any signed origin with [`Config::ExtendDepositAmount`] free
     * currency to reserve. This call can be disabled for all origins by configuring
     * [`Config::ExtendDepositAmount`] to `None`.
     */
    "extend": undefined;
    /**
     * Extend the safe-mode by force for a per-origin configured number of blocks.
     *
     * Emits an [`Event::Extended`] event on success.
     * Errors with [`Error::Exited`] if the safe-mode is inactive.
     *
     * Can only be called by the [`Config::ForceExtendOrigin`] origin.
     */
    "force_extend": undefined;
    /**
     * Exit safe-mode by force.
     *
     * Emits an [`Event::Exited`] with [`ExitReason::Force`] event on success.
     * Errors with [`Error::Exited`] if the safe-mode is inactive.
     *
     * Note: `safe-mode` will be automatically deactivated by [`Pallet::on_initialize`] hook
     * after the block height is greater than the [`EnteredUntil`] storage item.
     * Emits an [`Event::Exited`] with [`ExitReason::Timeout`] event when deactivated in the
     * hook.
     */
    "force_exit": undefined;
    /**
     * Slash a deposit for an account that entered or extended safe-mode at a given
     * historical block.
     *
     * This can only be called while safe-mode is entered.
     *
     * Emits a [`Event::DepositSlashed`] event on success.
     * Errors with [`Error::Entered`] if safe-mode is entered.
     *
     * Can only be called by the [`Config::ForceDepositOrigin`] origin.
     */
    "force_slash_deposit": Anonymize<I1ssp78ejl639m>;
    /**
     * Permissionlessly release a deposit for an account that entered safe-mode at a
     * given historical block.
     *
     * The call can be completely disabled by setting [`Config::ReleaseDelay`] to `None`.
     * This cannot be called while safe-mode is entered and not until
     * [`Config::ReleaseDelay`] blocks have passed since safe-mode was entered.
     *
     * Emits a [`Event::DepositReleased`] event on success.
     * Errors with [`Error::Entered`] if the safe-mode is entered.
     * Errors with [`Error::CannotReleaseYet`] if [`Config::ReleaseDelay`] block have not
     * passed since safe-mode was entered. Errors with [`Error::NoDeposit`] if the payee has no
     * reserved currency at the block specified.
     */
    "release_deposit": Anonymize<I1ssp78ejl639m>;
    /**
     * Force to release a deposit for an account that entered safe-mode at a given
     * historical block.
     *
     * This can be called while safe-mode is still entered.
     *
     * Emits a [`Event::DepositReleased`] event on success.
     * Errors with [`Error::Entered`] if safe-mode is entered.
     * Errors with [`Error::NoDeposit`] if the payee has no reserved currency at the
     * specified block.
     *
     * Can only be called by the [`Config::ForceDepositOrigin`] origin.
     */
    "force_release_deposit": Anonymize<I1ssp78ejl639m>;
}>;
export type I1ssp78ejl639m = {
    "account": SS58String;
    "block": number;
};
export type I3lo8is2egp8k4 = AnonymousEnum<{
    /**
     * Transact an Ethereum transaction.
     */
    "transact": Anonymize<I13qib3vtm9cs3>;
}>;
export type I13qib3vtm9cs3 = {
    "transaction": Anonymize<Ibjuap2vk03rp6>;
};
export type Iafltn68socb5h = AnonymousEnum<{
    /**
     * Withdraw balance from EVM into currency/balances pallet.
     */
    "withdraw": Anonymize<Idcabvplu05lea>;
    /**
     * Issue an EVM call operation. This is similar to a message call transaction in Ethereum.
     */
    "call": Anonymize<Id38gdpcotl637>;
    /**
     * Issue an EVM create operation. This is similar to a contract creation transaction in
     * Ethereum.
     */
    "create": Anonymize<I73q3qf5u7nnqg>;
    /**
     * Issue an EVM create2 operation.
     */
    "create2": Anonymize<Idpm1bc2cr6dgj>;
    "set_whitelist": Anonymize<I837c61fc07ine>;
    "disable_whitelist": Anonymize<I6m0oguilvhn8>;
}>;
export type Idcabvplu05lea = {
    "address": FixedSizeBinary<20>;
    "value": bigint;
};
export type Id38gdpcotl637 = {
    "source": FixedSizeBinary<20>;
    "target": FixedSizeBinary<20>;
    "input": Binary;
    "value": Anonymize<I4totqt881mlti>;
    "gas_limit": bigint;
    "max_fee_per_gas": Anonymize<I4totqt881mlti>;
    "max_priority_fee_per_gas"?: Anonymize<Ic4rgfgksgmm3e>;
    "nonce"?: Anonymize<Ic4rgfgksgmm3e>;
    "access_list": Anonymize<I1bsfec060j604>;
    "authorization_list": Anonymize<Idg0qi60379vnh>;
};
export type Ic4rgfgksgmm3e = (Anonymize<I4totqt881mlti>) | undefined;
export type I1bsfec060j604 = Array<[FixedSizeBinary<20>, Anonymize<Ic5m5lp1oioo8r>]>;
export type I73q3qf5u7nnqg = {
    "source": FixedSizeBinary<20>;
    "init": Binary;
    "value": Anonymize<I4totqt881mlti>;
    "gas_limit": bigint;
    "max_fee_per_gas": Anonymize<I4totqt881mlti>;
    "max_priority_fee_per_gas"?: Anonymize<Ic4rgfgksgmm3e>;
    "nonce"?: Anonymize<Ic4rgfgksgmm3e>;
    "access_list": Anonymize<I1bsfec060j604>;
    "authorization_list": Anonymize<Idg0qi60379vnh>;
};
export type Idpm1bc2cr6dgj = {
    "source": FixedSizeBinary<20>;
    "init": Binary;
    "salt": FixedSizeBinary<32>;
    "value": Anonymize<I4totqt881mlti>;
    "gas_limit": bigint;
    "max_fee_per_gas": Anonymize<I4totqt881mlti>;
    "max_priority_fee_per_gas"?: Anonymize<Ic4rgfgksgmm3e>;
    "nonce"?: Anonymize<Ic4rgfgksgmm3e>;
    "access_list": Anonymize<I1bsfec060j604>;
    "authorization_list": Anonymize<Idg0qi60379vnh>;
};
export type I837c61fc07ine = {
    "new": Anonymize<I4gqmlq9k6jlk3>;
};
export type I6m0oguilvhn8 = {
    "disabled": boolean;
};
export type I2aqcjbjlffus = AnonymousEnum<{
    "set_base_fee_per_gas": Anonymize<I7vi74gbubc8u5>;
    "set_elasticity": Anonymize<I3u0knmtb1ueq7>;
}>;
export type Ibdf4fkp7qcokd = AnonymousEnum<{
    /**
     * Verify and write a pulse from the beacon into the runtime
     */
    "write_pulse": Anonymize<I87tlou92i0bot>;
    /**
     * allows the root user to set the beacon configuration
     * generally this would be called from an offchain worker context.
     * there is no verification of configurations, so be careful with this.
     *
     * * `origin`: the root user
     * * `config`: the beacon configuration
     */
    "set_beacon_config": Anonymize<Ifd3mkud9g8rb1>;
    /**
     * allows the root user to set the oldest stored round
     */
    "set_oldest_stored_round": Anonymize<Iakvbbhvger3oa>;
}>;
export type I87tlou92i0bot = {
    "pulses_payload": {
        "block_number": number;
        "pulses": Array<Anonymize<Ialchst9lgd11u>>;
        "public": MultiSigner;
    };
    "signature"?: Anonymize<I86cdjmsf3a81s>;
};
export type MultiSigner = Enum<{
    "Ed25519": FixedSizeBinary<32>;
    "Sr25519": FixedSizeBinary<32>;
    "Ecdsa": FixedSizeBinary<33>;
}>;
export declare const MultiSigner: GetEnum<MultiSigner>;
export type I86cdjmsf3a81s = (MultiSignature) | undefined;
export type MultiSignature = Enum<{
    "Ed25519": FixedSizeBinary<64>;
    "Sr25519": FixedSizeBinary<64>;
    "Ecdsa": FixedSizeBinary<65>;
}>;
export declare const MultiSignature: GetEnum<MultiSignature>;
export type Ifd3mkud9g8rb1 = {
    "config_payload": {
        "block_number": number;
        "config": Anonymize<I494mq1ertfc9k>;
        "public": MultiSigner;
    };
    "signature"?: Anonymize<I86cdjmsf3a81s>;
};
export type Iakvbbhvger3oa = {
    "oldest_round": bigint;
};
export type I6nul30pateutj = AnonymousEnum<{
    /**
     * Create a crowdloan that will raise funds up to a maximum cap and if successful,
     * will transfer funds to the target address if provided and dispatch the call
     * (using creator origin).
     *
     * The initial deposit will be transfered to the crowdloan account and will be refunded
     * in case the crowdloan fails to raise the cap. Additionally, the creator will pay for
     * the execution of the call.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `deposit`: The initial deposit from the creator.
     * - `min_contribution`: The minimum contribution required to contribute to the crowdloan.
     * - `cap`: The maximum amount of funds that can be raised.
     * - `end`: The block number at which the crowdloan will end.
     * - `call`: The call to dispatch when the crowdloan is finalized.
     * - `target_address`: The address to transfer the raised funds to if provided.
     */
    "create": Anonymize<I92t98snpjjcts>;
    /**
     * Contribute to an active crowdloan.
     *
     * The contribution will be transfered to the crowdloan account and will be refunded
     * if the crowdloan fails to raise the cap. If the contribution would raise the amount above the cap,
     * the contribution will be set to the amount that is left to be raised.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to contribute to.
     * - `amount`: The amount to contribute.
     */
    "contribute": Anonymize<Iet4pe2le7ku09>;
    /**
     * Withdraw a contribution from an active (not yet finalized or dissolved) crowdloan.
     *
     * Only contributions over the deposit can be withdrawn by the creator.
     *
     * The dispatch origin for this call must be _Signed_.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to withdraw from.
     */
    "withdraw": Anonymize<I5dueehi6i2dg9>;
    /**
     * Finalize crowdloan that has reached the cap.
     *
     * The call will transfer the raised amount to the target address if it was provided when the crowdloan was created
     * and dispatch the call that was provided using the creator origin. The CurrentCrowdloanId will be set to the
     * crowdloan id being finalized so the dispatched call can access it temporarily by accessing
     * the `CurrentCrowdloanId` storage item.
     *
     * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to finalize.
     */
    "finalize": Anonymize<I5dueehi6i2dg9>;
    /**
     * Refund contributors of a non-finalized crowdloan.
     *
     * The call will try to refund all contributors (excluding the creator) up to the limit defined by the `RefundContributorsLimit`.
     * If the limit is reached, the call will stop and the crowdloan will be marked as partially refunded.
     * It may be needed to dispatch this call multiple times to refund all contributors.
     *
     * The dispatch origin for this call must be _Signed_ and doesn't need to be the creator of the crowdloan.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to refund.
     */
    "refund": Anonymize<I5dueehi6i2dg9>;
    /**
     * Dissolve a crowdloan.
     *
     * The crowdloan will be removed from the storage.
     * All contributions must have been refunded before the crowdloan can be dissolved (except the creator's one).
     *
     * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to dissolve.
     */
    "dissolve": Anonymize<I5dueehi6i2dg9>;
    /**
     * Update the minimum contribution of a non-finalized crowdloan.
     *
     * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to update the minimum contribution of.
     * - `new_min_contribution`: The new minimum contribution.
     */
    "update_min_contribution": Anonymize<I64ev05f6q10es>;
    /**
     * Update the end block of a non-finalized crowdloan.
     *
     * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to update the end block of.
     * - `new_end`: The new end block.
     */
    "update_end": Anonymize<Ikc5h15joooak>;
    /**
     * Update the cap of a non-finalized crowdloan.
     *
     * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
     *
     * Parameters:
     * - `crowdloan_id`: The id of the crowdloan to update the cap of.
     * - `new_cap`: The new cap.
     */
    "update_cap": Anonymize<Ie8f436ua5fs59>;
}>;
export type I92t98snpjjcts = {
    "deposit": bigint;
    "min_contribution": bigint;
    "cap": bigint;
    "end": number;
    "call"?: (TxCallData) | undefined;
    "target_address"?: Anonymize<Ihfphjolmsqq1>;
};
export type Iet4pe2le7ku09 = {
    "crowdloan_id": number;
    "amount": bigint;
};
export type Id0qcnu1chac12 = AnonymousEnum<{
    /**
     * Set the fee rate for swaps on a specific subnet (normalized value).
     * For example, 0.3% is approximately 196.
     *
     * Only callable by the admin origin
     */
    "set_fee_rate": Anonymize<I3mkis681qg30e>;
    /**
     * Enable user liquidity operations for a specific subnet. This switches the
     * subnet from V2 to V3 swap mode. Thereafter, adding new user liquidity can be disabled
     * by toggling this flag to false, but the swap mode will remain V3 because of existing
     * user liquidity until all users withdraw their liquidity.
     *
     * Only sudo or subnet owner can enable user liquidity.
     * Only sudo can disable user liquidity.
     */
    "toggle_user_liquidity": Anonymize<I2foqo7cbqf35v>;
    /**
     * Add liquidity to a specific price range for a subnet.
     *
     * Parameters:
     * - origin: The origin of the transaction
     * - netuid: Subnet ID
     * - tick_low: Lower bound of the price range
     * - tick_high: Upper bound of the price range
     * - liquidity: Amount of liquidity to add
     *
     * Emits `Event::LiquidityAdded` on success
     */
    "add_liquidity": Anonymize<I3mcu79ge1e54v>;
    /**
     * Remove liquidity from a specific position.
     *
     * Parameters:
     * - origin: The origin of the transaction
     * - netuid: Subnet ID
     * - position_id: ID of the position to remove
     *
     * Emits `Event::LiquidityRemoved` on success
     */
    "remove_liquidity": Anonymize<Icf66vuktncksu>;
    /**
     * Modify a liquidity position.
     *
     * Parameters:
     * - origin: The origin of the transaction
     * - netuid: Subnet ID
     * - position_id: ID of the position to remove
     * - liquidity_delta: Liquidity to add (if positive) or remove (if negative)
     *
     * Emits `Event::LiquidityRemoved` on success
     */
    "modify_position": Anonymize<Id69glo8rcjef>;
    /**
     * Disable user liquidity in all subnets.
     *
     * Emits `Event::UserLiquidityToggled` on success
     */
    "disable_lp": undefined;
}>;
export type I3mcu79ge1e54v = {
    "hotkey": SS58String;
    "netuid": number;
    "tick_low": number;
    "tick_high": number;
    "liquidity": bigint;
};
export type Icf66vuktncksu = {
    "hotkey": SS58String;
    "netuid": number;
    "position_id": bigint;
};
export type Id69glo8rcjef = {
    "hotkey": SS58String;
    "netuid": number;
    "position_id": bigint;
    "liquidity_delta": bigint;
};
export type I6jivj2j5qp8sa = AnonymousEnum<{
    /**
     * Deprecated version if [`Self::call`] for use in an in-storage `Call`.
     */
    "call_old_weight": Anonymize<Ia2rnh5pfua40a>;
    /**
     * Deprecated version if [`Self::instantiate_with_code`] for use in an in-storage `Call`.
     */
    "instantiate_with_code_old_weight": Anonymize<I3otc7e9a35k1k>;
    /**
     * Deprecated version if [`Self::instantiate`] for use in an in-storage `Call`.
     */
    "instantiate_old_weight": Anonymize<I89ier5tb9ne0s>;
    /**
     * Upload new `code` without instantiating a contract from it.
     *
     * If the code does not already exist a deposit is reserved from the caller
     * and unreserved only when [`Self::remove_code`] is called. The size of the reserve
     * depends on the size of the supplied `code`.
     *
     * If the code already exists in storage it will still return `Ok` and upgrades
     * the in storage version to the current
     * [`InstructionWeights::version`](InstructionWeights).
     *
     * - `determinism`: If this is set to any other value but [`Determinism::Enforced`] then
     * the only way to use this code is to delegate call into it from an offchain execution.
     * Set to [`Determinism::Enforced`] if in doubt.
     *
     * # Note
     *
     * Anyone can instantiate a contract from any uploaded code and thus prevent its removal.
     * To avoid this situation a constructor could employ access control so that it can
     * only be instantiated by permissioned entities. The same is true when uploading
     * through [`Self::instantiate_with_code`].
     *
     * Use [`Determinism::Relaxed`] exclusively for non-deterministic code. If the uploaded
     * code is deterministic, specifying [`Determinism::Relaxed`] will be disregarded and
     * result in higher gas costs.
     */
    "upload_code": Anonymize<Im2f0numhevg3>;
    /**
     * Remove the code stored under `code_hash` and refund the deposit to its owner.
     *
     * A code can only be removed by its original uploader (its owner) and only if it is
     * not used by any contract.
     */
    "remove_code": Anonymize<Ib51vk42m1po4n>;
    /**
     * Privileged function that changes the code of an existing contract.
     *
     * This takes care of updating refcounts and all other necessary operations. Returns
     * an error if either the `code_hash` or `dest` do not exist.
     *
     * # Note
     *
     * This does **not** change the address of the contract in question. This means
     * that the contract address is no longer derived from its code hash after calling
     * this dispatchable.
     */
    "set_code": Anonymize<I2agkcpojhkk43>;
    /**
     * Makes a call to an account, optionally transferring some balance.
     *
     * # Parameters
     *
     * * `dest`: Address of the contract to call.
     * * `value`: The balance to transfer from the `origin` to `dest`.
     * * `gas_limit`: The gas limit enforced when executing the constructor.
     * * `storage_deposit_limit`: The maximum amount of balance that can be charged from the
     * caller to pay for the storage consumed.
     * * `data`: The input data to pass to the contract.
     *
     * * If the account is a smart-contract account, the associated code will be
     * executed and any value will be transferred.
     * * If the account is a regular account, any value will be transferred.
     * * If no account exists and the call value is not less than `existential_deposit`,
     * a regular account will be created and any value will be transferred.
     */
    "call": Anonymize<I32rvg545edabm>;
    /**
     * Instantiates a new contract from the supplied `code` optionally transferring
     * some balance.
     *
     * This dispatchable has the same effect as calling [`Self::upload_code`] +
     * [`Self::instantiate`]. Bundling them together provides efficiency gains. Please
     * also check the documentation of [`Self::upload_code`].
     *
     * # Parameters
     *
     * * `value`: The balance to transfer from the `origin` to the newly created contract.
     * * `gas_limit`: The gas limit enforced when executing the constructor.
     * * `storage_deposit_limit`: The maximum amount of balance that can be charged/reserved
     * from the caller to pay for the storage consumed.
     * * `code`: The contract code to deploy in raw bytes.
     * * `data`: The input data to pass to the contract constructor.
     * * `salt`: Used for the address derivation. See [`Pallet::contract_address`].
     *
     * Instantiation is executed as follows:
     *
     * - The supplied `code` is deployed, and a `code_hash` is created for that code.
     * - If the `code_hash` already exists on the chain the underlying `code` will be shared.
     * - The destination address is computed based on the sender, code_hash and the salt.
     * - The smart-contract account is created at the computed address.
     * - The `value` is transferred to the new account.
     * - The `deploy` function is executed in the context of the newly-created account.
     */
    "instantiate_with_code": Anonymize<I83fv0vi59md7i>;
    /**
     * Instantiates a contract from a previously deployed wasm binary.
     *
     * This function is identical to [`Self::instantiate_with_code`] but without the
     * code deployment step. Instead, the `code_hash` of an on-chain deployed wasm binary
     * must be supplied.
     */
    "instantiate": Anonymize<I5tjjqcdd4tae0>;
    /**
     * When a migration is in progress, this dispatchable can be used to run migration steps.
     * Calls that contribute to advancing the migration have their fees waived, as it's helpful
     * for the chain. Note that while the migration is in progress, the pallet will also
     * leverage the `on_idle` hooks to run migration steps.
     */
    "migrate": Anonymize<I1894dm1lf1ae7>;
}>;
export type Ia2rnh5pfua40a = {
    "dest": MultiAddress;
    "value": bigint;
    "gas_limit": bigint;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "data": Binary;
};
export type I3otc7e9a35k1k = {
    "value": bigint;
    "gas_limit": bigint;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "code": Binary;
    "data": Binary;
    "salt": Binary;
};
export type I89ier5tb9ne0s = {
    "value": bigint;
    "gas_limit": bigint;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "code_hash": FixedSizeBinary<32>;
    "data": Binary;
    "salt": Binary;
};
export type Im2f0numhevg3 = {
    "code": Binary;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "determinism": Anonymize<I2dfliekq1ed7e>;
};
export type I2agkcpojhkk43 = {
    "dest": MultiAddress;
    "code_hash": FixedSizeBinary<32>;
};
export type I32rvg545edabm = {
    "dest": MultiAddress;
    "value": bigint;
    "gas_limit": Anonymize<I4q39t5hn830vp>;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "data": Binary;
};
export type I83fv0vi59md7i = {
    "value": bigint;
    "gas_limit": Anonymize<I4q39t5hn830vp>;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "code": Binary;
    "data": Binary;
    "salt": Binary;
};
export type I5tjjqcdd4tae0 = {
    "value": bigint;
    "gas_limit": Anonymize<I4q39t5hn830vp>;
    "storage_deposit_limit"?: Anonymize<I35p85j063s0il>;
    "code_hash": FixedSizeBinary<32>;
    "data": Binary;
    "salt": Binary;
};
export type I1894dm1lf1ae7 = {
    "weight_limit": Anonymize<I4q39t5hn830vp>;
};
export type I1o2fkthdkdbjl = AnonymousEnum<{
    /**
     * Announce the ML‑KEM public key that will become `CurrentKey` in
     * the following block.
     */
    "announce_next_key": Anonymize<Idkfsqnep2hpeb>;
    /**
     * Users submit an encrypted wrapper.
     *
     * Client‑side:
     *
     * 1. Read `NextKey` (ML‑KEM public key bytes) from storage.
     * 2. Sign your extrinsic so that it can be executed when added to the pool,
     * i.e. you may need to increment the nonce if you submit using the same account.
     * 3. `commitment = Hashing::hash(signed_extrinsic)`.
     * 4. Encrypt:
     *
     * plaintext = signed_extrinsic
     *
     * with ML‑KEM‑768 + XChaCha20‑Poly1305, producing
     *
     * ciphertext = [u16 kem_len] || kem_ct || nonce24 || aead_ct
     *
     */
    "submit_encrypted": Anonymize<I2u5b4034ft9hp>;
    /**
     * Marks a submission as failed to decrypt and removes it from storage.
     *
     * Called by the block author when decryption fails at any stage (e.g., ML-KEM decapsulate
     * failed, AEAD decrypt failed, invalid ciphertext format, etc.). This allows clients to be
     * notified of decryption failures through on-chain events.
     *
     * # Arguments
     *
     * * `id` - The wrapper id (hash of (author, commitment, ciphertext))
     * * `reason` - Human-readable reason for the decryption failure (e.g., "ML-KEM decapsulate failed")
     */
    "mark_decryption_failed": Anonymize<I602p6mm30elei>;
}>;
export type Idkfsqnep2hpeb = {
    "public_key": Binary;
};
export type I2u5b4034ft9hp = {
    "commitment": FixedSizeBinary<32>;
    "ciphertext": Binary;
};
export type I8vbtb6bd00lm0 = AnonymousEnum<{
    "System": Anonymize<Iekve0i6djpd9f>;
    "Timestamp": Anonymize<I7d75gqfg6jh9c>;
    "Grandpa": Anonymize<Ibck9ekr2i96uj>;
    "Balances": Anonymize<I9svldsp29mh87>;
    "SubtensorModule": Anonymize<I70bgb5j7tau9l>;
    "Utility": Anonymize<If4ks1adspb2ai>;
    "Sudo": Anonymize<Ic3n0u1krodt07>;
    "Multisig": Anonymize<I92oc1s48b79mg>;
    "Preimage": Anonymize<If81ks88t5mpk5>;
    "Scheduler": Anonymize<I8joseop2odch3>;
    "Proxy": Anonymize<Ia2ee5fnr4ukkf>;
    "Registry": Anonymize<Ifml9odtov51l3>;
    "Commitments": Anonymize<I5bqhvupj937er>;
    "AdminUtils": Anonymize<Iemvun0dttbcqs>;
    "SafeMode": Anonymize<I48eehof2eias5>;
    "Ethereum": Anonymize<I3lo8is2egp8k4>;
    "EVM": Anonymize<Iafltn68socb5h>;
    "BaseFee": Anonymize<I2aqcjbjlffus>;
    "Drand": Anonymize<Ibdf4fkp7qcokd>;
    "Crowdloan": Anonymize<I6nul30pateutj>;
    "Swap": Anonymize<Id0qcnu1chac12>;
    "Contracts": Anonymize<I6jivj2j5qp8sa>;
    "MevShield": Anonymize<I1o2fkthdkdbjl>;
}>;
export type Iaqet9jc3ihboe = {
    "header": Anonymize<Ic952bubvq4k7d>;
    "extrinsics": Anonymize<Itom7fk49o0c9>;
};
export type Ic952bubvq4k7d = {
    "parent_hash": FixedSizeBinary<32>;
    "number": number;
    "state_root": FixedSizeBinary<32>;
    "extrinsics_root": FixedSizeBinary<32>;
    "digest": Anonymize<I4mddgoa69c0a2>;
};
export type I2v50gu3s1aqk6 = AnonymousEnum<{
    "AllExtrinsics": undefined;
    "OnlyInherents": undefined;
}>;
export type Ibmofsd95figtn = ResultPayload<Anonymize<Ibq6c27da62s2q>, Anonymize<I5nrjkj9qumobs>>;
export type I5nrjkj9qumobs = AnonymousEnum<{
    "Invalid": Enum<{
        "Call": undefined;
        "Payment": undefined;
        "Future": undefined;
        "Stale": undefined;
        "BadProof": undefined;
        "AncientBirthBlock": undefined;
        "ExhaustsResources": undefined;
        "Custom": number;
        "BadMandatory": undefined;
        "MandatoryValidation": undefined;
        "BadSigner": undefined;
        "IndeterminateImplicit": undefined;
        "UnknownOrigin": undefined;
    }>;
    "Unknown": TransactionValidityUnknownTransaction;
}>;
export type TransactionValidityUnknownTransaction = Enum<{
    "CannotLookup": undefined;
    "NoUnsignedValidator": undefined;
    "Custom": number;
}>;
export declare const TransactionValidityUnknownTransaction: GetEnum<TransactionValidityUnknownTransaction>;
export type If7uv525tdvv7a = Array<[FixedSizeBinary<8>, Binary]>;
export type I2an1fs2eiebjp = {
    "okay": boolean;
    "fatal_error": boolean;
    "errors": Anonymize<If7uv525tdvv7a>;
};
export type Ie9sr1iqcg3cgm = ResultPayload<undefined, string>;
export type I1mqgk2tmnn9i2 = (string) | undefined;
export type I6lr8sctk0bi4e = Array<string>;
export type TransactionValidityTransactionSource = Enum<{
    "InBlock": undefined;
    "Local": undefined;
    "External": undefined;
}>;
export declare const TransactionValidityTransactionSource: GetEnum<TransactionValidityTransactionSource>;
export type I9ask1o4tfvcvs = ResultPayload<{
    "priority": bigint;
    "requires": Anonymize<Itom7fk49o0c9>;
    "provides": Anonymize<Itom7fk49o0c9>;
    "longevity": bigint;
    "propagate": boolean;
}, Anonymize<I5nrjkj9qumobs>>;
export type Icerf8h8pdu8ss = (Array<[Binary, FixedSizeBinary<4>]>) | undefined;
export type I6spmpef2c7svf = {
    "weight": Anonymize<I4q39t5hn830vp>;
    "class": DispatchClass;
    "partial_fee": bigint;
};
export type Iei2mvq0mjvt81 = {
    "inclusion_fee"?: ({
        "base_fee": bigint;
        "len_fee": bigint;
        "adjusted_weight_fee": bigint;
    }) | undefined;
    "tip": bigint;
};
export type If08sfhqn8ujfr = {
    "balance": Anonymize<I4totqt881mlti>;
    "nonce": Anonymize<I4totqt881mlti>;
};
export type I3dj14b7k3rkm5 = (Anonymize<I1bsfec060j604>) | undefined;
export type Ic5egmm215ml6k = (Anonymize<Idg0qi60379vnh>) | undefined;
export type Ibg4am9lqg35ku = ResultPayload<{
    "exit_reason": Anonymize<Iag9iovb9j5ijo>;
    "value": Binary;
    "used_gas": Anonymize<I8mgv59to1hjie>;
    "weight_info"?: Anonymize<Ib72ii9bshc8f5>;
    "logs": Anonymize<Ids7ng2qsv7snu>;
}, Anonymize<Ic871mj76419vm>>;
export type I8mgv59to1hjie = {
    "standard": Anonymize<I4totqt881mlti>;
    "effective": Anonymize<I4totqt881mlti>;
};
export type Ib72ii9bshc8f5 = ({
    "ref_time_limit"?: Anonymize<I35p85j063s0il>;
    "proof_size_limit"?: Anonymize<I35p85j063s0il>;
    "ref_time_usage"?: Anonymize<I35p85j063s0il>;
    "proof_size_usage"?: Anonymize<I35p85j063s0il>;
}) | undefined;
export type I7efspe2svrt0g = ResultPayload<{
    "exit_reason": Anonymize<Iag9iovb9j5ijo>;
    "value": FixedSizeBinary<20>;
    "used_gas": Anonymize<I8mgv59to1hjie>;
    "weight_info"?: Anonymize<Ib72ii9bshc8f5>;
    "logs": Anonymize<Ids7ng2qsv7snu>;
}, Anonymize<Ic871mj76419vm>>;
export type I5fvdd841odbi3 = (Anonymize<Ib0hfhkohlekcj>) | undefined;
export type I35vouom6s9r2 = (Anonymize<I32lgu058i52q9>) | undefined;
export type Ie6kgk6f04rsvk = (Anonymize<Ie7atdsih6q14b>) | undefined;
export type Ifgqf2rskq94om = [Anonymize<I5fvdd841odbi3>, Anonymize<I35vouom6s9r2>, Anonymize<Ie6kgk6f04rsvk>];
export type I7aold6s47n103 = [Anonymize<I5fvdd841odbi3>, Anonymize<Ie6kgk6f04rsvk>];
export type Ifla7g8u5j9k68 = {
    "gas_consumed": Anonymize<I4q39t5hn830vp>;
    "gas_required": Anonymize<I4q39t5hn830vp>;
    "storage_deposit": Anonymize<If7bmpttbdmqu4>;
    "debug_message": Binary;
    "result": ResultPayload<Anonymize<I620n7irgfspm4>, Anonymize<Ic871mj76419vm>>;
    "events"?: Anonymize<I6taghk79roh1q>;
};
export type If7bmpttbdmqu4 = AnonymousEnum<{
    "Refund": bigint;
    "Charge": bigint;
}>;
export type I620n7irgfspm4 = {
    "flags": number;
    "data": Binary;
};
export type I6taghk79roh1q = (Anonymize<Icr4vaj0vrd6je>) | undefined;
export type I9sijb8gfrns29 = AnonymousEnum<{
    "Upload": Binary;
    "Existing": FixedSizeBinary<32>;
}>;
export type I17s97pb2d5tj3 = {
    "gas_consumed": Anonymize<I4q39t5hn830vp>;
    "gas_required": Anonymize<I4q39t5hn830vp>;
    "storage_deposit": Anonymize<If7bmpttbdmqu4>;
    "debug_message": Binary;
    "result": ResultPayload<{
        "result": Anonymize<I620n7irgfspm4>;
        "account_id": SS58String;
    }, Anonymize<Ic871mj76419vm>>;
    "events"?: Anonymize<I6taghk79roh1q>;
};
export type I4gah17u2nc33h = ResultPayload<{
    "code_hash": FixedSizeBinary<32>;
    "deposit": bigint;
}, Anonymize<Ic871mj76419vm>>;
export type I9u22scd4ksrjm = ResultPayload<Anonymize<Iabpgqcjikia83>, Enum<{
    "DoesntExist": undefined;
    "KeyDecodingFailed": undefined;
    "MigrationInProgress": undefined;
}>>;
export type Ibil6rvg3saeb3 = Array<Anonymize<I4dh58q3tkaf4j>>;
export type I4dh58q3tkaf4j = {
    "delegate_ss58": SS58String;
    "take": number;
    "nominators": Array<[SS58String, Anonymize<If9jidduiuq7vv>]>;
    "owner_ss58": SS58String;
    "registrations": Anonymize<Icgljjb6j82uhn>;
    "validator_permits": Anonymize<Icgljjb6j82uhn>;
    "return_per_1000": bigint;
    "total_daily_return": bigint;
};
export type I97cs1i8k87lnm = (Anonymize<I4dh58q3tkaf4j>) | undefined;
export type I874e758ge6pa9 = Array<[Anonymize<I4dh58q3tkaf4j>, Anonymize<I4ojmnsk1dchql>]>;
export type I86tq0h1o8f1g5 = Array<Anonymize<I89nj65vjrv1i8>>;
export type I89nj65vjrv1i8 = {
    "hotkey": SS58String;
    "coldkey": SS58String;
    "uid": number;
    "netuid": number;
    "active": boolean;
    "axon_info": Anonymize<Ibc83gdj8hi3rc>;
    "prometheus_info": Anonymize<Iaap7oohdmr1sb>;
    "stake": Anonymize<Iba9inugg1atvo>;
    "rank": number;
    "emission": bigint;
    "incentive": number;
    "consensus": number;
    "trust": number;
    "validator_trust": number;
    "dividends": number;
    "last_update": bigint;
    "validator_permit": boolean;
    "weights": Anonymize<I95g6i7ilua7lq>;
    "bonds": Anonymize<I95g6i7ilua7lq>;
    "pruning_score": number;
};
export type Iba9inugg1atvo = Array<Anonymize<I95l2k9b1re95f>>;
export type I78cq8c9mego2f = (Anonymize<I89nj65vjrv1i8>) | undefined;
export type I64hm01ml98m4p = Array<Anonymize<If8j022vmi07bv>>;
export type If8j022vmi07bv = {
    "hotkey": SS58String;
    "coldkey": SS58String;
    "uid": number;
    "netuid": number;
    "active": boolean;
    "axon_info": Anonymize<Ibc83gdj8hi3rc>;
    "prometheus_info": Anonymize<Iaap7oohdmr1sb>;
    "stake": Anonymize<Iba9inugg1atvo>;
    "rank": number;
    "emission": bigint;
    "incentive": number;
    "consensus": number;
    "trust": number;
    "validator_trust": number;
    "dividends": number;
    "last_update": bigint;
    "validator_permit": boolean;
    "pruning_score": number;
};
export type I3gjbugrk45her = (Anonymize<If8j022vmi07bv>) | undefined;
export type I9nvi04b7jiso4 = ({
    "netuid": number;
    "rho": number;
    "kappa": number;
    "difficulty": bigint;
    "immunity_period": number;
    "max_allowed_validators": number;
    "min_allowed_weights": number;
    "max_weights_limit": number;
    "scaling_law_power": number;
    "subnetwork_n": number;
    "max_allowed_uids": number;
    "blocks_since_last_step": bigint;
    "tempo": number;
    "network_modality": number;
    "network_connect": Anonymize<I95g6i7ilua7lq>;
    "emission_values": bigint;
    "burn": bigint;
    "owner": SS58String;
}) | undefined;
export type I6s1052v0hl6mr = Array<Anonymize<I9nvi04b7jiso4>>;
export type I31p8sd8onusg0 = ({
    "netuid": number;
    "rho": number;
    "kappa": number;
    "difficulty": bigint;
    "immunity_period": number;
    "max_allowed_validators": number;
    "min_allowed_weights": number;
    "max_weights_limit": number;
    "scaling_law_power": number;
    "subnetwork_n": number;
    "max_allowed_uids": number;
    "blocks_since_last_step": bigint;
    "tempo": number;
    "network_modality": number;
    "network_connect": Anonymize<I95g6i7ilua7lq>;
    "emission_value": bigint;
    "burn": bigint;
    "owner": SS58String;
    "identity"?: Anonymize<I3m38saj8mvtpv>;
}) | undefined;
export type I2vgg418k9gfnm = Array<Anonymize<I31p8sd8onusg0>>;
export type I7dp6t7k7a8r36 = ({
    "rho": number;
    "kappa": number;
    "immunity_period": number;
    "min_allowed_weights": number;
    "max_weights_limit": number;
    "tempo": number;
    "min_difficulty": bigint;
    "max_difficulty": bigint;
    "weights_version": bigint;
    "weights_rate_limit": bigint;
    "adjustment_interval": number;
    "activity_cutoff": number;
    "registration_allowed": boolean;
    "target_regs_per_interval": number;
    "min_burn": bigint;
    "max_burn": bigint;
    "bonds_moving_avg": bigint;
    "max_regs_per_block": number;
    "serving_rate_limit": bigint;
    "max_validators": number;
    "adjustment_alpha": bigint;
    "difficulty": bigint;
    "commit_reveal_period": bigint;
    "commit_reveal_weights_enabled": boolean;
    "alpha_high": number;
    "alpha_low": number;
    "liquid_alpha_enabled": boolean;
}) | undefined;
export type Ibtpedbm9ai3hp = ({
    "rho": number;
    "kappa": number;
    "immunity_period": number;
    "min_allowed_weights": number;
    "max_weights_limit": number;
    "tempo": number;
    "min_difficulty": bigint;
    "max_difficulty": bigint;
    "weights_version": bigint;
    "weights_rate_limit": bigint;
    "adjustment_interval": number;
    "activity_cutoff": number;
    "registration_allowed": boolean;
    "target_regs_per_interval": number;
    "min_burn": bigint;
    "max_burn": bigint;
    "bonds_moving_avg": bigint;
    "max_regs_per_block": number;
    "serving_rate_limit": bigint;
    "max_validators": number;
    "adjustment_alpha": bigint;
    "difficulty": bigint;
    "commit_reveal_period": bigint;
    "commit_reveal_weights_enabled": boolean;
    "alpha_high": number;
    "alpha_low": number;
    "liquid_alpha_enabled": boolean;
    "alpha_sigmoid_steepness": bigint;
    "yuma_version": number;
    "subnet_is_active": boolean;
    "transfers_enabled": boolean;
    "bonds_reset_enabled": boolean;
    "user_liquidity_enabled": boolean;
}) | undefined;
export type I8ivaf995pho4u = Array<Anonymize<Ibjoh8vk2j7bqd>>;
export type Ibjoh8vk2j7bqd = ({
    "netuid": number;
    "owner_hotkey": SS58String;
    "owner_coldkey": SS58String;
    "subnet_name": Anonymize<Icgljjb6j82uhn>;
    "token_symbol": Anonymize<Icgljjb6j82uhn>;
    "tempo": number;
    "last_step": bigint;
    "blocks_since_last_step": bigint;
    "emission": bigint;
    "alpha_in": bigint;
    "alpha_out": bigint;
    "tao_in": bigint;
    "alpha_out_emission": bigint;
    "alpha_in_emission": bigint;
    "tao_in_emission": bigint;
    "pending_alpha_emission": bigint;
    "pending_root_emission": bigint;
    "subnet_volume": bigint;
    "network_registered_at": bigint;
    "subnet_identity"?: Anonymize<I3m38saj8mvtpv>;
    "moving_price": bigint;
}) | undefined;
export type Icr6rj04unermu = Array<Anonymize<I5gfdo8kg6rloq>>;
export type I5gfdo8kg6rloq = ({
    "netuid": number;
    "name": Anonymize<Icgljjb6j82uhn>;
    "symbol": Anonymize<Icgljjb6j82uhn>;
    "identity"?: Anonymize<I3m38saj8mvtpv>;
    "network_registered_at": bigint;
    "owner_hotkey": SS58String;
    "owner_coldkey": SS58String;
    "block": bigint;
    "tempo": number;
    "last_step": bigint;
    "blocks_since_last_step": bigint;
    "subnet_emission": bigint;
    "alpha_in": bigint;
    "alpha_out": bigint;
    "tao_in": bigint;
    "alpha_out_emission": bigint;
    "alpha_in_emission": bigint;
    "tao_in_emission": bigint;
    "pending_alpha_emission": bigint;
    "pending_root_emission": bigint;
    "subnet_volume": bigint;
    "moving_price": bigint;
    "rho": number;
    "kappa": number;
    "min_allowed_weights": number;
    "max_weights_limit": number;
    "weights_version": bigint;
    "weights_rate_limit": bigint;
    "activity_cutoff": number;
    "max_validators": number;
    "num_uids": number;
    "max_uids": number;
    "burn": bigint;
    "difficulty": bigint;
    "registration_allowed": boolean;
    "pow_registration_allowed": boolean;
    "immunity_period": number;
    "min_difficulty": bigint;
    "max_difficulty": bigint;
    "min_burn": bigint;
    "max_burn": bigint;
    "adjustment_alpha": bigint;
    "adjustment_interval": number;
    "target_regs_per_interval": number;
    "max_regs_per_block": number;
    "serving_rate_limit": bigint;
    "commit_reveal_weights_enabled": boolean;
    "commit_reveal_period": bigint;
    "liquid_alpha_enabled": boolean;
    "alpha_high": number;
    "alpha_low": number;
    "bonds_moving_avg": bigint;
    "hotkeys": Anonymize<Ia2lhg7l2hilo3>;
    "coldkeys": Anonymize<Ia2lhg7l2hilo3>;
    "identities": Anonymize<Iaf9dcc3cspgj7>;
    "axons": Anonymize<Iemjgg2q8584r9>;
    "active": Anonymize<I9eir063evtfb6>;
    "validator_permit": Anonymize<I9eir063evtfb6>;
    "pruning_score": Anonymize<Icgljjb6j82uhn>;
    "last_update": Anonymize<Iafqnechp3omqg>;
    "emission": Anonymize<Iafqnechp3omqg>;
    "dividends": Anonymize<Icgljjb6j82uhn>;
    "incentives": Anonymize<Icgljjb6j82uhn>;
    "consensus": Anonymize<Icgljjb6j82uhn>;
    "trust": Anonymize<Icgljjb6j82uhn>;
    "rank": Anonymize<Icgljjb6j82uhn>;
    "block_at_registration": Anonymize<Iafqnechp3omqg>;
    "alpha_stake": Anonymize<Iafqnechp3omqg>;
    "tao_stake": Anonymize<Iafqnechp3omqg>;
    "total_stake": Anonymize<Iafqnechp3omqg>;
    "tao_dividends_per_hotkey": Anonymize<Iba9inugg1atvo>;
    "alpha_dividends_per_hotkey": Anonymize<Iba9inugg1atvo>;
}) | undefined;
export type Iaf9dcc3cspgj7 = Array<(Anonymize<Ifjlj958aeheic>) | undefined>;
export type Iemjgg2q8584r9 = Array<Anonymize<Ibc83gdj8hi3rc>>;
export type I2u4s5o1c0r3fu = ({
    "netuid": number;
    "hotkeys": Anonymize<Ia2lhg7l2hilo3>;
    "coldkeys": Anonymize<Ia2lhg7l2hilo3>;
    "active": Anonymize<I9eir063evtfb6>;
    "validator_permit": Anonymize<I9eir063evtfb6>;
    "pruning_score": Anonymize<Icgljjb6j82uhn>;
    "last_update": Anonymize<Iafqnechp3omqg>;
    "emission": Anonymize<Iafqnechp3omqg>;
    "dividends": Anonymize<Icgljjb6j82uhn>;
    "incentives": Anonymize<Icgljjb6j82uhn>;
    "consensus": Anonymize<Icgljjb6j82uhn>;
    "trust": Anonymize<Icgljjb6j82uhn>;
    "rank": Anonymize<Icgljjb6j82uhn>;
    "block_at_registration": Anonymize<Iafqnechp3omqg>;
    "alpha_stake": Anonymize<Iafqnechp3omqg>;
    "tao_stake": Anonymize<Iafqnechp3omqg>;
    "total_stake": Anonymize<Iafqnechp3omqg>;
    "emission_history": Array<Anonymize<Iafqnechp3omqg>>;
}) | undefined;
export type Ic0g2vnp5r296p = ({
    "netuid": number;
    "name"?: Anonymize<I35lk2003i8c8g>;
    "symbol"?: Anonymize<I35lk2003i8c8g>;
    "identity"?: (Anonymize<I3m38saj8mvtpv>) | undefined;
    "network_registered_at"?: Anonymize<I35p85j063s0il>;
    "owner_hotkey"?: Anonymize<Ihfphjolmsqq1>;
    "owner_coldkey"?: Anonymize<Ihfphjolmsqq1>;
    "block"?: Anonymize<I35p85j063s0il>;
    "tempo"?: Anonymize<I4arjljr6dpflb>;
    "last_step"?: Anonymize<I35p85j063s0il>;
    "blocks_since_last_step"?: Anonymize<I35p85j063s0il>;
    "subnet_emission"?: Anonymize<I35p85j063s0il>;
    "alpha_in"?: Anonymize<I35p85j063s0il>;
    "alpha_out"?: Anonymize<I35p85j063s0il>;
    "tao_in"?: Anonymize<I35p85j063s0il>;
    "alpha_out_emission"?: Anonymize<I35p85j063s0il>;
    "alpha_in_emission"?: Anonymize<I35p85j063s0il>;
    "tao_in_emission"?: Anonymize<I35p85j063s0il>;
    "pending_alpha_emission"?: Anonymize<I35p85j063s0il>;
    "pending_root_emission"?: Anonymize<I35p85j063s0il>;
    "subnet_volume"?: Anonymize<I35p85j063s0il>;
    "moving_price"?: Anonymize<I35p85j063s0il>;
    "rho"?: Anonymize<I4arjljr6dpflb>;
    "kappa"?: Anonymize<I4arjljr6dpflb>;
    "min_allowed_weights"?: Anonymize<I4arjljr6dpflb>;
    "max_weights_limit"?: Anonymize<I4arjljr6dpflb>;
    "weights_version"?: Anonymize<I35p85j063s0il>;
    "weights_rate_limit"?: Anonymize<I35p85j063s0il>;
    "activity_cutoff"?: Anonymize<I4arjljr6dpflb>;
    "max_validators"?: Anonymize<I4arjljr6dpflb>;
    "num_uids"?: Anonymize<I4arjljr6dpflb>;
    "max_uids"?: Anonymize<I4arjljr6dpflb>;
    "burn"?: Anonymize<I35p85j063s0il>;
    "difficulty"?: Anonymize<I35p85j063s0il>;
    "registration_allowed"?: (boolean) | undefined;
    "pow_registration_allowed"?: (boolean) | undefined;
    "immunity_period"?: Anonymize<I4arjljr6dpflb>;
    "min_difficulty"?: Anonymize<I35p85j063s0il>;
    "max_difficulty"?: Anonymize<I35p85j063s0il>;
    "min_burn"?: Anonymize<I35p85j063s0il>;
    "max_burn"?: Anonymize<I35p85j063s0il>;
    "adjustment_alpha"?: Anonymize<I35p85j063s0il>;
    "adjustment_interval"?: Anonymize<I4arjljr6dpflb>;
    "target_regs_per_interval"?: Anonymize<I4arjljr6dpflb>;
    "max_regs_per_block"?: Anonymize<I4arjljr6dpflb>;
    "serving_rate_limit"?: Anonymize<I35p85j063s0il>;
    "commit_reveal_weights_enabled"?: (boolean) | undefined;
    "commit_reveal_period"?: Anonymize<I35p85j063s0il>;
    "liquid_alpha_enabled"?: (boolean) | undefined;
    "alpha_high"?: Anonymize<I4arjljr6dpflb>;
    "alpha_low"?: Anonymize<I4arjljr6dpflb>;
    "bonds_moving_avg"?: Anonymize<I35p85j063s0il>;
    "hotkeys"?: (Anonymize<Ia2lhg7l2hilo3>) | undefined;
    "coldkeys"?: (Anonymize<Ia2lhg7l2hilo3>) | undefined;
    "identities"?: (Anonymize<Iaf9dcc3cspgj7>) | undefined;
    "axons"?: (Anonymize<Iemjgg2q8584r9>) | undefined;
    "active"?: (Anonymize<I9eir063evtfb6>) | undefined;
    "validator_permit"?: (Anonymize<I9eir063evtfb6>) | undefined;
    "pruning_score"?: Anonymize<I35lk2003i8c8g>;
    "last_update"?: (Anonymize<Iafqnechp3omqg>) | undefined;
    "emission"?: (Anonymize<Iafqnechp3omqg>) | undefined;
    "dividends"?: Anonymize<I35lk2003i8c8g>;
    "incentives"?: Anonymize<I35lk2003i8c8g>;
    "consensus"?: Anonymize<I35lk2003i8c8g>;
    "trust"?: Anonymize<I35lk2003i8c8g>;
    "rank"?: Anonymize<I35lk2003i8c8g>;
    "block_at_registration"?: (Anonymize<Iafqnechp3omqg>) | undefined;
    "alpha_stake"?: (Anonymize<Iafqnechp3omqg>) | undefined;
    "tao_stake"?: (Anonymize<Iafqnechp3omqg>) | undefined;
    "total_stake"?: (Anonymize<Iafqnechp3omqg>) | undefined;
    "tao_dividends_per_hotkey"?: (Anonymize<Iba9inugg1atvo>) | undefined;
    "alpha_dividends_per_hotkey"?: (Anonymize<Iba9inugg1atvo>) | undefined;
    "validators"?: Anonymize<I35lk2003i8c8g>;
    "commitments"?: (Array<[SS58String, Anonymize<Icgljjb6j82uhn>]>) | undefined;
}) | undefined;
export type Ic9fkrj2ggjleq = Array<Anonymize<I66h6oadnuebe>>;
export type I66h6oadnuebe = {
    "hotkey": SS58String;
    "coldkey": SS58String;
    "netuid": number;
    "stake": bigint;
    "locked": bigint;
    "emission": bigint;
    "tao_emission": bigint;
    "drain": bigint;
    "is_registered": boolean;
};
export type Ifi9cmevnosufh = Array<[SS58String, Anonymize<Ic9fkrj2ggjleq>]>;
export type I1i5jfmqcsjper = (Anonymize<I66h6oadnuebe>) | undefined;
export type I3pbrjdm4vnbsa = (Anonymize<I6ouflveob4eli>) | undefined;
export type Iems84l8lk2v0c = {
    "slot_duration": bigint;
    "epoch_length": bigint;
    "c": Anonymize<I200n1ov5tbcvr>;
    "authorities": Anonymize<I3geksg000c171>;
    "randomness": FixedSizeBinary<32>;
    "allowed_slots": BabeAllowedSlots;
};
export type I200n1ov5tbcvr = FixedSizeArray<2, bigint>;
export type BabeAllowedSlots = Enum<{
    "PrimarySlots": undefined;
    "PrimaryAndSecondaryPlainSlots": undefined;
    "PrimaryAndSecondaryVRFSlots": undefined;
}>;
export declare const BabeAllowedSlots: GetEnum<BabeAllowedSlots>;
export type I1r5ke30ueqo0r = {
    "epoch_index": bigint;
    "start_slot": bigint;
    "duration": bigint;
    "authorities": Anonymize<I3geksg000c171>;
    "randomness": FixedSizeBinary<32>;
    "config": {
        "c": Anonymize<I200n1ov5tbcvr>;
        "allowed_slots": BabeAllowedSlots;
    };
};
export type I68ii5ik8avr9o = {
    "offender": FixedSizeBinary<32>;
    "slot": bigint;
    "first_header": Anonymize<Ic952bubvq4k7d>;
    "second_header": Anonymize<Ic952bubvq4k7d>;
};
export type I8slfm2rri67ri = Array<{
    "netuid": number;
    "price": bigint;
}>;
export type I34n2itmpoq7on = {
    "tao_amount": bigint;
    "alpha_amount": bigint;
    "tao_fee": bigint;
    "alpha_fee": bigint;
    "tao_slippage": bigint;
    "alpha_slippage": bigint;
};
export {};
