// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "./IWTAO.sol";

contract WTAO is ERC20, Wrapper {
    /**
     * @dev Initializes the Wrapped TAO (WTAO) ERC20 token contract.
     * 
     * This constructor sets the name and symbol of the token to "Wrapped TAO" and "WTAO", respectively.
     * It calls the constructor of the ERC20 token standard to initialize the token with the provided name and symbol.
     *
     * Requirements:
     * - No arguments are required for the constructor as it directly initializes the ERC20 token with the name 
     *   "Wrapped TAO" and the symbol "WTAO".
     * 
     * The contract is deployed with these default values, and the total supply will be minted when users deposit TAO.
     */
    constructor() ERC20("Wrapped TAO", "WTAO") {}

    /**
     * @dev Allows users to deposit TAO into the contract and receive an equivalent amount of wrapped tokens.
     * 
     * The contract will mint the corresponding amount of wrapped tokens (ERC20) to the sender's address,
     * based on the amount of TAO sent with the transaction. The deposited TAO is stored within the contract.
     *
     * Requirements:
     * - The caller must send a non-zero amount of TAO (msg.value > 0).
     *
     * Emits a `Deposit` event indicating the user address and the amount deposited.
     *
     * @notice This function mints wrapped tokens corresponding to the TAO sent.
     */
    function deposit() external payable {
        require(msg.value > 0, "Must send TAO to deposit");

        _mint(msg.sender, msg.value);
        emit Deposit(msg.sender, msg.value);
    }

    /**
     * @dev Allows users to withdraw TAO by burning an equivalent amount of wrapped tokens (WTAO).
     * 
     * This function allows the caller to burn a specified amount of wrapped tokens (WTAO) from their balance,
     * and in return, the equivalent amount of TAO is transferred to the caller.
     *
     * Requirements:
     * - The caller must have a sufficient balance of wrapped tokens to burn (balanceOf(msg.sender) >= amount).
     * - The contract must have enough TAO balance to fulfill the withdrawal.
     *
     * Emits a `Withdrawal` event indicating the user address and the amount withdrawn.
     *
     * @param amount The amount of wrapped tokens (WTAO) to burn and the corresponding TAO to withdraw.
     */
    function withdraw(uint256 amount) external {
        require(balanceOf(msg.sender) >= amount, "Insufficient balance");

        _burn(msg.sender, amount);
        payable(msg.sender).transfer(amount);
        emit Withdrawal(msg.sender, amount);
    }

    /**
     * @dev Returns the balance of TAO held by the contract.
     * 
     * This function allows external callers to check the total amount of TAO stored in the contract.
     * It is primarily used for tracking the contract's TAO balance, which is accumulated from user deposits.
     *
     * @return uint256 The amount of TAO (in wei, specifically 1e-18) currently held by the contract.
     */
    function getContractETHBalance() external view returns (uint256) {
        return address(this).balance;
    }
}
