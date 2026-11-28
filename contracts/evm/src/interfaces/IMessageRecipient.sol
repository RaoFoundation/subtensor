// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @notice A contract that receives messages from the Hyperlane Mailbox.
interface IMessageRecipient {
    /// @notice Called by the Mailbox with a message that its security module verified.
    /// @param origin The domain the message was sent from.
    /// @param sender The address that sent it, left-padded.
    function handle(uint32 origin, bytes32 sender, bytes calldata body) external payable;
}

/// @notice A recipient that names the security module the Mailbox must verify its messages
/// with. The zero address selects the Mailbox's default.
interface ISpecifiesInterchainSecurityModule {
    function interchainSecurityModule() external view returns (address);
}
