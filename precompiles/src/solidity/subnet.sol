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
        uint64 immunityPeriod
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
    ) external view returns (unt16);

    function setAlphaSigmoidSteepness(
        uint16 netuid,
        int16 steepness
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

    function setLiquidAlphaEnabled(
        uint16 netuid,
        bool liquidAlphaEnabled
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
}
