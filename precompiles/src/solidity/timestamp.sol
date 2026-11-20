// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant ITIMESTAMP_ADDRESS = 0x0000000000000000000000000000000000000811;

interface ITimestamp {
    function getTimestamp() external view returns (uint64);
    function wasUpdatedThisBlock() external view returns (bool);
}
