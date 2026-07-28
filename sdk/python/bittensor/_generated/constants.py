"""Generated from runtime metadata by codegen. DO NOT EDIT BY HAND.

Regenerate with: python -m codegen <ws-endpoint>
Spec version: 441

Pallet constant descriptors: unpack into substrate.constant.
"""
from typing import NamedTuple


class Item(NamedTuple):
    """A (container, name) pair; unpack into query/constant calls."""

    container: str
    name: str


class System:
    BlockWeights = Item('System', 'BlockWeights')
    BlockLength = Item('System', 'BlockLength')
    BlockHashCount = Item('System', 'BlockHashCount')
    DbWeight = Item('System', 'DbWeight')
    Version = Item('System', 'Version')
    SS58Prefix = Item('System', 'SS58Prefix')

class Timestamp:
    MinimumPeriod = Item('Timestamp', 'MinimumPeriod')

class Aura:
    SlotDuration = Item('Aura', 'SlotDuration')

class Grandpa:
    MaxAuthorities = Item('Grandpa', 'MaxAuthorities')
    MaxNominators = Item('Grandpa', 'MaxNominators')
    MaxSetIdSessionEntries = Item('Grandpa', 'MaxSetIdSessionEntries')

class Balances:
    ExistentialDeposit = Item('Balances', 'ExistentialDeposit')
    MaxLocks = Item('Balances', 'MaxLocks')
    MaxReserves = Item('Balances', 'MaxReserves')
    MaxFreezes = Item('Balances', 'MaxFreezes')

class TransactionPayment:
    OperationalFeeMultiplier = Item('TransactionPayment', 'OperationalFeeMultiplier')

class SubtensorModule:
    InitialIssuance = Item('SubtensorModule', 'InitialIssuance')
    InitialMinAllowedWeights = Item('SubtensorModule', 'InitialMinAllowedWeights')
    InitialEmissionValue = Item('SubtensorModule', 'InitialEmissionValue')
    InitialTempo = Item('SubtensorModule', 'InitialTempo')
    InitialDifficulty = Item('SubtensorModule', 'InitialDifficulty')
    InitialMaxDifficulty = Item('SubtensorModule', 'InitialMaxDifficulty')
    InitialMinDifficulty = Item('SubtensorModule', 'InitialMinDifficulty')
    InitialRAORecycledForRegistration = Item('SubtensorModule', 'InitialRAORecycledForRegistration')
    InitialBurn = Item('SubtensorModule', 'InitialBurn')
    InitialMaxBurn = Item('SubtensorModule', 'InitialMaxBurn')
    InitialMinBurn = Item('SubtensorModule', 'InitialMinBurn')
    InitialMinStake = Item('SubtensorModule', 'InitialMinStake')
    InitialMinTransfer = Item('SubtensorModule', 'InitialMinTransfer')
    MinBurnUpperBound = Item('SubtensorModule', 'MinBurnUpperBound')
    MaxBurnLowerBound = Item('SubtensorModule', 'MaxBurnLowerBound')
    MinTempo = Item('SubtensorModule', 'MinTempo')
    MaxTempo = Item('SubtensorModule', 'MaxTempo')
    MinActivityCutoffFactorMilli = Item('SubtensorModule', 'MinActivityCutoffFactorMilli')
    MaxActivityCutoffFactorMilli = Item('SubtensorModule', 'MaxActivityCutoffFactorMilli')
    InitialAdjustmentInterval = Item('SubtensorModule', 'InitialAdjustmentInterval')
    InitialBondsMovingAverage = Item('SubtensorModule', 'InitialBondsMovingAverage')
    InitialBondsPenalty = Item('SubtensorModule', 'InitialBondsPenalty')
    InitialBondsResetOn = Item('SubtensorModule', 'InitialBondsResetOn')
    InitialTargetRegistrationsPerInterval = Item('SubtensorModule', 'InitialTargetRegistrationsPerInterval')
    InitialRho = Item('SubtensorModule', 'InitialRho')
    InitialAlphaSigmoidSteepness = Item('SubtensorModule', 'InitialAlphaSigmoidSteepness')
    InitialKappa = Item('SubtensorModule', 'InitialKappa')
    InitialMinAllowedUids = Item('SubtensorModule', 'InitialMinAllowedUids')
    InitialMaxAllowedUids = Item('SubtensorModule', 'InitialMaxAllowedUids')
    InitialValidatorPruneLen = Item('SubtensorModule', 'InitialValidatorPruneLen')
    InitialScalingLawPower = Item('SubtensorModule', 'InitialScalingLawPower')
    InitialImmunityPeriod = Item('SubtensorModule', 'InitialImmunityPeriod')
    InitialActivityCutoff = Item('SubtensorModule', 'InitialActivityCutoff')
    InitialMaxRegistrationsPerBlock = Item('SubtensorModule', 'InitialMaxRegistrationsPerBlock')
    InitialPruningScore = Item('SubtensorModule', 'InitialPruningScore')
    InitialMaxAllowedValidators = Item('SubtensorModule', 'InitialMaxAllowedValidators')
    InitialDefaultDelegateTake = Item('SubtensorModule', 'InitialDefaultDelegateTake')
    InitialMinDelegateTake = Item('SubtensorModule', 'InitialMinDelegateTake')
    InitialDefaultChildKeyTake = Item('SubtensorModule', 'InitialDefaultChildKeyTake')
    InitialMinChildKeyTake = Item('SubtensorModule', 'InitialMinChildKeyTake')
    InitialMaxChildKeyTake = Item('SubtensorModule', 'InitialMaxChildKeyTake')
    InitialWeightsVersionKey = Item('SubtensorModule', 'InitialWeightsVersionKey')
    InitialServingRateLimit = Item('SubtensorModule', 'InitialServingRateLimit')
    InitialTxRateLimit = Item('SubtensorModule', 'InitialTxRateLimit')
    InitialTxDelegateTakeRateLimit = Item('SubtensorModule', 'InitialTxDelegateTakeRateLimit')
    InitialTxChildKeyTakeRateLimit = Item('SubtensorModule', 'InitialTxChildKeyTakeRateLimit')
    InitialAdjustmentAlpha = Item('SubtensorModule', 'InitialAdjustmentAlpha')
    InitialNetworkImmunityPeriod = Item('SubtensorModule', 'InitialNetworkImmunityPeriod')
    InitialNetworkMinLockCost = Item('SubtensorModule', 'InitialNetworkMinLockCost')
    InitialSubnetOwnerCut = Item('SubtensorModule', 'InitialSubnetOwnerCut')
    InitialNetworkLockReductionInterval = Item('SubtensorModule', 'InitialNetworkLockReductionInterval')
    InitialNetworkRateLimit = Item('SubtensorModule', 'InitialNetworkRateLimit')
    KeySwapCost = Item('SubtensorModule', 'KeySwapCost')
    AlphaHigh = Item('SubtensorModule', 'AlphaHigh')
    AlphaLow = Item('SubtensorModule', 'AlphaLow')
    LiquidAlphaOn = Item('SubtensorModule', 'LiquidAlphaOn')
    Yuma3On = Item('SubtensorModule', 'Yuma3On')
    InitialColdkeySwapAnnouncementDelay = Item('SubtensorModule', 'InitialColdkeySwapAnnouncementDelay')
    InitialColdkeySwapReannouncementDelay = Item('SubtensorModule', 'InitialColdkeySwapReannouncementDelay')
    InitialDissolveNetworkScheduleDuration = Item('SubtensorModule', 'InitialDissolveNetworkScheduleDuration')
    InitialTaoWeight = Item('SubtensorModule', 'InitialTaoWeight')
    InitialEmaPriceHalvingPeriod = Item('SubtensorModule', 'InitialEmaPriceHalvingPeriod')
    InitialStartCallDelay = Item('SubtensorModule', 'InitialStartCallDelay')
    KeySwapOnSubnetCost = Item('SubtensorModule', 'KeySwapOnSubnetCost')
    HotkeySwapOnSubnetInterval = Item('SubtensorModule', 'HotkeySwapOnSubnetInterval')
    LeaseDividendsDistributionInterval = Item('SubtensorModule', 'LeaseDividendsDistributionInterval')
    MaxImmuneUidsPercentage = Item('SubtensorModule', 'MaxImmuneUidsPercentage')
    SubtensorPalletId = Item('SubtensorModule', 'SubtensorPalletId')
    BurnAccountId = Item('SubtensorModule', 'BurnAccountId')
    InitialMaxEpochsPerBlock = Item('SubtensorModule', 'InitialMaxEpochsPerBlock')

class Utility:
    batched_calls_limit = Item('Utility', 'batched_calls_limit')

class Multisig:
    DepositBase = Item('Multisig', 'DepositBase')
    DepositFactor = Item('Multisig', 'DepositFactor')
    MaxSignatories = Item('Multisig', 'MaxSignatories')

class Scheduler:
    MaximumWeight = Item('Scheduler', 'MaximumWeight')
    MaxScheduledPerBlock = Item('Scheduler', 'MaxScheduledPerBlock')

class Proxy:
    ProxyDepositBase = Item('Proxy', 'ProxyDepositBase')
    ProxyDepositFactor = Item('Proxy', 'ProxyDepositFactor')
    MaxProxies = Item('Proxy', 'MaxProxies')
    MaxPending = Item('Proxy', 'MaxPending')
    AnnouncementDepositBase = Item('Proxy', 'AnnouncementDepositBase')
    AnnouncementDepositFactor = Item('Proxy', 'AnnouncementDepositFactor')

class Commitments:
    MaxFields = Item('Commitments', 'MaxFields')
    InitialDeposit = Item('Commitments', 'InitialDeposit')
    FieldDeposit = Item('Commitments', 'FieldDeposit')

class SafeMode:
    EnterDuration = Item('SafeMode', 'EnterDuration')
    ExtendDuration = Item('SafeMode', 'ExtendDuration')
    EnterDepositAmount = Item('SafeMode', 'EnterDepositAmount')
    ExtendDepositAmount = Item('SafeMode', 'ExtendDepositAmount')
    ReleaseDelay = Item('SafeMode', 'ReleaseDelay')

class Drand:
    UnsignedPriority = Item('Drand', 'UnsignedPriority')
    HttpFetchTimeout = Item('Drand', 'HttpFetchTimeout')

class Crowdloan:
    PalletId = Item('Crowdloan', 'PalletId')
    MinimumDeposit = Item('Crowdloan', 'MinimumDeposit')
    AbsoluteMinimumContribution = Item('Crowdloan', 'AbsoluteMinimumContribution')
    MinimumBlockDuration = Item('Crowdloan', 'MinimumBlockDuration')
    MaximumBlockDuration = Item('Crowdloan', 'MaximumBlockDuration')
    RefundContributorsLimit = Item('Crowdloan', 'RefundContributorsLimit')
    MaxContributors = Item('Crowdloan', 'MaxContributors')

class Swap:
    ProtocolId = Item('Swap', 'ProtocolId')
    MaxFeeRate = Item('Swap', 'MaxFeeRate')
    MinimumLiquidity = Item('Swap', 'MinimumLiquidity')
    MinimumReserve = Item('Swap', 'MinimumReserve')

class Contracts:
    Schedule = Item('Contracts', 'Schedule')
    DepositPerByte = Item('Contracts', 'DepositPerByte')
    DefaultDepositLimit = Item('Contracts', 'DefaultDepositLimit')
    DepositPerItem = Item('Contracts', 'DepositPerItem')
    CodeHashLockupDepositPercent = Item('Contracts', 'CodeHashLockupDepositPercent')
    MaxCodeLen = Item('Contracts', 'MaxCodeLen')
    MaxStorageKeyLen = Item('Contracts', 'MaxStorageKeyLen')
    MaxTransientStorageSize = Item('Contracts', 'MaxTransientStorageSize')
    MaxDelegateDependencies = Item('Contracts', 'MaxDelegateDependencies')
    UnsafeUnstableInterface = Item('Contracts', 'UnsafeUnstableInterface')
    MaxDebugBufferLen = Item('Contracts', 'MaxDebugBufferLen')
    Environment = Item('Contracts', 'Environment')
    ApiVersion = Item('Contracts', 'ApiVersion')

class LimitOrders:
    MaxOrdersPerBatch = Item('LimitOrders', 'MaxOrdersPerBatch')
    PalletId = Item('LimitOrders', 'PalletId')
    PalletHotkey = Item('LimitOrders', 'PalletHotkey')

