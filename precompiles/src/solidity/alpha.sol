pragma solidity ^0.8.0;

address constant IALPHA_ADDRESS = 0x0000000000000000000000000000000000000808;

interface IAlpha {
    /// @dev Returns the current alpha price for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The alpha price in RAO per alpha.
    function getAlphaPrice(uint16 netuid) external view returns (uint256);

    /// @dev Returns the moving (EMA) alpha price for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The moving alpha price in RAO per alpha.
    function getMovingAlphaPrice(uint16 netuid) external view returns (uint256);

    /// @dev Returns the amount of TAO in the pool for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The TAO amount in the pool.
    function getTaoInPool(uint16 netuid) external view returns (uint64);

    /// @dev Returns the amount of alpha in the pool for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The alpha amount in the pool.
    function getAlphaInPool(uint16 netuid) external view returns (uint64);

    /// @dev Returns the amount of alpha outside the pool for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The alpha amount outside the pool.
    function getAlphaOutPool(uint16 netuid) external view returns (uint64);

    /// @dev Returns the total alpha issuance for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The total alpha issuance.
    function getAlphaIssuance(uint16 netuid) external view returns (uint64);

    /// @dev Returns the global TAO weight.
    /// @return The TAO weight value.
    function getTaoWeight() external view returns (uint256);

    /// @dev Simulates swapping TAO for alpha.
    /// @param netuid The subnet identifier.
    /// @param tao The amount of TAO to swap.
    /// @return The amount of alpha that would be received.
    function simSwapTaoForAlpha(
        uint16 netuid,
        uint64 tao
    ) external view returns (uint256);

    /// @dev Simulates swapping alpha for TAO.
    /// @param netuid The subnet identifier.
    /// @param alpha The amount of alpha to swap.
    /// @return The amount of TAO that would be received.
    function simSwapAlphaForTao(
        uint16 netuid,
        uint64 alpha
    ) external view returns (uint256);

    /// @dev Returns the mechanism type for a subnet (0 for Stable, 1 for Dynamic).
    /// @param netuid The subnet identifier.
    /// @return The subnet mechanism type.
    function getSubnetMechanism(uint16 netuid) external view returns (uint16);

    /// @dev Returns the root subnet unique identifier.
    /// @return The root subnet ID.
    function getRootNetuid() external view returns (uint16);

    /// @dev Returns the EMA price halving blocks parameter for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The number of blocks for EMA price halving.
    function getEMAPriceHalvingBlocks(
        uint16 netuid
    ) external view returns (uint64);

    /// @dev Returns the transaction volume for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The subnet volume.
    function getSubnetVolume(uint16 netuid) external view returns (uint256);

    /// @dev Returns the amount of tao emission into the pool per block for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The tao-in emission per block.
    function getTaoInEmission(uint16 netuid) external view returns (uint256);

    /// @dev Returns the amount of alpha emission into the pool per block for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The alpha-in emission per block.
    function getAlphaInEmission(uint16 netuid) external view returns (uint256);

    /// @dev Returns the amount of alpha emission outside the pool per block for a subnet.
    /// @param netuid The subnet identifier.
    /// @return The alpha-out emission per block.
    function getAlphaOutEmission(uint16 netuid) external view returns (uint256);

    /// @dev Returns the sum of alpha prices for all subnets.
    /// @return The sum of alpha prices.
    function getSumAlphaPrice() external view returns (uint256);

    /// @dev Returns the CK burn rate.
    /// @return The CK burn rate.
    function getCKBurn() external view returns (uint256);

    /// mode: 0 = burn, 1 = recycle.
    function setRecycleOrBurn(uint16 netuid, uint8 mode) external;
    function setBurnHalfLife(uint16 netuid, uint16 burnHalfLife) external;
    /// Raw U64F64 bits.
    function setBurnIncreaseMultiplier(
        uint16 netuid,
        uint128 rawMultiplier
    ) external;
    function getEmissionAccounting(
        uint16 netuid,
        bytes32 hotkey
    )
        external
        view
        returns (
            uint64 alphaDividends,
            uint64 rootAlphaDividends,
            uint64 lastHotkeyEmission,
            uint64 pendingServerEmission,
            uint64 pendingValidatorEmission,
            uint64 pendingRootAlphaDividends,
            uint64 pendingOwnerCut,
            uint128 minerBurned,
            uint64 raoRecycledForRegistration
        );
    function getSubnetEconomicState(
        uint16 netuid
    )
        external
        view
        returns (
            bool emissionEnabled,
            uint128 rootProportion,
            uint64 excessTao,
            uint64 rootSellTao,
            uint64 protocolAlpha
        );
    function getSubnetFlowState(
        uint16 netuid
    )
        external
        view
        returns (
            int64 taoFlow,
            bool hasTaoFlowEma,
            uint64 taoFlowEmaBlock,
            int128 taoFlowEma,
            int64 protocolFlow,
            bool hasProtocolFlowEma,
            uint64 protocolFlowEmaBlock,
            int128 protocolFlowEma
        );
    function getEmissionGateConfig()
        external
        view
        returns (
            uint64 blockEmission,
            int128 movingAlpha,
            bool netTaoFlowEnabled,
            int128 taoFlowCutoff,
            uint128 flowNormExponent,
            uint128 emissionBarQuantile,
            uint128 emissionGateExponent,
            uint128 emissionGateBar,
            uint64 flowEmaSmoothingFactor
        );
    function getSwapState(
        uint16 netuid
    )
        external
        view
        returns (
            uint16 feeRate,
            bool initialized,
            uint64 quoteWeight,
            uint64 taoReservoir,
            uint64 alphaReservoir
        );
    function hasSwapMigrationRun(
        bytes calldata migrationName
    ) external view returns (bool);
}
