pragma solidity ^0.8.0;

address constant ISUBNET_ADDRESS = 0x0000000000000000000000000000000000000803;

interface ISubnet {
    /// Registers a new network without specifying details.
    function registerNetwork(bytes32 hotkey) external payable;
    /// Registers a new network with specified subnet name, GitHub repository, and contact information.
    function registerNetwork(
        bytes32 hotkey,
        string memory subnetName,
        string memory githubRepo,
        string memory subnetContact,
        string memory subnetUrl,
        string memory discord,
        string memory description,
        string memory additional
    ) external payable;
    /// Registers a new network with specified subnet name, GitHub repository, contact information, and logo URL.
    function registerNetwork(
        bytes32 hotkey,
        string memory subnetName,
        string memory githubRepo,
        string memory subnetContact,
        string memory subnetUrl,
        string memory discord,
        string memory description,
        string memory logoUrl,
        string memory additional
    ) external payable;

    function getServingRateLimit(uint16 netuid) external view returns (uint64);

    function getNetworkRegistrationBlock(
        uint16 netuid
    ) external view returns (uint64);

    /**
     * @dev Returns the monotonic registration generation for a netuid.
     * The value increments whenever the netuid is successfully registered.
     */
    function getRegisteredSubnetCounter(
        uint16 netuid
    ) external view returns (uint64);

    function setServingRateLimit(
        uint16 netuid,
        uint64 servingRateLimit
    ) external payable;

    function getMinDifficulty(uint16 netuid) external view returns (uint64);

    function setMinDifficulty(
        uint16 netuid,
        uint64 minDifficulty
    ) external payable;

    function getMaxDifficulty(uint16 netuid) external view returns (uint64);

    function setMaxDifficulty(
        uint16 netuid,
        uint64 maxDifficulty
    ) external payable;

    function getWeightsVersionKey(uint16 netuid) external view returns (uint64);

    function setWeightsVersionKey(
        uint16 netuid,
        uint64 weightsVersionKey
    ) external payable;

    function getWeightsSetRateLimit(
        uint16 netuid
    ) external view returns (uint64);

    function setWeightsSetRateLimit(
        uint16 netuid,
        uint64 weightsSetRateLimit
    ) external payable;

    function getAdjustmentAlpha(uint16 netuid) external view returns (uint64);

    function setAdjustmentAlpha(
        uint16 netuid,
        uint64 adjustmentAlpha
    ) external payable;

    function getMaxWeightLimit(uint16 netuid) external view returns (uint16);

    function getImmunityPeriod(uint16) external view returns (uint16);

    function setImmunityPeriod(
        uint16 netuid,
        uint16 immunityPeriod
    ) external payable;

    function getMinAllowedWeights(uint16 netuid) external view returns (uint16);

    function setMinAllowedWeights(
        uint16 netuid,
        uint16 minAllowedWeights
    ) external payable;

    function getKappa(uint16) external view returns (uint16);

    function setKappa(uint16 netuid, uint16 kappa) external payable;

    function getRho(uint16) external view returns (uint16);

    function setRho(uint16 netuid, uint16 rho) external payable;

    function getAlphaSigmoidSteepness(
        uint16 netuid
    ) external view returns (uint16);

    function setAlphaSigmoidSteepness(
        uint16 netuid,
        uint16 steepness
    ) external payable;

    function getActivityCutoff(uint16 netuid) external view returns (uint16);

    function setActivityCutoff(
        uint16 netuid,
        uint16 activityCutoff
    ) external payable;

    function getActivityCutoffFactor(
        uint16 netuid
    ) external view returns (uint32);

    function setActivityCutoffFactor(
        uint16 netuid,
        uint32 factorMilli
    ) external payable;

    function getNetworkRegistrationAllowed(
        uint16 netuid
    ) external view returns (bool);

    function setNetworkRegistrationAllowed(
        uint16 netuid,
        bool networkRegistrationAllowed
    ) external payable;

    function getNetworkPowRegistrationAllowed(
        uint16 netuid
    ) external view returns (bool);

    function setNetworkPowRegistrationAllowed(
        uint16 netuid,
        bool networkPowRegistrationAllowed
    ) external payable;

    function getMinBurn(uint16 netuid) external view returns (uint64);

    function setMinBurn(uint16 netuid, uint64 minBurn) external payable;

    function getMaxBurn(uint16 netuid) external view returns (uint64);

    function setMaxBurn(uint16 netuid, uint64 maxBurn) external payable;

    /**
     * @dev Returns whether owner-cut emission is automatically stake-locked.
     */
    function getOwnerCutAutoLockEnabled(
        uint16 netuid
    ) external view returns (bool);

    /**
     * @dev Sets whether owner-cut emission is automatically stake-locked.
     * Callable by root or the subnet owner.
     */
    function setOwnerCutAutoLockEnabled(
        uint16 netuid,
        bool enabled
    ) external payable;

    function getDifficulty(uint16 netuid) external view returns (uint64);

    function setDifficulty(uint16 netuid, uint64 difficulty) external payable;

    function getBondsMovingAverage(
        uint16 netuid
    ) external view returns (uint64);

    function setBondsMovingAverage(
        uint16 netuid,
        uint64 bondsMovingAverage
    ) external payable;

    function getCommitRevealWeightsEnabled(
        uint16 netuid
    ) external view returns (bool);

    function setCommitRevealWeightsEnabled(
        uint16 netuid,
        bool commitRevealWeightsEnabled
    ) external payable;

    function getLiquidAlphaEnabled(uint16 netuid) external view returns (bool);

    /**
     * @dev Returns the liquid-alpha consensus source: 0 Current, 1 Previous,
     * 2 Auto.
     */
    function getLiquidAlphaConsensusMode(
        uint16 netuid
    ) external view returns (uint8);

    function isSubnetDissolving(uint16 netuid) external view returns (bool);

    /**
     * @dev Returns stable dissolution and cleanup state for a subnet.
     *
     * cleanupPhase is zero while cleanup has not started. Once cleanup is in
     * progress, the append-only phase codes are:
     * 1 root claimable dividends; 2 root claimed dividends;
     * 3 calculate stake value; 4 settle stakes; 5 clear alpha;
     * 6 clear hotkey totals; 7 clear stake locks; 8 clear decaying stake locks;
     * 9 finish stake cleanup; 10 clear protocol liquidity;
     * 11 purge subnet commitments; 12 clear network membership;
     * 13 clear network parameters; 14 clear network maps;
     * 15 update root weights; 16 clear childkey takes;
     * 17 clear childkeys; 18 clear parentkeys;
     * 19 clear last hotkey emissions; 20 clear last-epoch hotkey alpha;
     * 21 clear transaction rate-limit records; 22 clear network locks;
     * 23 clear decaying network locks.
     */
    function getSubnetDissolutionStatus(
        uint16 netuid
    )
        external
        view
        returns (
            bool isDissolving,
            bool cleanupInProgress,
            uint8 cleanupPhase
        );

    function setLiquidAlphaEnabled(
        uint16 netuid,
        bool liquidAlphaEnabled
    ) external payable;

    /**
     * @dev Sets the liquid-alpha consensus source: 0 Current, 1 Previous,
     * 2 Auto. Reverts for any other value.
     */
    function setLiquidAlphaConsensusMode(
        uint16 netuid,
        uint8 mode
    ) external payable;

    function getYuma3Enabled(uint16 netuid) external view returns (bool);

    function setYuma3Enabled(
        uint16 netuid,
        bool yuma3Enabled
    ) external payable;

    function getBondsResetEnabled(uint16 netuid) external view returns (bool);

    function setBondsResetEnabled(
        uint16 netuid,
        bool bondsResetEnabled
    ) external payable;


    function getAlphaValues(
        uint16 netuid
    ) external view returns (uint16, uint16);

    function setAlphaValues(
        uint16 netuid,
        uint16 alphaLow,
        uint16 alphaHigh
    ) external payable;

    function getCommitRevealWeightsInterval(
        uint16 netuid
    ) external view returns (uint64);

    function setCommitRevealWeightsInterval(
        uint16 netuid,
        uint64 commitRevealWeightsInterval
    ) external payable;

    function toggleTransfers(uint16 netuid, bool toggle) external payable;

    function setSubnetIdentity(
        uint16 netuid,
        string calldata subnetName,
        string calldata githubRepo,
        string calldata subnetContact,
        string calldata subnetUrl,
        string calldata discord,
        string calldata description,
        string calldata logoUrl,
        string calldata additional
    ) external;
    function updateSubnetSymbol(uint16 netuid, string calldata symbol) external;
    function triggerEpoch(uint16 netuid) external;
    function setBondsPenalty(uint16 netuid, uint16 bondsPenalty) external;
    function setMaxAllowedUids(uint16 netuid, uint16 maxAllowedUids) external;
    function setMaxBurnV2(uint16 netuid, uint64 maxBurn) external;
    function setMechanismCount(uint16 netuid, uint8 mechanismCount) external;
    function setMechanismEmissionSplit(
        uint16 netuid,
        bool hasSplit,
        uint16[] calldata split
    ) external;
    function setMinBurnV2(uint16 netuid, uint64 minBurn) external;
    function setOwnerCutEnabled(uint16 netuid, bool enabled) external;
    function setOwnerImmuneNeuronLimit(
        uint16 netuid,
        uint16 immuneNeurons
    ) external;
    function setTempo(uint16 netuid, uint16 tempo) external;
    function trimToMaxAllowedUids(uint16 netuid, uint16 maxUids) external;
    function getSubnetMetadata(
        uint16 netuid
    )
        external
        view
        returns (
            bytes memory tokenSymbol,
            bytes32 owner,
            bytes32 ownerHotkey,
            uint16 tempo,
            uint8 recycleOrBurn
        );
    function getSubnetCapacityConfig(
        uint16 netuid
    )
        external
        view
        returns (
            uint16 minAllowedUids,
            uint16 maxAllowedUids,
            uint16 maxAllowedValidators,
            uint16 adjustmentInterval,
            uint16 targetRegistrationsPerInterval,
            uint16 minNonImmuneUids,
            uint16 immuneOwnerUidsLimit,
            uint16 bondsPenalty,
            bool ownerCutEnabled,
            bool transfersEnabled,
            uint16 maxRegistrationsPerBlock,
            uint8 mechanismCount
        );
    function getMechanismEmissionSplit(
        uint16 netuid
    ) external view returns (bool exists, uint16[] memory split);
    function getBurnConfig(
        uint16 netuid
    ) external view returns (uint16 halfLife, uint128 increaseMultiplier);
    function getGlobalNetworkLimits()
        external
        view
        returns (
            uint16 minActivityCutoff,
            uint16 adminFreezeWindow,
            uint16 ownerHyperparamRateLimit,
            uint64 dissolveScheduleDuration,
            uint16 subnetLimit,
            uint16 totalNetworks,
            uint64 networkImmunityPeriod,
            uint64 startCallDelay,
            uint64 minNetworkLockCost,
            uint64 lastNetworkLockCost,
            uint64 networkLockReductionInterval,
            uint16 subnetOwnerCut
        );
    function getGlobalRateLimits()
        external
        view
        returns (
            uint64 networkRateLimit,
            uint64 weightsVersionKeyRateLimit,
            uint64 transactionRateLimit,
            uint64 delegateTakeRateLimit,
            uint64 childkeyTakeRateLimit,
            uint8 maxEpochsPerBlock
        );
    function getGlobalProtocolConfig()
        external
        view
        returns (
            uint8 maxMechanismCount,
            uint16 commitRevealWeightsVersion,
            uint64 networkRegistrationStartBlock,
            uint64 taoInRefundDeploymentBlock
        );
}
