// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IRUNTIME_CONFIGURATION_ADDRESS = 0x0000000000000000000000000000000000000812;

interface IRuntimeConfiguration {
    function getEvmChainId() external view returns (uint64);
    function getTransactionRateLimit() external view returns (uint64);

    function getSubtensorEconomicConstants() external view returns (
        uint256 initialIssuance,
        uint256 initialRaoRecycledForRegistration,
        uint256 initialBurn,
        uint256 initialMinBurn,
        uint256 initialMaxBurn,
        uint256 initialMinStake,
        uint256 initialMinTransfer,
        uint256 minBurnUpperBound,
        uint256 maxBurnLowerBound,
        uint256 initialNetworkMinLockCost,
        uint256 keySwapCost,
        uint256 keySwapOnSubnetCost,
        uint256 minBalanceToPerformColdkeySwap
    );

    function getSubtensorSubnetConstants() external view returns (
        uint16 initialTempo,
        uint16 minTempo,
        uint16 maxTempo,
        uint16 initialMinAllowedUids,
        uint16 initialMaxAllowedUids,
        uint16 initialMaxAllowedValidators,
        uint16 initialImmunityPeriod,
        uint16 initialActivityCutoff,
        uint32 minActivityCutoffFactorMilli,
        uint32 maxActivityCutoffFactorMilli,
        uint8 maxImmuneUidsPercentage,
        uint16 initialSubnetOwnerCut,
        uint8 initialMaxEpochsPerBlock
    );

    function getSubtensorConsensusConstants() external view returns (
        uint16 initialMinAllowedWeights,
        uint16 initialEmissionValue,
        uint16 initialRho,
        int16 initialAlphaSigmoidSteepness,
        uint16 initialKappa,
        uint64 initialBondsMovingAverage,
        uint16 initialBondsPenalty,
        bool initialBondsResetOn,
        uint64 initialValidatorPruneLen,
        uint16 initialScalingLawPower,
        uint16 initialPruningScore,
        uint64 initialWeightsVersionKey,
        uint64 initialTaoWeight
    );

    function getSubtensorRegistrationConstants() external view returns (
        uint64 initialDifficulty,
        uint64 initialMinDifficulty,
        uint64 initialMaxDifficulty,
        uint16 initialAdjustmentInterval,
        uint64 initialAdjustmentAlpha,
        uint16 initialMaxRegistrationsPerBlock,
        uint16 initialTargetRegistrationsPerInterval,
        uint64 initialNetworkRateLimit,
        uint64 initialNetworkImmunityPeriod,
        uint64 initialNetworkLockReductionInterval,
        uint64 initialEmaPriceHalvingPeriod
    );

    function getSubtensorDelegationConstants() external view returns (
        uint16 initialDefaultDelegateTake,
        uint16 initialMinDelegateTake,
        uint16 initialDefaultChildKeyTake,
        uint16 initialMinChildKeyTake,
        uint16 initialMaxChildKeyTake,
        uint16 alphaHigh,
        uint16 alphaLow,
        bool liquidAlphaOn,
        bool yuma3On
    );

    function getSubtensorRateLimitConstants() external view returns (
        uint64 initialServingRateLimit,
        uint64 initialTxRateLimit,
        uint64 initialTxDelegateTakeRateLimit,
        uint64 initialTxChildKeyTakeRateLimit,
        uint64 evmKeyAssociateRateLimit,
        uint64 initialColdkeySwapAnnouncementDelay,
        uint64 initialColdkeySwapReannouncementDelay,
        uint64 initialDissolveNetworkScheduleDuration,
        uint64 initialStartCallDelay,
        uint64 hotkeySwapOnSubnetInterval,
        uint64 leaseDividendsDistributionInterval
    );

    function getSubtensorProtocolConstants() external view returns (
        uint32 maxCrv3CommitSizeBytes,
        uint32 maxAssociatedUidsPerEvmAddress,
        uint32 maxColdkeyCollateralHotkeys,
        uint128 accountFlagsAcceptLockedAlpha,
        uint64 minCommitRevealPeriods,
        uint64 maxCommitRevealPeriods,
        uint16 globalMaxSubnetCount,
        uint8 maxMechanismCountPerSubnet,
        uint64 votingPowerDisableGracePeriodBlocks,
        uint64 maxVotingPowerEmaAlpha,
        uint64 emissionBarUpdateInterval,
        uint64 stakingLockDuration,
        uint256 lockStateZeroThreshold,
        uint32 initialActivityCutoffFactorMilli,
        uint256 maxTaoIssuance
    );

    function getSubtensorSystemAccounts() external view returns (
        bytes32 subtensorPalletAccount,
        bytes32 burnAccount
    );

    function getBalancesConstants() external view returns (
        uint256 existentialDeposit,
        uint32 maxLocks,
        uint32 maxReserves,
        uint32 maxFreezes
    );

    function getProxyConstants() external view returns (
        uint256 proxyDepositBase,
        uint256 proxyDepositFactor,
        uint32 maxProxies,
        uint32 maxPending,
        uint256 announcementDepositBase,
        uint256 announcementDepositFactor
    );

    function getSchedulerConstants() external view returns (
        uint64 maximumWeightRefTime,
        uint64 maximumWeightProofSize,
        uint32 maxScheduledPerBlock
    );

    function getDrandConstants() external view returns (
        string memory quicknetChainHash,
        uint64 unsignedPriority,
        uint64 httpFetchTimeout,
        uint64 maxPulsesToFetch,
        uint64 maxKeptPulses,
        uint64 maxRemovedPulses
    );

    function getCrowdloanConstants() external view returns (
        uint256 minimumDeposit,
        uint256 absoluteMinimumContribution,
        uint64 minimumBlockDuration,
        uint64 maximumBlockDuration,
        uint32 refundContributorsLimit,
        uint32 maxContributors,
        bytes32 palletAccount
    );

    function getSwapConstants() external view returns (
        uint16 maxFeeRate,
        uint256 minimumLiquidity,
        uint256 minimumReserve,
        bytes32 protocolAccount
    );

    function getTimestampConstants() external view returns (uint64 minimumPeriod);
    function getAdminConstants() external view returns (uint32 maxAuthorities);
}
