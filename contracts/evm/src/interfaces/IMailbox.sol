// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice Minimal Hyperlane Mailbox surface used by the rails contracts.
interface IMailbox {
    function dispatch(uint32 destinationDomain, bytes32 recipientAddress, bytes calldata messageBody)
        external
        payable
        returns (bytes32 messageId);

    function quoteDispatch(uint32 destinationDomain, bytes32 recipientAddress, bytes calldata messageBody)
        external
        view
        returns (uint256 fee);

    function localDomain() external view returns (uint32);
}
