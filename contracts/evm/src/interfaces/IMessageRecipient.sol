// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice Hyperlane message recipient interface.
interface IMessageRecipient {
    function handle(uint32 origin, bytes32 sender, bytes calldata messageBody) external payable;
}
