// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {SafeCast} from "@openzeppelin/contracts/utils/math/SafeCast.sol";

/// @title A linearly refilling allowance.
/// @notice At most `limit` can be used at once, and what was used is given back evenly over
/// `period` seconds: an empty window is full again after `period`. The hub runs the same
/// algorithm over blocks (`RateWindow` in the subtensor repository).
library RateLimit {
    struct Window {
        uint128 limit;
        uint128 used;
        uint64 period;
        uint64 updatedAt;
    }

    error RateLimited(uint256 amount, uint256 available);

    /// @notice What is in use at time `time`, after the refill since the last use.
    function usedAt(Window memory window, uint256 time) internal pure returns (uint256) {
        if (window.period == 0) return 0;
        uint256 elapsed = time - window.updatedAt;
        if (elapsed >= window.period) return 0;
        uint256 refilled = uint256(window.limit) * elapsed / window.period;
        return window.used > refilled ? window.used - refilled : 0;
    }

    /// @notice What can be used at time `time`.
    function availableAt(Window memory window, uint256 time) internal pure returns (uint256) {
        uint256 used = usedAt(window, time);
        return window.limit > used ? window.limit - used : 0;
    }

    /// @notice Uses `amount` now, or reverts if the window does not have it.
    function consume(Window storage window, uint256 amount) internal {
        Window memory current = window;
        uint256 used = usedAt(current, block.timestamp);
        uint256 available = current.limit > used ? current.limit - used : 0;
        if (amount > available) revert RateLimited(amount, available);
        window.used = SafeCast.toUint128(used + amount);
        window.updatedAt = SafeCast.toUint64(block.timestamp);
    }

    /// @notice Sets the limit and the period. What is in use now stays in use.
    function retarget(Window storage window, uint128 limit, uint64 period) internal {
        uint256 used = usedAt(window, block.timestamp);
        window.limit = limit;
        window.period = period;
        window.used = SafeCast.toUint128(used);
        window.updatedAt = SafeCast.toUint64(block.timestamp);
    }
}
