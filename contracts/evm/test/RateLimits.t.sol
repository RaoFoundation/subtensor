// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Test} from "forge-std/Test.sol";
import {RateLimits} from "../src/RateLimits.sol";

/// Harness exposing the library on a storage window.
contract WindowHarness {
    using RateLimits for RateLimits.Window;

    RateLimits.Window public w;

    constructor(uint64 limit, uint64 refill) {
        w.limit = limit;
        w.refillPerTick = refill;
    }

    function tryReserve(uint64 nowTick, uint64 amount) external returns (bool) {
        return w.tryReserve(nowTick, amount);
    }

    function available(uint64 nowTick) external view returns (uint64) {
        return w.available(nowTick);
    }

    function release(uint64 nowTick, uint64 amount) external {
        w.release(nowTick, amount);
    }
}

/// Mirrors the Rust unit tests in common/src/rails.rs so both sides of the
/// bridge provably run the same algorithm.
contract RateLimitsTest is Test {
    function testReserveAndRefillParity() public {
        WindowHarness h = new WindowHarness(100, 10);
        assertTrue(h.tryReserve(0, 100));
        assertFalse(h.tryReserve(0, 1));
        // 5 ticks refill 50 units.
        assertEq(h.available(5), 50);
        assertTrue(h.tryReserve(5, 50));
        assertFalse(h.tryReserve(5, 1));
        // Full refill after long idle; never exceeds limit.
        assertEq(h.available(1000), 100);
    }

    function testReleaseParity() public {
        WindowHarness h = new WindowHarness(100, 0);
        assertTrue(h.tryReserve(0, 80));
        h.release(0, 30);
        assertEq(h.available(0), 50);
        // Releasing more than used saturates to zero used.
        h.release(0, 1000);
        assertEq(h.available(0), 100);
    }
}
