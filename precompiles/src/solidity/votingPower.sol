// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IVOTING_POWER_ADDRESS = 0x000000000000000000000000000000000000080D;

interface IVotingPower {
    /// @dev Returns the voting power (EMA of stake) for a hotkey on a subnet.
    /// Returns 0 if the hotkey has no entry, tracking is disabled for the
    /// subnet, or the hotkey is not registered.
    /// @param netuid The subnet identifier.
    /// @param hotkey The hotkey public key (32 bytes).
    /// @return The voting power in rao (same precision as stake).
    function getVotingPower(
        uint16 netuid,
        bytes32 hotkey
    ) external view returns (uint256);

    /// @dev Returns whether voting power tracking is enabled for a subnet.
    /// @param netuid The subnet identifier.
    function isVotingPowerTrackingEnabled(
        uint16 netuid
    ) external view returns (bool);

    /// @dev Returns the block at which voting power tracking will be disabled
    /// (0 if not scheduled). Tracking continues until that block, then stops.
    /// @param netuid The subnet identifier.
    function getVotingPowerDisableAtBlock(
        uint16 netuid
    ) external view returns (uint64);

    /// @dev Returns the EMA alpha used for voting power calculation, with 18
    /// decimal precision (1.0 = 10^18). Higher alpha reacts faster to stake
    /// changes.
    /// @param netuid The subnet identifier.
    function getVotingPowerEmaAlpha(
        uint16 netuid
    ) external view returns (uint64);

    /// @dev Returns the sum of voting power across all validators on a subnet,
    /// useful for computing voting thresholds (e.g. a 51% quorum).
    /// @param netuid The subnet identifier.
    function getTotalVotingPower(uint16 netuid) external view returns (uint256);

    function enableVotingPowerTracking(uint16 netuid) external;
    function disableVotingPowerTracking(uint16 netuid) external;
}
