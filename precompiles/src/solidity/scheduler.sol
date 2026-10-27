// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant ISCHEDULER_ADDRESS = 0x000000000000000000000000000000000000080F;

interface IScheduler {
    struct ScheduledCall {
        bool exists;
        bool hasTaskId;
        bytes32 taskId;
        uint8 priority;
        bytes32 callHash;
        bool hasCallLength;
        uint32 callLength;
        bool isPeriodic;
        uint64 period;
        uint32 remaining;
    }

    function getIncompleteSince() external view returns (bool exists, uint64 blockNumber);
    function getScheduledCallCount(uint64 when) external view returns (uint32);
    function getScheduledCall(
        uint64 when,
        uint32 index
    ) external view returns (ScheduledCall memory);
    function getRetry(
        uint64 when,
        uint32 index
    ) external view returns (bool exists, uint8 totalRetries, uint8 remaining, uint64 period);
    function getTaskAddress(
        bytes32 taskId
    ) external view returns (bool exists, uint64 when, uint32 index);
}
