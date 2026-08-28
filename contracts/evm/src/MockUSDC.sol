// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {BaseERC20} from "./BaseERC20.sol";

/// @notice Test-only stand-in for USDC on the fake Base chain (anvil).
/// Anyone can mint; 9 decimals per the rails rule (a production portal for
/// real 6-decimal USDC carries a decimal adapter instead).
contract MockUSDC is BaseERC20 {
    constructor() BaseERC20("Mock USDC", "USDC") {}

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}
