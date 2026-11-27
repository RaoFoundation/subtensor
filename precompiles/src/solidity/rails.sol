// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IRAILS_ADDRESS = 0x0000000000000000000000000000000000000815;

/// @notice The hub of the canonical rails, as the gateway contract and clients on
/// Bittensor's EVM reach it. All amounts have 9 decimals.
interface IRails {
    /// @notice Settles the order that the portal `sender` on transport domain `domain`
    /// sent. The hub takes this call only from the gateway contract.
    /// @param order The order, as the portal encoded it: twelve ABI words.
    /// @return status 1 settled, 2 refunded, 3 rejected.
    /// @return amountOut What the order delivered, if it was settled.
    function execute(
        uint32 domain,
        bytes32 sender,
        bytes calldata order
    ) external returns (uint8 status, uint64 amountOut);

    /// @notice Removes the message at `position`, the head of the hub's outbox. The hub
    /// takes this call only from the gateway contract, which makes it in the transaction
    /// that hands the message to the transport.
    function ackOutbound(uint64 position) external;

    /// @notice The message at the head of the hub's outbox.
    /// @return pending Whether there is a message.
    /// @return position Its position, to pass to `ackOutbound`.
    /// @return domain The transport domain of the portal it goes to.
    /// @return recipient That portal.
    /// @return body The message, as the portal decodes it.
    function nextOutbound()
        external
        view
        returns (
            bool pending,
            uint64 position,
            uint32 domain,
            bytes32 recipient,
            bytes memory body
        );

    /// @notice What `order`, sent now from the spoke of `bridge`, would deliver. The
    /// order's nonce, payer, minimum and deadline are not read. Costs the gas of settling
    /// an order.
    /// @return ok Whether the hub would settle the order.
    /// @return amountOut The amount of the destination asset it would deliver.
    /// @return reason If not ok, the name of the error.
    function quote(
        uint32 bridge,
        bytes calldata order
    ) external view returns (bool ok, uint64 amountOut, string memory reason);

    /// @notice What became of order `nonce` of `bridge`.
    /// @return status 0 unknown (not taken yet, or its receipt is no longer kept),
    /// 1 settled, 2 refunded, 3 rejected, 4 skipped.
    /// @return amountOut What the order delivered, if it was settled.
    /// @return blockNumber The block it was disposed of in.
    function receipt(
        uint32 bridge,
        uint64 nonce
    ) external view returns (uint8 status, uint64 amountOut, uint32 blockNumber);

    /// @notice The nonce of the next order the hub takes from `bridge`. Every order below
    /// it has been disposed of.
    function nextOrder(uint32 bridge) external view returns (uint64);

    /// @notice The alpha that one unit of the alpha token of `netuid` is worth now, times
    /// 1e18. Zero if the subnet has no market or its market only winds down.
    function alphaRate(uint16 netuid) external view returns (uint256);

    /// @notice The gamma that payments through the rails have spent at `service` on
    /// `netuid` for the address `payer`, since ever. A service credits an address by the
    /// growth of this total.
    function creditsOf(
        uint16 netuid,
        bytes32 service,
        address payer
    ) external view returns (uint64);
}
