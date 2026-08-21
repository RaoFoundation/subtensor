// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Test} from "forge-std/Test.sol";
import {CanonicalShareToken} from "../src/CanonicalShareToken.sol";

contract CanonicalShareTokenTest is Test {
    CanonicalShareToken token;

    address owner = makeAddr("owner");
    address bridgeA = makeAddr("bridgeA");
    address user = makeAddr("user");

    function setUp() public {
        token = new CanonicalShareToken("Canonical USD", "cUSD", owner);
        vm.prank(owner);
        token.setMinterLimits(bridgeA, 1_000, 10);
    }

    function testMinterWindowEnforced() public {
        vm.startPrank(bridgeA);
        token.mint(user, 1_000);
        vm.expectRevert("mint rate limited");
        token.mint(user, 1);
        // 10 seconds refill 100 units.
        vm.warp(block.timestamp + 10);
        token.mint(user, 100);
        vm.stopPrank();
        assertEq(token.balanceOf(user), 1_100);
    }

    function testRevokedMinterCannotMint() public {
        vm.prank(owner);
        token.removeMinter(bridgeA);
        vm.prank(bridgeA);
        vm.expectRevert("not minter");
        token.mint(user, 1);
    }

    function testBurnReleasesMintHeadroom() public {
        vm.startPrank(bridgeA);
        token.mint(bridgeA, 1_000);
        vm.expectRevert("mint rate limited");
        token.mint(bridgeA, 1);
        token.burnFrom(bridgeA, 400);
        token.mint(bridgeA, 400);
        vm.stopPrank();
        assertEq(token.balanceOf(bridgeA), 1_000);
    }
}
