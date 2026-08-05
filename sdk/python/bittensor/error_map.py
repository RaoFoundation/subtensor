"""Semantic error taxonomy and the complete chain-error classification.

Every error name in the generated catalog (bittensor/_generated/errors.py) is
deliberately assigned a semantic :class:`ErrorCode` here, so a failure always
carries an actionable code instead of degrading to ``unknown``. The
``python -m codegen.check --names`` gate enforces both directions: every name
mapped here must still exist on chain, and every name on chain must classify.

``ErrorCode`` lives in this module (rather than ``result.py``, its public home)
only to avoid a circular import; import it from ``bittensor.result``.

Names are keyed bare (without the pallet) because that is what the receipt
decoder yields. Names that appear in several pallets are listed once, in the
first pallet's section, with a comment noting the collision — every collision
maps to the same code, which the layout below makes reviewable.
"""

from __future__ import annotations

from enum import Enum


class ErrorCode(str, Enum):
    INSUFFICIENT_BALANCE = "insufficient_balance"
    INSUFFICIENT_LIQUIDITY = "insufficient_liquidity"
    RATE_LIMITED = "rate_limited"
    NOT_REGISTERED = "not_registered"
    NOT_AUTHORIZED = "not_authorized"
    ALREADY_EXISTS = "already_exists"
    NOT_FOUND = "not_found"
    SUBNET_NOT_EXISTS = "subnet_not_exists"
    SUBTOKEN_DISABLED = "subtoken_disabled"
    DISABLED = "disabled"
    TOO_EARLY = "too_early"
    EXPIRED = "expired"
    LIMIT_EXCEEDED = "limit_exceeded"
    UNIT_MISMATCH = "unit_mismatch"
    INVALID_ARGUMENT = "invalid_argument"
    POLICY_VIOLATION = "policy_violation"
    INTERNAL = "internal"
    UNKNOWN = "unknown"


_C = ErrorCode

NAME_TO_CODE: dict[str, ErrorCode] = {
    # ── System ──────────────────────────────────────────────────────────
    "InvalidSpecName": _C.INVALID_ARGUMENT,
    "SpecVersionNeedsToIncrease": _C.INVALID_ARGUMENT,
    "FailedToExtractRuntimeVersion": _C.INVALID_ARGUMENT,
    "NonDefaultComposite": _C.INVALID_ARGUMENT,
    "NonZeroRefCount": _C.INVALID_ARGUMENT,
    "CallFiltered": _C.NOT_AUTHORIZED,
    "MultiBlockMigrationsOngoing": _C.TOO_EARLY,
    "NothingAuthorized": _C.NOT_FOUND,
    "Unauthorized": _C.NOT_AUTHORIZED,  # also LimitOrders
    # ── Grandpa ─────────────────────────────────────────────────────────
    "PauseFailed": _C.INVALID_ARGUMENT,
    "ResumeFailed": _C.INVALID_ARGUMENT,
    "ChangePending": _C.ALREADY_EXISTS,
    "TooSoon": _C.RATE_LIMITED,
    "InvalidKeyOwnershipProof": _C.INVALID_ARGUMENT,
    "InvalidEquivocationProof": _C.INVALID_ARGUMENT,
    "DuplicateOffenceReport": _C.ALREADY_EXISTS,
    # ── Balances ────────────────────────────────────────────────────────
    "VestingBalance": _C.INSUFFICIENT_BALANCE,
    "LiquidityRestrictions": _C.INSUFFICIENT_BALANCE,
    "InsufficientBalance": _C.INSUFFICIENT_BALANCE,  # also Crowdloan, Swap
    "ExistentialDeposit": _C.INSUFFICIENT_BALANCE,
    "Expendability": _C.INSUFFICIENT_BALANCE,
    "ExistingVestingSchedule": _C.ALREADY_EXISTS,
    "DeadAccount": _C.NOT_FOUND,
    "TooManyReserves": _C.LIMIT_EXCEEDED,
    "TooManyHolds": _C.LIMIT_EXCEEDED,
    "TooManyFreezes": _C.LIMIT_EXCEEDED,
    "IssuanceDeactivated": _C.DISABLED,
    "DeltaZero": _C.INVALID_ARGUMENT,
    # ── SubtensorModule ─────────────────────────────────────────────────
    "RootNetworkDoesNotExist": _C.SUBNET_NOT_EXISTS,
    "InvalidIpType": _C.INVALID_ARGUMENT,
    "InvalidIpAddress": _C.INVALID_ARGUMENT,
    "InvalidPort": _C.INVALID_ARGUMENT,
    "HotKeyNotRegisteredInSubNet": _C.NOT_REGISTERED,
    "HotKeyAccountNotExists": _C.NOT_REGISTERED,
    "HotKeyNotRegisteredInNetwork": _C.NOT_REGISTERED,
    "NonAssociatedColdKey": _C.NOT_AUTHORIZED,
    "NotEnoughStake": _C.INSUFFICIENT_BALANCE,
    "NotEnoughStakeToWithdraw": _C.INSUFFICIENT_BALANCE,
    # Hotkey stake-weight floor for set/commit weights (not free-TAO balance).
    # Kept under insufficient_balance; remediation is overridden by name.
    "NotEnoughStakeToSetWeights": _C.INSUFFICIENT_BALANCE,
    "NotEnoughStakeToSetChildkeys": _C.INSUFFICIENT_BALANCE,
    "NotEnoughBalanceToStake": _C.INSUFFICIENT_BALANCE,
    "BalanceWithdrawalError": _C.INSUFFICIENT_BALANCE,
    "ZeroBalanceAfterWithdrawn": _C.INSUFFICIENT_BALANCE,
    # Setting non-self weights without a validator permit: acquire one (enough
    # stake weight to be in the subnet's top-K) or wait for the next epoch's
    # permit recalculation — re-signing with another key does not help.
    "NeuronNoValidatorPermit": _C.NOT_AUTHORIZED,
    "WeightVecNotEqualSize": _C.INVALID_ARGUMENT,
    "DuplicateUids": _C.INVALID_ARGUMENT,
    "UidVecContainInvalidOne": _C.INVALID_ARGUMENT,
    "WeightVecLengthIsLow": _C.INVALID_ARGUMENT,
    "TooManyRegistrationsThisBlock": _C.RATE_LIMITED,
    "HotKeyAlreadyRegisteredInSubNet": _C.ALREADY_EXISTS,
    "NewHotKeyIsSameWithOld": _C.INVALID_ARGUMENT,
    "NewHotKeyNotCleanForRootSwap": _C.INVALID_ARGUMENT,
    "InvalidWorkBlock": _C.INVALID_ARGUMENT,
    "InvalidDifficulty": _C.INVALID_ARGUMENT,
    "InvalidSeal": _C.INVALID_ARGUMENT,
    "MaxWeightExceeded": _C.LIMIT_EXCEEDED,
    "HotKeyAlreadyDelegate": _C.ALREADY_EXISTS,
    "SettingWeightsTooFast": _C.RATE_LIMITED,
    "IncorrectWeightVersionKey": _C.INVALID_ARGUMENT,
    "ServingRateLimitExceeded": _C.RATE_LIMITED,
    "UidsLengthExceedUidsInSubNet": _C.LIMIT_EXCEEDED,
    "NetworkTxRateLimitExceeded": _C.RATE_LIMITED,
    "DelegateTxRateLimitExceeded": _C.RATE_LIMITED,
    "HotKeySetTxRateLimitExceeded": _C.RATE_LIMITED,
    "StakingRateLimitExceeded": _C.RATE_LIMITED,
    "SubNetRegistrationDisabled": _C.DISABLED,
    "TooManyRegistrationsThisInterval": _C.RATE_LIMITED,
    "TransactorAccountShouldBeHotKey": _C.NOT_AUTHORIZED,
    "FaucetDisabled": _C.DISABLED,
    "NotSubnetOwner": _C.NOT_AUTHORIZED,
    "RegistrationNotPermittedOnRootSubnet": _C.INVALID_ARGUMENT,
    "StakeTooLowForRoot": _C.INSUFFICIENT_BALANCE,
    "AllNetworksInImmunity": _C.TOO_EARLY,
    "NotEnoughBalanceToPaySwapHotKey": _C.INSUFFICIENT_BALANCE,
    "NotRootSubnet": _C.INVALID_ARGUMENT,
    "CanNotSetRootNetworkWeights": _C.INVALID_ARGUMENT,
    "NoNeuronIdAvailable": _C.LIMIT_EXCEEDED,
    "DelegateTakeTooLow": _C.INVALID_ARGUMENT,
    "DelegateTakeTooHigh": _C.INVALID_ARGUMENT,
    "NoWeightsCommitFound": _C.NOT_FOUND,
    "InvalidRevealCommitHashNotMatch": _C.INVALID_ARGUMENT,
    # Plain set_weights was called while commit-reveal is ON for the subnet
    # (subnets/weights.rs): the direct path is disabled there — use the
    # commit/reveal flow instead (the SDK's set-weights auto-selects it).
    "CommitRevealEnabled": _C.DISABLED,
    "CommitRevealDisabled": _C.DISABLED,
    "LiquidAlphaDisabled": _C.DISABLED,
    "AlphaHighTooLow": _C.INVALID_ARGUMENT,
    "AlphaLowOutOfRange": _C.INVALID_ARGUMENT,
    "ColdKeyAlreadyAssociated": _C.ALREADY_EXISTS,
    "NotEnoughBalanceToPaySwapColdKey": _C.INSUFFICIENT_BALANCE,
    "InvalidChild": _C.INVALID_ARGUMENT,
    "DuplicateChild": _C.INVALID_ARGUMENT,
    "ProportionOverflow": _C.INVALID_ARGUMENT,
    "TooManyChildren": _C.LIMIT_EXCEEDED,
    "TxRateLimitExceeded": _C.RATE_LIMITED,
    "ColdkeySwapAnnouncementNotFound": _C.NOT_FOUND,
    "ColdkeySwapTooEarly": _C.TOO_EARLY,
    "ColdkeySwapReannouncedTooEarly": _C.TOO_EARLY,
    "AnnouncedColdkeyHashDoesNotMatch": _C.INVALID_ARGUMENT,
    "ColdkeySwapAlreadyDisputed": _C.ALREADY_EXISTS,
    "NewColdKeyIsHotkey": _C.INVALID_ARGUMENT,
    "InvalidChildkeyTake": _C.INVALID_ARGUMENT,
    "TxChildkeyTakeRateLimitExceeded": _C.RATE_LIMITED,
    "InvalidIdentity": _C.INVALID_ARGUMENT,
    "MechanismDoesNotExist": _C.SUBNET_NOT_EXISTS,  # also Swap
    "StakeUnavailable": _C.INSUFFICIENT_BALANCE,
    "SubnetNotExists": _C.SUBNET_NOT_EXISTS,
    "TooManyUnrevealedCommits": _C.LIMIT_EXCEEDED,
    "ExpiredWeightCommit": _C.EXPIRED,
    "RevealTooEarly": _C.TOO_EARLY,
    "InputLengthsUnequal": _C.INVALID_ARGUMENT,
    "CommittingWeightsTooFast": _C.RATE_LIMITED,
    "AmountTooLow": _C.LIMIT_EXCEEDED,
    "InsufficientLiquidity": _C.INSUFFICIENT_LIQUIDITY,  # also Swap
    "SlippageTooHigh": _C.INSUFFICIENT_LIQUIDITY,
    "TransferDisallowed": _C.DISABLED,
    "ActivityCutoffTooLow": _C.INVALID_ARGUMENT,
    "CallDisabled": _C.DISABLED,
    "FirstEmissionBlockNumberAlreadySet": _C.ALREADY_EXISTS,
    "NeedWaitingMoreBlocksToStarCall": _C.TOO_EARLY,
    "StartCallNotReady": _C.TOO_EARLY,
    "WaitingForDissolvedSubnetCleanup": _C.TOO_EARLY,
    # Lock id space is one-per-subnet-registration; overflow means the coldkey
    # holds the maximum number of registered subnets, not an arithmetic bug.
    "LockIdOverFlow": _C.LIMIT_EXCEEDED,
    "NotEnoughAlphaOutToRecycle": _C.INSUFFICIENT_LIQUIDITY,
    "CannotBurnOrRecycleOnRootSubnet": _C.INVALID_ARGUMENT,
    "UnableToRecoverPublicKey": _C.INVALID_ARGUMENT,
    "InvalidRecoveredPublicKey": _C.INVALID_ARGUMENT,
    "SubtokenDisabled": _C.SUBTOKEN_DISABLED,  # also Swap
    "HotKeySwapOnSubnetIntervalNotPassed": _C.RATE_LIMITED,
    "KeepStakeBlockedByCollateral": _C.POLICY_VIOLATION,
    "SameNetuid": _C.INVALID_ARGUMENT,
    "InsufficientTaoBalance": _C.INSUFFICIENT_BALANCE,
    "InsufficientAlphaBalance": _C.INSUFFICIENT_BALANCE,
    "InvalidLeaseBeneficiary": _C.INVALID_ARGUMENT,
    "LeaseCannotEndInThePast": _C.INVALID_ARGUMENT,
    "LeaseNetuidNotFound": _C.NOT_FOUND,
    "LeaseDoesNotExist": _C.NOT_FOUND,
    "LeaseHasNoEndBlock": _C.INVALID_ARGUMENT,
    "LeaseHasNotEnded": _C.TOO_EARLY,
    "Overflow": _C.INTERNAL,  # also Crowdloan
    "BeneficiaryDoesNotOwnHotkey": _C.NOT_AUTHORIZED,
    "ExpectedBeneficiaryOrigin": _C.NOT_AUTHORIZED,
    "AdminActionProhibitedDuringWeightsWindow": _C.TOO_EARLY,
    "SymbolDoesNotExist": _C.NOT_FOUND,
    "SymbolAlreadyInUse": _C.ALREADY_EXISTS,
    "IncorrectCommitRevealVersion": _C.INVALID_ARGUMENT,
    "InvalidRevealRound": _C.EXPIRED,
    "RevealPeriodTooLarge": _C.INVALID_ARGUMENT,
    "RevealPeriodTooSmall": _C.INVALID_ARGUMENT,
    "InvalidValue": _C.INVALID_ARGUMENT,  # also AdminUtils
    "SubnetLimitReached": _C.LIMIT_EXCEEDED,
    "CannotAffordLockCost": _C.INSUFFICIENT_BALANCE,
    "EvmKeyAssociateRateLimitExceeded": _C.RATE_LIMITED,
    "EvmKeyAssociationLimitExceeded": _C.LIMIT_EXCEEDED,
    "SameAutoStakeHotkeyAlreadySet": _C.ALREADY_EXISTS,
    "UidMapCouldNotBeCleared": _C.INTERNAL,
    "TrimmingWouldExceedMaxImmunePercentage": _C.LIMIT_EXCEEDED,
    "ChildParentInconsistency": _C.INVALID_ARGUMENT,
    "InvalidRootClaimThreshold": _C.INVALID_ARGUMENT,
    "BasketHasNoWeights": _C.INVALID_ARGUMENT,
    "BetaBasketSeedInProgress": _C.TOO_EARLY,
    "RootWeightSettingDisabled": _C.DISABLED,
    "RootStakeLocked": _C.TOO_EARLY,
    "TooManyUIDsPerMechanism": _C.LIMIT_EXCEEDED,
    "VotingPowerTrackingNotEnabled": _C.DISABLED,
    "InvalidVotingPowerEmaAlpha": _C.INVALID_ARGUMENT,
    "Deprecated": _C.DISABLED,  # also AdminUtils, Swap
    "SubnetBuybackRateLimitExceeded": _C.RATE_LIMITED,
    "NetworkDissolveAlreadyQueued": _C.ALREADY_EXISTS,
    "AddStakeBurnRateLimitExceeded": _C.RATE_LIMITED,
    "ColdkeyCollateralIncomplete": _C.INTERNAL,
    "ColdkeyCollateralPositionsFull": _C.LIMIT_EXCEEDED,
    # Pending announcement blocks nearly all signed calls until the swap is
    # executed or the announcement is cleared (guards/check_coldkey_swap.rs).
    # Bucketed as disabled (feature/path blocked), not already_exists.
    "ColdkeySwapAnnounced": _C.DISABLED,
    # The account is frozen because its pending coldkey swap is disputed; all
    # signed calls are blocked until root resolves it via reset_coldkey_swap
    # (guards/check_coldkey_swap.rs). Waiting does not help, so not too_early.
    "ColdkeySwapDisputed": _C.DISABLED,
    "ColdkeySwapClearTooEarly": _C.TOO_EARLY,
    "DisabledTemporarily": _C.DISABLED,
    "RegistrationPriceLimitExceeded": _C.LIMIT_EXCEEDED,
    "LockHotkeyMismatch": _C.INVALID_ARGUMENT,
    "InsufficientStakeForLock": _C.INSUFFICIENT_BALANCE,
    "NoExistingLock": _C.NOT_FOUND,
    "ActiveLockExists": _C.ALREADY_EXISTS,
    "CannotUseSystemAccount": _C.NOT_AUTHORIZED,
    "UnlockAmountTooHigh": _C.INSUFFICIENT_BALANCE,
    "TempoOutOfBounds": _C.INVALID_ARGUMENT,
    "ActivityCutoffFactorMilliOutOfBounds": _C.INVALID_ARGUMENT,
    "EpochTriggerAlreadyPending": _C.ALREADY_EXISTS,
    "AutoEpochAlreadyImminent": _C.ALREADY_EXISTS,
    "DynamicTempoBlockedByCommitReveal": _C.DISABLED,
    "AccountRejectsLockedAlpha": _C.POLICY_VIOLATION,
    # ── Utility ─────────────────────────────────────────────────────────
    "TooManyCalls": _C.LIMIT_EXCEEDED,
    "InvalidDerivedAccount": _C.INVALID_ARGUMENT,
    # ── Sudo ────────────────────────────────────────────────────────────
    "RequireSudo": _C.NOT_AUTHORIZED,
    # ── Multisig ────────────────────────────────────────────────────────
    "MinimumThreshold": _C.INVALID_ARGUMENT,
    "AlreadyApproved": _C.ALREADY_EXISTS,
    "NoApprovalsNeeded": _C.INVALID_ARGUMENT,
    "TooFewSignatories": _C.INVALID_ARGUMENT,
    "TooManySignatories": _C.LIMIT_EXCEEDED,
    "SignatoriesOutOfOrder": _C.INVALID_ARGUMENT,
    "SenderInSignatories": _C.INVALID_ARGUMENT,
    "NotFound": _C.NOT_FOUND,  # also Scheduler, Proxy
    "NotOwner": _C.NOT_AUTHORIZED,
    "NoTimepoint": _C.INVALID_ARGUMENT,
    "WrongTimepoint": _C.INVALID_ARGUMENT,
    "UnexpectedTimepoint": _C.INVALID_ARGUMENT,
    "MaxWeightTooLow": _C.INVALID_ARGUMENT,
    "AlreadyStored": _C.ALREADY_EXISTS,
    # ── Preimage ────────────────────────────────────────────────────────
    "TooBig": _C.LIMIT_EXCEEDED,
    "AlreadyNoted": _C.ALREADY_EXISTS,
    "NotAuthorized": _C.NOT_AUTHORIZED,
    "NotNoted": _C.NOT_FOUND,
    "Requested": _C.INVALID_ARGUMENT,
    "NotRequested": _C.NOT_FOUND,
    "TooMany": _C.LIMIT_EXCEEDED,  # also Proxy
    "TooFew": _C.INVALID_ARGUMENT,
    # ── Scheduler ───────────────────────────────────────────────────────
    "FailedToSchedule": _C.INTERNAL,
    "TargetBlockNumberInPast": _C.INVALID_ARGUMENT,
    "RescheduleNoChange": _C.INVALID_ARGUMENT,
    "Named": _C.INVALID_ARGUMENT,
    # ── Proxy ───────────────────────────────────────────────────────────
    "NotProxy": _C.NOT_AUTHORIZED,
    "Unproxyable": _C.NOT_AUTHORIZED,
    "Duplicate": _C.ALREADY_EXISTS,
    "NoPermission": _C.NOT_AUTHORIZED,
    "Unannounced": _C.TOO_EARLY,
    "NoSelfProxy": _C.INVALID_ARGUMENT,
    "AnnouncementDepositInvariantViolated": _C.INTERNAL,
    "InvalidDerivedAccountId": _C.INVALID_ARGUMENT,
    # ── Commitments ─────────────────────────────────────────────────────
    "TooManyFieldsInCommitmentInfo": _C.LIMIT_EXCEEDED,
    "AccountNotAllowedCommit": _C.NOT_AUTHORIZED,
    "SpaceLimitExceeded": _C.LIMIT_EXCEEDED,
    "UnexpectedUnreserveLeftover": _C.INTERNAL,
    # ── AdminUtils ──────────────────────────────────────────────────────
    "SubnetDoesNotExist": _C.SUBNET_NOT_EXISTS,
    "MaxValidatorsLargerThanMaxUIds": _C.INVALID_ARGUMENT,
    "MaxAllowedUIdsLessThanCurrentUIds": _C.INVALID_ARGUMENT,
    "BondsMovingAverageMaxReached": _C.LIMIT_EXCEEDED,
    # A negative steepness value is an invalid argument for non-root callers
    # (admin-utils/src/lib.rs); use a non-negative value or a root origin.
    "NegativeSigmoidSteepness": _C.INVALID_ARGUMENT,
    "ValueNotInBounds": _C.INVALID_ARGUMENT,
    "MinAllowedUidsGreaterThanCurrentUids": _C.INVALID_ARGUMENT,
    "MinAllowedUidsGreaterThanMaxAllowedUids": _C.INVALID_ARGUMENT,
    "MaxAllowedUidsLessThanMinAllowedUids": _C.INVALID_ARGUMENT,
    "MaxAllowedUidsGreaterThanDefaultMaxAllowedUids": _C.INVALID_ARGUMENT,
    "NotPermittedOnRootSubnet": _C.INVALID_ARGUMENT,
    "POWRegistrationDisabled": _C.DISABLED,
    "CollateralLockShareTooHigh": _C.INVALID_ARGUMENT,
    "CollateralDrainRatioOutOfBounds": _C.INVALID_ARGUMENT,
    # ── SafeMode ────────────────────────────────────────────────────────
    "Entered": _C.ALREADY_EXISTS,
    "Exited": _C.ALREADY_EXISTS,
    "NotConfigured": _C.DISABLED,
    "NoDeposit": _C.NOT_FOUND,
    "AlreadyDeposited": _C.ALREADY_EXISTS,
    "CannotReleaseYet": _C.TOO_EARLY,
    "CurrencyError": _C.INTERNAL,
    # ── Ethereum ────────────────────────────────────────────────────────
    "InvalidSignature": _C.INVALID_ARGUMENT,  # also EVM, LimitOrders
    "PreLogExists": _C.INVALID_ARGUMENT,
    # ── EVM ─────────────────────────────────────────────────────────────
    "BalanceLow": _C.INSUFFICIENT_BALANCE,
    "FeeOverflow": _C.INTERNAL,
    "PaymentOverflow": _C.INTERNAL,
    "WithdrawFailed": _C.INSUFFICIENT_BALANCE,
    "GasPriceTooLow": _C.INVALID_ARGUMENT,
    "InvalidNonce": _C.INVALID_ARGUMENT,
    "GasLimitTooLow": _C.INVALID_ARGUMENT,
    "GasLimitTooHigh": _C.INVALID_ARGUMENT,
    "InvalidChainId": _C.INVALID_ARGUMENT,
    "Reentrancy": _C.INTERNAL,
    "TransactionMustComeFromEOA": _C.NOT_AUTHORIZED,
    "Undefined": _C.INTERNAL,
    "NotAllowed": _C.NOT_AUTHORIZED,
    "CreateOriginNotAllowed": _C.NOT_AUTHORIZED,
    # ── Drand ───────────────────────────────────────────────────────────
    "NoneValue": _C.NOT_FOUND,
    "StorageOverflow": _C.INTERNAL,
    "DrandConnectionFailure": _C.INTERNAL,
    "UnverifiedPulse": _C.INVALID_ARGUMENT,
    "InvalidRoundNumber": _C.INVALID_ARGUMENT,
    "PulseVerificationError": _C.INVALID_ARGUMENT,
    # ── Crowdloan ───────────────────────────────────────────────────────
    "DepositTooLow": _C.INVALID_ARGUMENT,
    "CapTooLow": _C.INVALID_ARGUMENT,
    "MinimumContributionTooLow": _C.INVALID_ARGUMENT,
    "CannotEndInPast": _C.INVALID_ARGUMENT,
    "BlockDurationTooShort": _C.INVALID_ARGUMENT,
    "BlockDurationTooLong": _C.INVALID_ARGUMENT,
    "InvalidCrowdloanId": _C.NOT_FOUND,
    "CapRaised": _C.LIMIT_EXCEEDED,
    "ContributionPeriodEnded": _C.EXPIRED,
    "ContributionTooLow": _C.INVALID_ARGUMENT,
    "InvalidOrigin": _C.NOT_AUTHORIZED,
    "AlreadyFinalized": _C.ALREADY_EXISTS,
    "AlreadyFinalizing": _C.ALREADY_EXISTS,
    "ContributionPeriodNotEnded": _C.TOO_EARLY,
    "NoContribution": _C.NOT_FOUND,
    "CapNotRaised": _C.TOO_EARLY,
    "Underflow": _C.INTERNAL,
    "CallUnavailable": _C.NOT_FOUND,
    "NotReadyToDissolve": _C.TOO_EARLY,
    "DepositCannotBeWithdrawn": _C.INVALID_ARGUMENT,
    "MaxContributorsReached": _C.LIMIT_EXCEEDED,
    "InvalidFinalizationConfig": _C.INVALID_ARGUMENT,
    "MaxContributionReached": _C.LIMIT_EXCEEDED,
    "MaximumContributionTooLow": _C.INVALID_ARGUMENT,
    "MinimumContributionTooHigh": _C.INVALID_ARGUMENT,
    # ── Swap ────────────────────────────────────────────────────────────
    "FeeRateTooHigh": _C.INVALID_ARGUMENT,
    "InsufficientInputAmount": _C.INVALID_ARGUMENT,
    "PriceLimitExceeded": _C.INSUFFICIENT_LIQUIDITY,
    "InvalidTickRange": _C.INVALID_ARGUMENT,
    "InvalidLiquidityValue": _C.INVALID_ARGUMENT,
    "ReservesTooLow": _C.INSUFFICIENT_LIQUIDITY,
    "ReservesOutOfBalance": _C.INSUFFICIENT_LIQUIDITY,
    "SwapInputTooLarge": _C.INSUFFICIENT_LIQUIDITY,
    # ── Contracts ───────────────────────────────────────────────────────
    "InvalidSchedule": _C.INVALID_ARGUMENT,
    "InvalidCallFlags": _C.INVALID_ARGUMENT,
    "OutOfGas": _C.LIMIT_EXCEEDED,
    "OutputBufferTooSmall": _C.INVALID_ARGUMENT,
    "TransferFailed": _C.INSUFFICIENT_BALANCE,
    "MaxCallDepthReached": _C.LIMIT_EXCEEDED,
    "ContractNotFound": _C.NOT_FOUND,
    "CodeTooLarge": _C.LIMIT_EXCEEDED,
    "CodeNotFound": _C.NOT_FOUND,
    "CodeInfoNotFound": _C.NOT_FOUND,
    "OutOfBounds": _C.INVALID_ARGUMENT,
    "DecodingFailed": _C.INVALID_ARGUMENT,
    "ContractTrapped": _C.INTERNAL,
    "ValueTooLarge": _C.LIMIT_EXCEEDED,
    "TerminatedWhileReentrant": _C.INVALID_ARGUMENT,
    "InputForwarded": _C.INVALID_ARGUMENT,
    "RandomSubjectTooLong": _C.LIMIT_EXCEEDED,
    "TooManyTopics": _C.LIMIT_EXCEEDED,
    "NoChainExtension": _C.DISABLED,
    "XCMDecodeFailed": _C.INVALID_ARGUMENT,
    "DuplicateContract": _C.ALREADY_EXISTS,
    "TerminatedInConstructor": _C.INTERNAL,
    "ReentranceDenied": _C.INVALID_ARGUMENT,
    "StateChangeDenied": _C.NOT_AUTHORIZED,
    "StorageDepositNotEnoughFunds": _C.INSUFFICIENT_BALANCE,
    "StorageDepositLimitExhausted": _C.LIMIT_EXCEEDED,
    "CodeInUse": _C.INVALID_ARGUMENT,
    "ContractReverted": _C.INTERNAL,
    "CodeRejected": _C.INVALID_ARGUMENT,
    "Indeterministic": _C.INVALID_ARGUMENT,
    "MigrationInProgress": _C.TOO_EARLY,
    "NoMigrationPerformed": _C.INVALID_ARGUMENT,
    "MaxDelegateDependenciesReached": _C.LIMIT_EXCEEDED,
    "DelegateDependencyNotFound": _C.NOT_FOUND,
    "DelegateDependencyAlreadyExists": _C.ALREADY_EXISTS,
    "CannotAddSelfAsDelegateDependency": _C.INVALID_ARGUMENT,
    "OutOfTransientStorage": _C.LIMIT_EXCEEDED,
    # ── MevShield ───────────────────────────────────────────────────────
    "BadEncKeyLen": _C.INVALID_ARGUMENT,
    "Unreachable": _C.INTERNAL,
    "TooManyPendingExtrinsics": _C.LIMIT_EXCEEDED,
    "WeightExceedsAbsoluteMax": _C.LIMIT_EXCEEDED,
    # ── LimitOrders ─────────────────────────────────────────────────────
    "OrderAlreadyProcessed": _C.ALREADY_EXISTS,
    "OrderCancelled": _C.EXPIRED,
    "OrderExpired": _C.EXPIRED,
    "PriceConditionNotMet": _C.TOO_EARLY,
    "SwapReturnedZero": _C.INSUFFICIENT_LIQUIDITY,
    "RootNetUidNotAllowed": _C.INVALID_ARGUMENT,
    "OrderNetUidMismatch": _C.INVALID_ARGUMENT,
    "LimitOrdersDisabled": _C.DISABLED,
    "RelayerMissMatch": _C.INVALID_ARGUMENT,
    "PartialFillsNotEnabled": _C.DISABLED,
    "IncorrectPartialFillAmount": _C.INVALID_ARGUMENT,
    "RelayerRequiredForPartialFill": _C.INVALID_ARGUMENT,
    "ChainIdMismatch": _C.INVALID_ARGUMENT,
    "PalletHotkeyNotRegistered": _C.NOT_REGISTERED,
    "ArithmeticOverflow": _C.INTERNAL,
    "DuplicateOrderInBatch": _C.INVALID_ARGUMENT,
    "ZeroShareInBatch": _C.INVALID_ARGUMENT,
}

# ── Pool-rejection custom codes ──────────────────────────────────────────
# ``InvalidTransaction::Custom(u8)`` codes from
# ``common/src/transaction_error.rs``. Pool rejections arrive as
# ``"Custom error: N"`` with no module name; map each code to the catalog
# name that ``From<Error> for CustomTransactionError`` produces when the
# mapping is 1:1, or to a ``DISPATCH_ERRORS`` name when the custom code is
# an umbrella / unused-but-reserved variant.
#
# Note: Frontier's ethereum pallet uses a *separate* overlapping Custom(u8)
# space for eth_* extrinsics; this table is for Subtensor/substrate calls.
CUSTOM_TRANSACTION_ERRORS: dict[int, str] = {
    0: "ColdkeySwapAnnounced",  # ColdkeyInSwapSchedule
    1: "StakeAmountTooLow",  # AmountTooLow | NotEnoughStakeToSetWeights
    2: "NotEnoughBalanceToStake",  # BalanceTooLow
    3: "SubnetNotExists",
    4: "HotKeyAccountNotExists",
    5: "NotEnoughStakeToWithdraw",
    6: "RateLimitExceeded",  # Committing/SettingWeightsTooFast | NetworkTx…
    7: "InsufficientLiquidity",
    8: "SlippageTooHigh",
    9: "TransferDisallowed",
    10: "HotKeyNotRegisteredInNetwork",
    11: "InvalidIpAddress",
    12: "ServingRateLimitExceeded",
    13: "InvalidPort",
    14: "ZeroMaxAmount",
    15: "InvalidRevealRound",
    16: "NoWeightsCommitFound",  # CommitNotFound
    17: "RevealTooEarly",  # CommitBlockNotInRevealRange
    18: "InputLengthsUnequal",
    19: "HotKeyNotRegisteredInSubNet",  # UidNotFound
    20: "EvmKeyAssociateRateLimitExceeded",
    21: "ColdkeySwapDisputed",
    22: "InvalidRealAccount",
    23: "FailedShieldedTxParsing",
    24: "InvalidShieldedTxPubKeyHash",
    25: "NonAssociatedColdKey",
    26: "DelegateTakeTooLow",
    27: "DelegateTakeTooHigh",
    255: "BadRequest",
}

# Standard ``InvalidTransaction`` / ``UnknownTransaction`` variants (non-Custom).
# Needle → name; needles are matched case-insensitively against the node's
# Display/RPC text (see ``sp_runtime::transaction_validity``) when there is
# no ``Custom error: N``.
INVALID_TRANSACTION_ERRORS: tuple[tuple[str, str], ...] = (
    ("inability to pay some fees", "Payment"),
    ("will be valid in the future", "Future"),
    ("transaction is outdated", "Stale"),
    ("has a bad signature", "BadProof"),
    ("call is not expected", "Call"),
    ("ancient birth block", "AncientBirthBlock"),
    ("block limit", "ExhaustsResources"),
    ("labelled as mandatory", "BadMandatory"),
    ("dispatch is mandatory", "MandatoryValidation"),
    ("invalid signing address", "BadSigner"),
    ("implicit data was unable to be calculated", "IndeterminateImplicit"),
    ("did not authorize any origin", "UnknownOrigin"),
    ("lookup information required to validate", "CannotLookup"),
    ("unsigned validator", "NoUnsignedValidator"),
    ("priority is too low", "PriorityTooLow"),
    ("already imported", "AlreadyImported"),
)

# ── Dispatch-level errors ────────────────────────────────────────────────
# ``DispatchError`` variants other than ``Module``: ``BadOrigin``,
# ``CannotLookup``, and the ``sp_runtime::TokenError`` variants. These are
# raised by runtime machinery (origin checks, the balances fungible traits)
# rather than by a pallet, so they never appear in the generated module-error
# catalog — which is why they live outside ``NAME_TO_CODE`` (the ``--names``
# gate pins that table to the catalog). Keyed by the bare variant name the
# receipt decoder yields; each entry carries the semantic code and a short
# description (there is no on-chain docstring to fall back on).
#
# Also covers custom-transaction names that have no module-error counterpart
# (shield/proxy validity checks and the BadRequest catch-all).
DISPATCH_ERRORS: dict[str, tuple[ErrorCode, str]] = {
    "BadOrigin": (
        _C.NOT_AUTHORIZED,
        "The call was dispatched with the wrong origin: the signing key (or the "
        "unsigned/root origin used) is not allowed to make this call.",
    ),
    "CannotLookup": (
        _C.NOT_FOUND,
        "Required information could not be looked up: an account referenced by "
        "the call, or data needed to validate the transaction in the pool.",
    ),
    # TokenError variants.
    "FundsUnavailable": (
        _C.INSUFFICIENT_BALANCE,
        "The account's spendable balance cannot cover the amount being moved plus "
        "fees; funds that are locked, reserved, or needed for the existential "
        "deposit do not count.",
    ),
    "OnlyProvider": (
        _C.INSUFFICIENT_BALANCE,
        "The balance being moved is the account's only provider reference, so "
        "moving it would destroy the account.",
    ),
    "BelowMinimum": (
        _C.INSUFFICIENT_BALANCE,
        "The receiving account would end up below the existential deposit.",
    ),
    "CannotCreate": (
        _C.INSUFFICIENT_BALANCE,
        "The receiving account does not exist and cannot be created by this call.",
    ),
    "UnknownAsset": (
        _C.NOT_FOUND,
        "The asset in question is unknown.",
    ),
    "Frozen": (
        _C.DISABLED,
        "The funds exist but are frozen and cannot be spent.",
    ),
    "Unsupported": (
        _C.DISABLED,
        "The operation is not supported by the asset.",
    ),
    "CannotCreateHold": (
        _C.INSUFFICIENT_BALANCE,
        "The account cannot be created to record the amount on hold.",
    ),
    "NotExpendable": (
        _C.INSUFFICIENT_BALANCE,
        "The withdrawal would drop the account below the existential deposit and reap it.",
    ),
    "Blocked": (
        _C.NOT_AUTHORIZED,
        "The account cannot receive the funds (it is blocked).",
    ),
    "StakeAmountTooLow": (
        _C.LIMIT_EXCEEDED,
        "The stake amount is below the chain minimum, or the hotkey's stake weight is "
        "below the floor required to set/commit weights. Check `btcli stake list`.",
    ),
    "RateLimitExceeded": (
        _C.RATE_LIMITED,
        "A per-key rate limit blocked this extrinsic before inclusion (weights commit/set "
        "or network registration). Wait for the rate-limit window, then retry.",
    ),
    "ZeroMaxAmount": (
        _C.INVALID_ARGUMENT,
        "The computed max stake/unstake amount for this price limit is zero, so the "
        "extrinsic cannot move any value. Relax the limit or wait for a better price.",
    ),
    "InvalidRealAccount": (
        _C.INVALID_ARGUMENT,
        "The real account on a proxy/as-multi call is invalid for this extrinsic.",
    ),
    "FailedShieldedTxParsing": (
        _C.INVALID_ARGUMENT,
        "The shielded (MEV-protected) extrinsic payload could not be parsed.",
    ),
    "InvalidShieldedTxPubKeyHash": (
        _C.INVALID_ARGUMENT,
        "The shielded extrinsic's public-key hash does not match the expected key.",
    ),
    "BadRequest": (
        _C.INVALID_ARGUMENT,
        "The node rejected the extrinsic as a bad request (unclassified validity failure).",
    ),
    # Standard InvalidTransaction / UnknownTransaction variants.
    "Payment": (
        _C.INSUFFICIENT_BALANCE,
        "The signing account cannot cover the transaction fee (and tip). Fund the "
        "account with free TAO or lower the tip; check with `btcli wallet balance`. "
        "MEV-shielded submissions charge the outer carrier in TAO only — if you "
        "only hold staked alpha, retry with `--no-mev-shield`.",
    ),
    "Future": (
        _C.TOO_EARLY,
        "The extrinsic nonce is ahead of the account's on-chain nonce, so the node "
        "will only accept it later. Wait for intervening extrinsics to land, or "
        "resubmit with the current nonce.",
    ),
    "Stale": (
        _C.EXPIRED,
        "The extrinsic nonce is below the account's on-chain nonce (already used or "
        "superseded). Rebuild and resign with a fresh nonce.",
    ),
    "BadProof": (
        _C.INVALID_ARGUMENT,
        "The signature does not match the extrinsic payload. Resign the call; with "
        "`--signer extension`, retry — if it keeps failing, use a local wallet.",
    ),
    "Call": (
        _C.INVALID_ARGUMENT,
        "This call is not expected by the runtime's transaction validation (filtered "
        "or unsupported at the pool).",
    ),
    "AncientBirthBlock": (
        _C.EXPIRED,
        "The extrinsic's era birth block is too old for the node's BlockHashCount "
        "window. Rebuild and resign against a recent block.",
    ),
    "ExhaustsResources": (
        _C.LIMIT_EXCEEDED,
        "Including this extrinsic would exceed the current block's weight/length "
        "limits. Retry in a later block, or reduce the call's size/weight.",
    ),
    "BadMandatory": (
        _C.INTERNAL,
        "A mandatory inherent call failed during block production. This is a "
        "validator/runtime fault, not a user-extrinsic issue.",
    ),
    "MandatoryValidation": (
        _C.INVALID_ARGUMENT,
        "A mandatory dispatch was submitted for pool validation; only inherent "
        "extrinsics may be mandatory.",
    ),
    "BadSigner": (
        _C.NOT_AUTHORIZED,
        "The signing address is disabled or known to be invalid.",
    ),
    "IndeterminateImplicit": (
        _C.INVALID_ARGUMENT,
        "The transaction extension could not compute its implicit signed data. "
        "Rebuild and resign the extrinsic.",
    ),
    "UnknownOrigin": (
        _C.NOT_AUTHORIZED,
        "No transaction extension authorized an origin for this extrinsic.",
    ),
    "NoUnsignedValidator": (
        _C.NOT_AUTHORIZED,
        "No unsigned-transaction validator accepted this extrinsic.",
    ),
    "PriorityTooLow": (
        _C.RATE_LIMITED,
        "The extrinsic's priority is too low to replace an existing pool entry with "
        "the same nonce. Raise the tip or wait for the pending extrinsic to land.",
    ),
    "AlreadyImported": (
        _C.ALREADY_EXISTS,
        "The extrinsic is already in the pool or on chain. Treat the submission as "
        "done, or rebuild with a new nonce if you meant a distinct call.",
    ),
    # Watch-status fatals from author_submitAndWatchExtrinsic.
    "Dropped": (
        _C.UNKNOWN,
        "The extrinsic was dropped from the transaction pool before inclusion.",
    ),
    "Usurped": (
        _C.ALREADY_EXISTS,
        "Another extrinsic with the same nonce replaced this one in the pool.",
    ),
    "Invalid": (
        _C.INVALID_ARGUMENT,
        "The pool marked the extrinsic invalid after submission.",
    ),
    "Retracted": (
        _C.UNKNOWN,
        "The extrinsic was retracted from the pool/chain watch.",
    ),
    "FinalityTimeout": (
        _C.UNKNOWN,
        "Watching the extrinsic timed out before finality was observed.",
    ),
    # MevShield inclusion-event failures (not module errors).
    "ExtrinsicDecodeFailed": (
        _C.INVALID_ARGUMENT,
        "The shielded extrinsic could not be decoded when the block author tried "
        "to include it. Resubmit with a fresh MEV-protected encrypt.",
    ),
    "ExtrinsicExpired": (
        _C.EXPIRED,
        "The shielded extrinsic sat in the MevShield queue past its lifetime and "
        "expired. Resubmit with MEV protection.",
    ),
    "DecryptionFailed": (
        _C.INVALID_ARGUMENT,
        "The block author failed to decrypt the shielded extrinsic. Resubmit with "
        "a fresh MEV-protected submission.",
    ),
    # Other DispatchError wrappers beyond Token/BadOrigin.
    "Overflow": (
        _C.INTERNAL,
        "An arithmetic overflow occurred while dispatching the call.",
    ),
    "Underflow": (
        _C.INTERNAL,
        "An arithmetic underflow occurred while dispatching the call.",
    ),
    "DivisionByZero": (
        _C.INTERNAL,
        "A division by zero occurred while dispatching the call.",
    ),
    "Arithmetic": (
        _C.INTERNAL,
        "An arithmetic error occurred while dispatching the call.",
    ),
    "Transactional": (
        _C.INTERNAL,
        "A transactional-layer error occurred while dispatching the call.",
    ),
    "Exhausted": (
        _C.LIMIT_EXCEEDED,
        "A runtime resource was exhausted while dispatching the call.",
    ),
    "Corruption": (
        _C.INTERNAL,
        "The runtime reported state corruption while dispatching the call.",
    ),
    "Unavailable": (
        _C.INTERNAL,
        "A required runtime resource was unavailable while dispatching the call.",
    ),
    "RootNotAllowed": (
        _C.NOT_AUTHORIZED,
        "This call cannot be dispatched with a root origin.",
    ),
    "Other": (
        _C.UNKNOWN,
        "An unspecified dispatch error occurred.",
    ),
}
