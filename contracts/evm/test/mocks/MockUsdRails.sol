// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice Recording stub etched at the 0x814 precompile address in tests.
contract MockUsdRails {
    uint256 public gatewayExecuteCalls;
    uint64 public lastAmount;
    bytes public lastEnvelope;
    address public lastCaller;
    bool public shouldRevert;

    function setShouldRevert(bool v) external {
        shouldRevert = v;
    }

    function gatewayExecute(uint64 amount, bytes calldata envelope) external {
        require(!shouldRevert, "usd rails: forced revert");
        gatewayExecuteCalls += 1;
        lastAmount = amount;
        lastEnvelope = envelope;
        lastCaller = msg.sender;
    }
}
