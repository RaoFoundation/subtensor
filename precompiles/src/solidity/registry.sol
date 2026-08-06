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

    function getPrecompileStatus(
        address precompile,
        bytes4 selector
    ) external view returns (PrecompileStatus memory);
}
