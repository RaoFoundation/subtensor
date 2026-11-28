// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Wire} from "../Wire.sol";

/// @notice What the vault and the canonical tokens call on the portal after they took a
/// payer's funds.
interface IPortal {
    /// @notice The vault locked `stableAmount` of the stablecoin, in its own decimals, for
    /// `payer`. Sends the order to the hub. Only the vault may call this.
    /// @return nonce The order's sequence number.
    function deposited(
        address payer,
        uint256 stableAmount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) external payable returns (uint64 nonce);

    /// @notice The calling canonical token burned `amount` of `payer`'s. Sends the order to
    /// the hub. Only a canonical token of this portal may call this.
    /// @return nonce The order's sequence number.
    function sold(
        address payer,
        uint64 amount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) external payable returns (uint64 nonce);
}
