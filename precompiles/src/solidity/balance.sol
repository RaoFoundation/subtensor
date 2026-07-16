// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IBALANCE_ADDRESS = 0x000000000000000000000000000000000000080E;

interface IBalance {
    /// @dev Returns the native free TAO balance for an ss58 account public key.
    /// @param coldkey The coldkey public key (32 bytes).
    /// @return The free balance in rao (1 TAO = 1e9 rao).
    function getFreeBalance(bytes32 coldkey) external view returns (uint256);
}
