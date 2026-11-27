// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";

import {RateLimit} from "../src/RateLimit.sol";

contract RateLimitHarness {
    using RateLimit for RateLimit.Window;

    RateLimit.Window public window;

    function retarget(uint128 limit, uint64 period) external {
        window.retarget(limit, period);
    }

    function consume(uint256 amount) external {
        window.consume(amount);
    }

    function available() external view returns (uint256) {
        return window.availableAt(block.timestamp);
    }
}

contract RateLimitTest is Test {
    RateLimitHarness internal limiter = new RateLimitHarness();

    function setUp() public {
        vm.warp(1_800_000_000);
        limiter.retarget(1_000, 100);
    }

    function test_AFullWindowGivesTheLimitAtOnce() public {
        assertEq(limiter.available(), 1_000);
        limiter.consume(1_000);
        assertEq(limiter.available(), 0);
        vm.expectRevert(abi.encodeWithSelector(RateLimit.RateLimited.selector, 1, 0));
        limiter.consume(1);
    }

    function test_WhatWasUsedRefillsEvenlyOverThePeriod() public {
        limiter.consume(1_000);
        skip(25);
        assertEq(limiter.available(), 250);
        limiter.consume(250);
        skip(50);
        assertEq(limiter.available(), 500);
        skip(1_000);
        assertEq(limiter.available(), 1_000);
    }

    function test_ALimitOfZeroAllowsNothing() public {
        limiter.retarget(0, 100);
        assertEq(limiter.available(), 0);
        limiter.consume(0);
        vm.expectRevert(abi.encodeWithSelector(RateLimit.RateLimited.selector, 1, 0));
        limiter.consume(1);
    }

    function test_APeriodOfZeroRefillsAtOnce() public {
        limiter.retarget(1_000, 0);
        limiter.consume(1_000);
        assertEq(limiter.available(), 1_000);
        vm.expectRevert(abi.encodeWithSelector(RateLimit.RateLimited.selector, 1_001, 1_000));
        limiter.consume(1_001);
    }

    function test_RetargetingKeepsWhatIsInUse() public {
        limiter.consume(800);
        limiter.retarget(500, 100);
        // More is in use than the new limit allows: nothing is available until it refills.
        assertEq(limiter.available(), 0);
        skip(60);
        assertEq(limiter.available(), 0);
        skip(10);
        assertEq(limiter.available(), 50);
    }

    /// Whatever the uses and the waits, no span of one period lets more than twice the
    /// limit through, and no single moment lets more than the limit.
    function testFuzz_UseNeverExceedsTheLimitPlusTheRefill(
        uint128 limit,
        uint64 period,
        uint256[8] memory amounts,
        uint32[8] memory waits
    ) public {
        limit = uint128(bound(limit, 1, type(uint96).max));
        period = uint64(bound(period, 1, 365 days));
        limiter.retarget(limit, period);
        uint256 start = vm.getBlockTimestamp();
        uint256 total;
        for (uint256 i = 0; i < amounts.length; ++i) {
            skip(bound(waits[i], 0, period / 4));
            uint256 available = limiter.available();
            assertLe(available, limit);
            uint256 amount = bound(amounts[i], 0, available);
            limiter.consume(amount);
            total += amount;
            // What passed is at most a full window plus what refilled since the start.
            uint256 refilled = uint256(limit) * (vm.getBlockTimestamp() - start) / period;
            assertLe(total, uint256(limit) + refilled + 1);
            // One more than what is left never passes.
            uint256 over = limiter.available() + 1;
            vm.expectRevert(abi.encodeWithSelector(RateLimit.RateLimited.selector, over, over - 1));
            limiter.consume(over);
        }
    }
}
