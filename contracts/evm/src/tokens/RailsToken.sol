// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

import {Wire} from "../Wire.sol";
import {IPortal} from "../interfaces/IPortal.sol";

/// @title A canonical token of the rails on this chain.
/// @notice Not upgradeable. The portal that created it mints it when the hub says so. Only
/// a holder can burn its own tokens, by selling them: the portal cannot take tokens from
/// anyone, and no approval is ever given to it.
abstract contract RailsToken is ERC20 {
    /// @notice The portal that created the token: the only minter.
    IPortal public immutable portal;

    error OnlyPortal();

    modifier onlyPortal() {
        if (msg.sender != address(portal)) revert OnlyPortal();
        _;
    }

    constructor() {
        portal = IPortal(msg.sender);
    }

    /// @notice Amounts have 9 decimals, as on Bittensor.
    function decimals() public pure virtual override returns (uint8) {
        return 9;
    }

    /// @notice Burns `amount` of the caller's tokens and orders `destination` for them from
    /// the hub. If the hub cannot settle the order, it has the tokens minted back.
    /// @param minOut The least of the destination asset the caller accepts.
    /// @param deadline Unix seconds after which the hub refunds the order.
    /// @return nonce The order's sequence number at the portal.
    function sell(
        uint64 amount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) external payable returns (uint64 nonce) {
        _burn(msg.sender, amount);
        return portal.sold{value: msg.value}(msg.sender, amount, destination, minOut, deadline);
    }
}
