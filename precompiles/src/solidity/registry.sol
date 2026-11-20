// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IPRECOMPILE_REGISTRY_ADDRESS = 0x0000000000000000000000000000000000000813;

interface IPrecompileRegistry {
    struct PrecompileStatus {
        bool isDeprecated;
        bool isDisabled;
        address newPrecompile;
        bytes4 newSelector;
        string message;
    }

    /// @notice Reports whether the containing precompile is disabled.
    /// @dev In v444, `selector` and the selector-lifecycle result fields are
    /// reserved for future use and do not establish whether a selector exists.
    function getPrecompileStatus(
        address precompile,
        bytes4 selector
    ) external view returns (PrecompileStatus memory);
}
