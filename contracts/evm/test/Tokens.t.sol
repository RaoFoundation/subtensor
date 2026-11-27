// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20Errors} from "@openzeppelin/contracts/interfaces/draft-IERC6093.sol";

import {Wire} from "../src/Wire.sol";
import {AlphaToken} from "../src/tokens/AlphaToken.sol";
import {RailsToken} from "../src/tokens/RailsToken.sol";
import {Fixture} from "./Fixture.sol";

contract TokensTest is Fixture {
    AlphaToken internal chutes;

    function setUp() public override {
        super.setUp();
        chutes = registerToken(CHUTES, "Chutes", "CHUTES");
    }

    function test_CanonicalTaoIsBittensorsTaoWithNineDecimals() public view {
        assertEq(tao.name(), "Bittensor");
        assertEq(tao.symbol(), "TAO");
        assertEq(tao.decimals(), 9);
        assertEq(address(tao.portal()), address(portal));
        assertEq(tao.totalSupply(), 0);
    }

    function test_AnAlphaTokenIsNamedByTheHub() public view {
        assertEq(chutes.name(), "Chutes");
        assertEq(chutes.symbol(), "CHUTES");
        assertEq(chutes.decimals(), 9);
        assertEq(chutes.netuid(), CHUTES);
        assertEq(chutes.rate(), 1e18);
        assertEq(address(chutes), portal.alphaTokenAddress(CHUTES));
    }

    function test_OnlyThePortalMints() public {
        for (uint256 i = 0; i < 4; ++i) {
            address caller = [alice, guardian, multisig, address(timelock)][i];
            vm.startPrank(caller);
            vm.expectRevert(RailsToken.OnlyPortal.selector);
            tao.mint(caller, 1);
            vm.expectRevert(RailsToken.OnlyPortal.selector);
            chutes.mint(caller, 1, 1e18);
            vm.expectRevert(RailsToken.OnlyPortal.selector);
            chutes.initialize(1, "x", "X");
            vm.stopPrank();
        }
    }

    function test_AnAlphaTokenIsNamedOnce() public {
        vm.prank(address(portal));
        vm.expectRevert(AlphaToken.AlreadyInitialized.selector);
        chutes.initialize(CHUTES, "Other", "OTHER");
    }

    function test_TheNumberOfTokensStandsStillWhileTheirWorthGrows() public {
        credit(Wire.ASSET_ALPHA, CHUTES, alice, 100 * UNIT, 1e18);
        assertEq(chutes.balanceOf(alice), 100 * UNIT);
        assertEq(chutes.convertToAssets(100 * UNIT), 100 * UNIT);
        assertEq(chutes.principalOf(alice), 100 * UNIT);

        // The vault earned a tenth. The hub reports the rate with its next mint.
        credit(Wire.ASSET_ALPHA, CHUTES, bob, 50 * UNIT, 1.1e18);

        // Alice's tokens did not change. They are worth a tenth more than she paid.
        assertEq(chutes.balanceOf(alice), 100 * UNIT);
        assertEq(chutes.convertToAssets(chutes.balanceOf(alice)), 110 * UNIT);
        assertEq(chutes.principalOf(alice), 100 * UNIT);
        // Bob paid the new rate.
        assertEq(chutes.principalOf(bob), 55 * UNIT);
        assertEq(chutes.convertToShares(55 * UNIT), 50 * UNIT);
        assertEq(chutes.rateUpdatedAt(), block.timestamp);
    }

    function test_PrincipalMovesWithTheTokens() public {
        credit(Wire.ASSET_ALPHA, CHUTES, alice, 100 * UNIT, 1.2e18);
        assertEq(chutes.principalOf(alice), 120 * UNIT);

        vm.prank(alice);
        chutes.transfer(bob, 25 * UNIT);
        assertEq(chutes.principalOf(alice), 90 * UNIT);
        assertEq(chutes.principalOf(bob), 30 * UNIT);

        // A sale takes its share of the principal away.
        vm.prank(alice);
        chutes.sell(uint64(25 * UNIT), toStable(alice), 0, type(uint64).max);
        assertEq(chutes.balanceOf(alice), 50 * UNIT);
        assertEq(chutes.principalOf(alice), 60 * UNIT);

        // Moving nothing, or moving to oneself, changes nothing.
        vm.startPrank(alice);
        chutes.transfer(bob, 0);
        chutes.transfer(alice, 10 * UNIT);
        vm.stopPrank();
        assertEq(chutes.principalOf(alice), 60 * UNIT);
        vm.prank(stranger);
        chutes.transfer(alice, 0);
        assertEq(chutes.principalOf(stranger), 0);
    }

    function test_ARefundMintsAtTheRateOnRecord() public {
        credit(Wire.ASSET_ALPHA, CHUTES, alice, 100 * UNIT, 1.5e18);
        // A refund reports no rate.
        credit(Wire.ASSET_ALPHA, CHUTES, bob, 10 * UNIT, 0);
        assertEq(chutes.rate(), 1.5e18);
        assertEq(chutes.principalOf(bob), 15 * UNIT);
    }

    function test_SellingBurnsTheSellersTokensAndSendsAnOrder() public {
        credit(Wire.ASSET_TAO, 0, alice, 40 * UNIT, 0);
        credit(Wire.ASSET_ALPHA, CHUTES, alice, 100 * UNIT, 1e18);

        vm.startPrank(alice);
        uint64 first =
            tao.sell(uint64(10 * UNIT), toStable(alice), uint64(3_900 * UNIT), 1_900_000_000);
        Wire.Order memory order = lastOrder();
        assertEq(order.nonce, first);
        assertEq(order.payer, alice);
        assertEq(order.source, Wire.ASSET_TAO);
        assertEq(order.sourceNetuid, 0);
        assertEq(order.amount, 10 * UNIT);
        assertEq(order.destination, Wire.TO_STABLE);
        assertEq(order.minOut, 3_900 * UNIT);
        assertEq(order.deadline, 1_900_000_000);
        assertEq(tao.balanceOf(alice), 30 * UNIT);
        assertEq(tao.totalSupply(), 30 * UNIT);

        uint64 second = chutes.sell(uint64(60 * UNIT), toTao(alice), 0, type(uint64).max);
        order = lastOrder();
        assertEq(second, first + 1);
        assertEq(order.source, Wire.ASSET_ALPHA);
        assertEq(order.sourceNetuid, CHUTES);
        assertEq(order.amount, 60 * UNIT);
        assertEq(chutes.totalSupply(), 40 * UNIT);
        vm.stopPrank();
    }

    function test_NobodySellsTokensTheyDoNotHold() public {
        credit(Wire.ASSET_TAO, 0, alice, 40 * UNIT, 0);
        // Not more than the balance.
        vm.prank(alice);
        vm.expectRevert(
            abi.encodeWithSelector(
                IERC20Errors.ERC20InsufficientBalance.selector, alice, 40 * UNIT, 41 * UNIT
            )
        );
        tao.sell(uint64(41 * UNIT), toStable(alice), 0, type(uint64).max);
        // Not someone else's, with or without an approval: a sale burns the caller's own.
        vm.prank(alice);
        tao.approve(bob, 40 * UNIT);
        vm.prank(bob);
        vm.expectRevert();
        tao.sell(uint64(1 * UNIT), toStable(bob), 0, type(uint64).max);
        assertEq(tao.balanceOf(alice), 40 * UNIT);
    }

    function test_ATokenIsNotSoldForItself() public {
        credit(Wire.ASSET_TAO, 0, alice, 40 * UNIT, 0);
        credit(Wire.ASSET_ALPHA, CHUTES, alice, 100 * UNIT, 1e18);
        vm.startPrank(alice);
        vm.expectRevert();
        tao.sell(uint64(UNIT), toTao(alice), 0, type(uint64).max);
        vm.expectRevert();
        chutes.sell(uint64(UNIT), toAlpha(CHUTES, alice), 0, type(uint64).max);
        // Another subnet's token is another asset.
        chutes.sell(uint64(UNIT), toAlpha(51, alice), 0, type(uint64).max);
        vm.stopPrank();
    }

    function test_CanonicalTaoTakesPermits() public {
        (address owner, uint256 key) = makeAddrAndKey("owner");
        credit(Wire.ASSET_TAO, 0, owner, 40 * UNIT, 0);
        bytes32 digest = keccak256(
            abi.encodePacked(
                "\x19\x01",
                tao.DOMAIN_SEPARATOR(),
                keccak256(
                    abi.encode(
                        keccak256(
                            "Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)"
                        ),
                        owner,
                        bob,
                        7 * UNIT,
                        0,
                        block.timestamp
                    )
                )
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest);
        tao.permit(owner, bob, 7 * UNIT, block.timestamp, v, r, s);
        assertEq(tao.allowance(owner, bob), 7 * UNIT);
    }

    /// The principals never sum to more than the worth that mints added, and a holder's
    /// principal is never more than its tokens' share of that.
    function testFuzz_PrincipalIsConserved(
        uint64[6] memory amounts,
        uint64[6] memory rates,
        uint8[12] memory moves,
        uint64[12] memory parts
    ) public {
        address[3] memory holders = [alice, bob, stranger];
        uint256 added;
        for (uint256 i = 0; i < amounts.length; ++i) {
            uint64 amount = uint64(bound(amounts[i], 1, 1_000_000 * UNIT));
            uint128 reported = uint128(bound(rates[i], 1e18, 5e18));
            credit(Wire.ASSET_ALPHA, CHUTES, holders[i % 3], amount, reported);
            added += uint256(amount) * reported / 1e18;
        }
        for (uint256 i = 0; i < moves.length; ++i) {
            address from = holders[moves[i] % 3];
            address to = holders[(moves[i] / 3) % 3];
            uint256 amount = bound(parts[i], 0, chutes.balanceOf(from));
            vm.prank(from);
            if (moves[i] % 2 == 0) {
                chutes.transfer(to, amount);
            } else if (amount != 0) {
                chutes.sell(uint64(amount), toStable(from), 0, type(uint64).max);
            }
            uint256 total;
            for (uint256 h = 0; h < 3; ++h) {
                total += chutes.principalOf(holders[h]);
                // An account without tokens keeps no principal.
                if (chutes.balanceOf(holders[h]) == 0) {
                    assertEq(chutes.principalOf(holders[h]), 0);
                }
            }
            assertLe(total, added);
        }
    }
}
