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
    /// @return bridge The hub's id of the portal's bridge: the queue that holds the
    /// answer to the order.
    function execute(
        uint32 domain,
        bytes32 sender,
        bytes calldata order
    ) external returns (uint8 status, uint64 amountOut, uint32 bridge);

    /// @notice Removes message `nonce`, the head of the hub's queue for `bridge`, and
    /// moves `paid` from the bridge's account on the hub to the caller. The hub takes this
    /// call only from the gateway contract, which makes it in the transaction that pays
    /// the transport and hands it the message. Reverts if `paid` is more than the hub
    /// pays for the message: the limit that `nextOutbound` returns. The bridge's account
    /// holds a reserve for every queued message, and pays for the messages of that
    /// bridge alone.
    /// @param paid What the gateway pays the transport for the message from the hub's TAO,
    /// in wei. The hub counts it in its own unit, rounded up.
    function ackOutbound(uint32 bridge, uint64 nonce, uint256 paid) external;

    /// @notice The message at the head of the hub's queue for `bridge`. Each bridge has
    /// its own queue, first in first out.
    /// @return pending Whether there is a message.
    /// @return nonce Its nonce, to pass to `ackOutbound`.
    /// @return domain The transport domain of the portal it goes to.
    /// @return recipient That portal.
    /// @return body The message, as the portal decodes it.
    /// @return limit The most the hub pays the transport for the message now, in wei:
    /// the bridge's reserve for one message, and above it what the bridge's account can
    /// spare, up to the bridge's ceiling.
    function nextOutbound(
        uint32 bridge
    )
        external
        view
        returns (
            bool pending,
            uint64 nonce,
            uint32 domain,
            bytes32 recipient,
            bytes memory body,
            uint256 limit
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
