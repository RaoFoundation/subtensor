// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IRUNTIME_CONFIGURATION_ADDRESS = 0x0000000000000000000000000000000000000812;

interface IRuntimeConfiguration {
    function getEvmChainId() external view returns (uint64);
    function getTransactionRateLimit() external view returns (uint64);
}
