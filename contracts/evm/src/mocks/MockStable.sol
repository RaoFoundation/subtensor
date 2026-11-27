// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

/// @notice A stablecoin for local networks and tests: six decimals, and anyone can mint.
/// Never deploy it where it could be mistaken for money.
contract MockStable is ERC20 {
    constructor() ERC20("USD Coin (local)", "USDC") {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}
