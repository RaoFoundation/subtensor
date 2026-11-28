// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice The part of the Hyperlane Mailbox that the rails use.
interface IMailbox {
    /// @notice Sends `messageBody` to `recipientAddress` on `destinationDomain`.
    /// `msg.value` pays the transport's hooks.
    function dispatch(
        uint32 destinationDomain,
        bytes32 recipientAddress,
        bytes calldata messageBody
    ) external payable returns (bytes32 messageId);

    /// @notice What `dispatch` of the same message costs.
    function quoteDispatch(
        uint32 destinationDomain,
        bytes32 recipientAddress,
        bytes calldata messageBody
    ) external view returns (uint256 fee);

    function localDomain() external view returns (uint32);
}
