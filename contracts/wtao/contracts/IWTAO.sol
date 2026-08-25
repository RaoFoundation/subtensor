// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

interface Wrapper {
    // Event for depositing TAO, same as in WETH
    event Deposit(address indexed dst, uint wad);
    // Event for withdrawing TAO, same as in WETH
    event Withdrawal(address indexed src, uint wad);

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
    function deposit() external payable;

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
    function withdraw(uint amount) external;
}

interface IWTAO is IERC20, Wrapper {}
