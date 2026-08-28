// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice The single rails rate-limit algorithm: a linearly-refilling
/// window. This mirrors `subtensor_runtime_common::rails::RateWindow` exactly
/// (saturating semantics), with "ticks" abstract: the runtime uses block
/// numbers, contracts use `block.timestamp` seconds.
library RateLimits {
    struct Window {
        uint64 limit;
        uint64 used;
        uint64 refillPerTick;
        uint64 lastTick;
    }

    /// @notice Decay `used` according to elapsed ticks (saturating).
    function refresh(Window storage w, uint64 nowTick) internal {
        uint64 last = w.lastTick;
        uint64 elapsed = nowTick > last ? nowTick - last : 0;
        uint256 refill = uint256(elapsed) * uint256(w.refillPerTick);
        uint64 used = w.used;
        // Safe: in the false branch refill < used <= u64::MAX.
        // forge-lint: disable-next-line(unsafe-typecast)
        w.used = refill >= used ? 0 : used - uint64(refill);
        w.lastTick = nowTick;
    }

    /// @notice Headroom available at `nowTick` without mutating storage.
    function available(Window storage w, uint64 nowTick) internal view returns (uint64) {
        uint64 last = w.lastTick;
        uint64 elapsed = nowTick > last ? nowTick - last : 0;
        uint256 refill = uint256(elapsed) * uint256(w.refillPerTick);
        uint64 used = w.used;
        // Safe: in the false branch refill < used <= u64::MAX.
        // forge-lint: disable-next-line(unsafe-typecast)
        uint64 decayed = refill >= used ? 0 : used - uint64(refill);
        uint64 limit = w.limit;
        return limit > decayed ? limit - decayed : 0;
    }

    /// @notice Atomically consume headroom; returns false if insufficient.
    function tryReserve(Window storage w, uint64 nowTick, uint64 amount) internal returns (bool) {
        refresh(w, nowTick);
        uint256 next = uint256(w.used) + uint256(amount);
        if (next > w.limit) {
            return false;
        }
        // Safe: next <= w.limit which is a uint64.
        // forge-lint: disable-next-line(unsafe-typecast)
        w.used = uint64(next);
        return true;
    }

    /// @notice Release previously consumed headroom (saturating).
    function release(Window storage w, uint64 nowTick, uint64 amount) internal {
        refresh(w, nowTick);
        uint64 used = w.used;
        w.used = amount >= used ? 0 : used - amount;
    }
}
