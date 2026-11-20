// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IDRAND_ADDRESS = 0x0000000000000000000000000000000000000810;

interface IDrand {
    function getBeaconConfig()
        external
        view
        returns (
            bytes memory publicKey,
            uint32 period,
            uint32 genesisTime,
            bytes memory chainHash,
            bytes memory groupHash,
            bytes memory schemeId,
            bytes memory beaconId
        );
    function getPulse(
        uint64 round
    ) external view returns (bool exists, uint64 storedRound, bytes memory randomness, bytes memory signature);
    function getStoredRoundRange() external view returns (uint64 oldest, uint64 latest);
    function getNextUnsignedAt() external view returns (uint64);
    function hasMigrationRun(bytes calldata key) external view returns (bool);
}
