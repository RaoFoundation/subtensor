// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {ERC20Permit} from "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";

import {RailsToken} from "./RailsToken.sol";

/// @title Canonical TAO.
/// @notice One token is one TAO that the hub holds in its reserve on Bittensor.
contract CanonicalTao is RailsToken, ERC20Permit {
    constructor() ERC20("Bittensor", "TAO") ERC20Permit("Bittensor") {}

    function decimals() public pure override(ERC20, RailsToken) returns (uint8) {
        return RailsToken.decimals();
    }

    /// @notice Mints `amount` to `to`. Only the portal, on a credit from the hub.
    function mint(address to, uint256 amount) external onlyPortal {
        _mint(to, amount);
    }
}
