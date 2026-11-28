// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {
    PausableUpgradeable
} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {SafeCast} from "@openzeppelin/contracts/utils/math/SafeCast.sol";

import {Portal} from "../src/Portal.sol";
import {RateLimit} from "../src/RateLimit.sol";
import {Wire} from "../src/Wire.sol";
import {MockStable} from "../src/mocks/MockStable.sol";
import {AlphaToken} from "../src/tokens/AlphaToken.sol";
import {Fixture} from "./Fixture.sol";

/// @notice A payer that takes the change of a transport fee and counts it.
contract PayingContract {
    uint256 public change;

    receive() external payable {
        change += msg.value;
    }
}

contract PortalTest is Fixture {
    event OrderSent(
        uint64 indexed nonce,
        address indexed payer,
        uint8 source,
        uint16 sourceNetuid,
        uint64 amount,
        Wire.Destination destination,
        uint64 minOut,
        uint64 deadline,
        bytes32 messageId
    );
    event Credited(
        uint64 indexed nonce,
        address indexed to,
        uint8 asset,
        uint16 netuid,
        uint64 amount,
        bool answersOrder,
        uint64 indexed order,
        bool refund
    );
    event TokenCreated(uint16 indexed netuid, address token, string name, string symbol);
    event RegistryMismatch(uint16 indexed netuid, address listed, address token);
    event FeesSwept(address indexed recipient, uint256 amount);
    event StablePaid(address indexed to, uint256 amount, uint256 stillOwed);

    // ---------------------------------------------------------------- orders

    function test_ADepositBecomesTheOrderTheHubReads() public {
        stable.mint(alice, 250 * USDC);
        vm.startPrank(alice);
        stable.approve(address(vault), 250 * USDC);
        uint64 nonce = vault.deposit(
            250 * USDC, toCredits(51, keccak256("service"), bob), uint64(240 * UNIT), 1_900_000_000
        );
        vm.stopPrank();

        assertEq(nonce, 0);
        assertEq(portal.nextOrder(), 1);
        (uint32 destination, bytes32 recipient, bytes memory body,) = mailbox.sent(0);
        assertEq(destination, HUB_DOMAIN);
        assertEq(recipient, GATEWAY);
        assertEq(
            body,
            abi.encode(
                Wire.ORDER_VERSION,
                uint64(0),
                alice,
                Wire.ASSET_STABLE,
                uint16(0),
                uint64(250 * UNIT),
                Wire.TO_CREDITS,
                uint16(51),
                keccak256("service"),
                bob,
                uint64(240 * UNIT),
                uint64(1_900_000_000)
            )
        );
        assertEq(body.length, 384);
    }

    function test_OrdersAreNumberedInSequence() public {
        for (uint64 i = 0; i < 5; ++i) {
            assertEq(deposit(alice, 10 * USDC, toTao(alice)), i);
            assertEq(lastOrder().nonce, i);
        }
        assertEq(mailbox.sentCount(), 5);
    }

    function test_OnlyTheVaultAndTheTokensSendOrders() public {
        Wire.Destination memory destination = toTao(alice);
        for (uint256 i = 0; i < 5; ++i) {
            address caller = [alice, guardian, multisig, address(timelock), address(tao)][i];
            vm.prank(caller);
            vm.expectRevert(Portal.OnlyVault.selector);
            portal.deposited(alice, 100 * USDC, destination, 0, type(uint64).max);
        }
        for (uint256 i = 0; i < 5; ++i) {
            address caller = [alice, guardian, multisig, address(timelock), address(vault)][i];
            vm.prank(caller);
            vm.expectRevert(abi.encodeWithSelector(Portal.NotACanonicalToken.selector, caller));
            portal.sold(alice, 100, destination, 0, type(uint64).max);
        }
        assertEq(mailbox.sentCount(), 0);
    }

    function test_AnOrderOfNothingOrForItsOwnAssetIsRefused() public {
        stable.mint(alice, 100 * USDC);
        vm.startPrank(alice);
        stable.approve(address(vault), 100 * USDC);
        vm.expectRevert(Portal.SameAsset.selector);
        vault.deposit(100 * USDC, toStable(alice), 0, type(uint64).max);
        vm.expectRevert(abi.encodeWithSelector(Wire.UnknownDestination.selector, 9));
        vault.deposit(
            100 * USDC, Wire.Destination(9, 0, bytes32(0), address(0)), 0, type(uint64).max
        );
        vm.stopPrank();
        assertEq(stable.balanceOf(alice), 100 * USDC);
    }

    function test_AnAmountBeyondTheHubsRangeIsRefused() public {
        uint256 tooMuch = uint256(type(uint64).max) / 1000 + 1;
        stable.mint(alice, tooMuch);
        vm.startPrank(alice);
        stable.approve(address(vault), tooMuch);
        vm.expectRevert(
            abi.encodeWithSelector(
                SafeCast.SafeCastOverflowedUintDowncast.selector, 64, tooMuch * 1000
            )
        );
        vault.deposit(tooMuch, toTao(alice), 0, type(uint64).max);
        vm.stopPrank();
    }

    function test_ThePayerPaysTheTransportAndGetsTheChange() public {
        mailbox.setFee(0.01 ether);
        assertEq(portal.orderFee(), 0.01 ether);
        stable.mint(alice, 100 * USDC);
        vm.deal(alice, 1 ether);
        vm.startPrank(alice);
        stable.approve(address(vault), 100 * USDC);

        vm.expectRevert(abi.encodeWithSelector(Portal.FeeNotPaid.selector, 0.01 ether, 0.009 ether));
        vault.deposit{value: 0.009 ether}(50 * USDC, toTao(alice), 0, type(uint64).max);

        vault.deposit{value: 0.3 ether}(50 * USDC, toTao(alice), 0, type(uint64).max);
        vm.stopPrank();
        assertEq(alice.balance, 0.99 ether);
        assertEq(address(mailbox).balance, 0.01 ether);
        assertEq(address(portal).balance, 0);
        assertEq(address(vault).balance, 0);
    }

    function test_AnOrderPaysForItsAnswerAndTheFeesGoToTheirRecipient() public {
        mailbox.setFee(0.01 ether);
        timelocked(address(portal), abi.encodeCall(portal.setAnswerFee, (0.002 ether, bob)));
        assertEq(portal.orderFee(), 0.012 ether);
        stable.mint(alice, 100 * USDC);
        vm.deal(alice, 1 ether);
        vm.startPrank(alice);
        stable.approve(address(vault), 100 * USDC);

        // The transport's fee alone is not enough.
        vm.expectRevert(abi.encodeWithSelector(Portal.FeeNotPaid.selector, 0.012 ether, 0.01 ether));
        vault.deposit{value: 0.01 ether}(50 * USDC, toTao(alice), 0, type(uint64).max);

        vault.deposit{value: 0.3 ether}(50 * USDC, toTao(alice), 0, type(uint64).max);
        vault.deposit{value: 0.012 ether}(50 * USDC, toTao(alice), 0, type(uint64).max);
        vm.stopPrank();

        // The transport got its fee, the payer the change, and the answer fees wait.
        assertEq(alice.balance, 1 ether - 2 * 0.012 ether);
        assertEq(address(mailbox).balance, 0.02 ether);
        assertEq(address(portal).balance, 0.004 ether);

        // Anyone sends them on, and only to the recipient.
        vm.expectEmit(address(portal));
        emit FeesSwept(bob, 0.004 ether);
        vm.prank(stranger);
        portal.sweepFees();
        assertEq(bob.balance, 0.004 ether);
        assertEq(address(portal).balance, 0);
    }

    function test_AnAnswerFeeNeedsARecipient() public {
        // No recipient was set: nothing can be swept, and no fee can be set without one.
        vm.expectRevert(Portal.NoFeeRecipient.selector);
        portal.sweepFees();
        vm.prank(address(timelock));
        vm.expectRevert(Portal.NoFeeRecipient.selector);
        portal.setAnswerFee(1, address(0));

        vm.prank(stranger);
        vm.expectRevert();
        portal.setAnswerFee(1, stranger);
    }

    function test_AnOrderForTheZeroAddressIsRefused() public {
        stable.mint(alice, 100 * USDC);
        vm.startPrank(alice);
        stable.approve(address(vault), 100 * USDC);
        vm.expectRevert(Wire.NoRecipient.selector);
        vault.deposit(100 * USDC, toTao(address(0)), 0, type(uint64).max);
        vm.stopPrank();
        assertEq(stable.balanceOf(alice), 100 * USDC);
    }

    function test_AContractThatPaysGetsItsChangeToo() public {
        mailbox.setFee(0.01 ether);
        PayingContract payer = new PayingContract();
        vm.deal(address(payer), 1 ether);
        credit(Wire.ASSET_TAO, 0, address(payer), 5 * UNIT, 0);
        vm.prank(address(payer));
        tao.sell{value: 0.05 ether}(uint64(5 * UNIT), toStable(address(payer)), 0, type(uint64).max);
        assertEq(payer.change(), 0.04 ether);
        assertEq(address(portal).balance, 0);
    }

    // ---------------------------------------------------------------- the hub's answers

    function test_ACreditOfTheStablecoinIsReleasedFromTheVault() public {
        deposit(alice, 250 * USDC, toTao(alice));

        vm.expectEmit(address(portal));
        emit Credited(0, bob, Wire.ASSET_STABLE, 0, uint64(99 * UNIT + 999), true, 0, true);
        mailbox.deliver(
            portal,
            HUB_DOMAIN,
            GATEWAY,
            creditBody(0, Wire.ASSET_STABLE, 0, bob, uint64(99 * UNIT + 999), true, 0, true, 0)
        );

        // The hub's 9 decimals are cut to the stablecoin's 6. The dust stays in the vault.
        assertEq(stable.balanceOf(bob), 99 * USDC);
        assertEq(stable.balanceOf(address(vault)), 151 * USDC);
        assertTrue(portal.processed(0));
    }

    function test_ACreditOfATokenIsMinted() public {
        AlphaToken chutes = registerToken(CHUTES, "Chutes", "CHUTES");
        credit(Wire.ASSET_TAO, 0, alice, 7 * UNIT, 0);
        credit(Wire.ASSET_ALPHA, CHUTES, bob, 20 * UNIT, 1.25e18);
        assertEq(tao.balanceOf(alice), 7 * UNIT);
        assertEq(chutes.balanceOf(bob), 20 * UNIT);
        assertEq(chutes.rate(), 1.25e18);
    }

    function test_TheHubCreatesAnAlphaTokenWhereThePortalPutsIt() public {
        address expected = portal.alphaTokenAddress(CHUTES);
        assertEq(expected.code.length, 0);

        vm.expectEmit(address(portal));
        emit TokenCreated(CHUTES, expected, "Chutes", "CHUTES");
        AlphaToken chutes = registerToken(CHUTES, "Chutes", "CHUTES");

        assertEq(address(chutes), expected);
        assertEq(address(chutes.portal()), address(portal));
        // Each subnet has its own token.
        AlphaToken lium = registerToken(51, "lium.io", "LIUM");
        assertTrue(address(lium) != address(chutes));
        assertEq(address(lium), portal.alphaTokenAddress(51));
        assertEq(lium.symbol(), "LIUM");
    }

    function test_AWrongListingInTheHubIsReportedAndDoesNotStopTheToken() public {
        address expected = portal.alphaTokenAddress(CHUTES);
        vm.expectEmit(address(portal));
        emit TokenCreated(CHUTES, expected, "Chutes", "CHUTES");
        vm.expectEmit(address(portal));
        emit RegistryMismatch(CHUTES, address(0xBAD), expected);
        mailbox.deliver(
            portal,
            HUB_DOMAIN,
            GATEWAY,
            abi.encode(
                Wire.KIND_REGISTER_TOKEN, hubNonce++, CHUTES, address(0xBAD), "Chutes", "CHUTES"
            )
        );
        credit(Wire.ASSET_ALPHA, CHUTES, alice, UNIT, 1e18);
        assertEq(portal.alphaTokens(CHUTES).balanceOf(alice), UNIT);
    }

    function test_ATokenIsCreatedOnce() public {
        registerToken(CHUTES, "Chutes", "CHUTES");
        vm.expectRevert(abi.encodeWithSelector(Portal.TokenExists.selector, CHUTES));
        mailbox.deliver(
            portal,
            HUB_DOMAIN,
            GATEWAY,
            abi.encode(Wire.KIND_REGISTER_TOKEN, hubNonce, CHUTES, address(0), "Again", "AGAIN")
        );
    }

    function test_ACreditForATokenThatIsNotThereYetWaits() public {
        bytes memory body =
            creditBody(1, Wire.ASSET_ALPHA, CHUTES, alice, UNIT, false, 0, false, 1e18);
        // The transport delivered the credit before the token's registration.
        vm.expectRevert(abi.encodeWithSelector(Portal.NoSuchToken.selector, CHUTES));
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertFalse(portal.processed(1));

        registerToken(CHUTES, "Chutes", "CHUTES");
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertEq(portal.alphaTokens(CHUTES).balanceOf(alice), UNIT);
    }

    function test_OnlyTheTransportDeliversAndOnlyFromTheHub() public {
        bytes memory body = creditBody(0, Wire.ASSET_TAO, 0, alice, UNIT, false, 0, false, 0);
        for (uint256 i = 0; i < 4; ++i) {
            address caller = [alice, guardian, multisig, address(timelock)][i];
            vm.prank(caller);
            vm.expectRevert(Portal.OnlyMailbox.selector);
            portal.handle(HUB_DOMAIN, GATEWAY, body);
        }
        // The right transport, another chain or another sender.
        vm.expectRevert(abi.encodeWithSelector(Portal.NotTheHub.selector, 1, GATEWAY));
        mailbox.deliver(portal, 1, GATEWAY, body);
        bytes32 other = bytes32(uint256(0xBAD));
        vm.expectRevert(abi.encodeWithSelector(Portal.NotTheHub.selector, HUB_DOMAIN, other));
        mailbox.deliver(portal, HUB_DOMAIN, other, body);
        assertEq(tao.totalSupply(), 0);
    }

    function test_AMessageOfTheHubIsCarriedOutOnce() public {
        bytes memory body = creditBody(4, Wire.ASSET_TAO, 0, alice, UNIT, false, 0, false, 0);
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        vm.expectRevert(abi.encodeWithSelector(Portal.AlreadyProcessed.selector, 4));
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertEq(tao.balanceOf(alice), UNIT);
        // Messages need not arrive in order.
        mailbox.deliver(
            portal,
            HUB_DOMAIN,
            GATEWAY,
            creditBody(2, Wire.ASSET_TAO, 0, alice, UNIT, false, 0, false, 0)
        );
        assertEq(tao.balanceOf(alice), 2 * UNIT);
    }

    function test_AMessageThePortalDoesNotKnowIsRefused() public {
        vm.expectRevert(abi.encodeWithSelector(Portal.UnknownMessage.selector, 4));
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, abi.encode(uint8(4), uint64(0)));
        vm.expectRevert(abi.encodeWithSelector(Portal.UnknownAsset.selector, 3));
        mailbox.deliver(
            portal, HUB_DOMAIN, GATEWAY, creditBody(0, 3, 0, alice, UNIT, false, 0, false, 0)
        );
        vm.expectRevert();
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, hex"");
    }

    function test_APaymentBeyondTheVaultsAllowanceIsPaidInParts() public {
        deposit(alice, 3_000_000 * USDC, toTao(alice));
        bytes memory body =
            creditBody(0, Wire.ASSET_STABLE, 0, bob, uint64(2_500_000 * UNIT), false, 0, false, 0);

        // The vault releases 1,000,000 a day. The hub's answer is carried out once: what
        // fits is paid now, and the rest is on record.
        vm.expectEmit(address(portal));
        emit StablePaid(bob, 1_000_000 * USDC, 1_500_000 * USDC);
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertTrue(portal.processed(0));
        assertEq(stable.balanceOf(bob), 1_000_000 * USDC);
        assertEq(portal.owed(bob), 1_500_000 * USDC);
        assertEq(portal.totalOwed(), 1_500_000 * USDC);

        // Nothing more fits now. Anyone asks again later, and only bob is paid.
        vm.prank(stranger);
        assertEq(portal.claim(bob), 0);
        skip(12 hours);
        vm.prank(stranger);
        assertEq(portal.claim(bob), 500_000 * USDC);
        skip(1 days);
        assertEq(portal.claim(bob), 1_000_000 * USDC);
        assertEq(stable.balanceOf(bob), 2_500_000 * USDC);
        assertEq(portal.owed(bob), 0);
        assertEq(portal.totalOwed(), 0);
        // Nothing is owed: nothing is paid.
        skip(1 days);
        assertEq(portal.claim(bob), 0);
        assertEq(stable.balanceOf(address(vault)), 500_000 * USDC);
    }

    /// Whatever the hub tells the portal to pay, and whenever somebody asks again, the
    /// payee has received exactly what the vault could release, what is paid and what is
    /// owed add up to the payment, and in the end the payee has all of it.
    function testFuzz_APaymentInPartsAddsUpToThePayment(uint256 amount, uint32[6] calldata waits)
        public
    {
        amount = bound(amount, 1, 5_000_000 * USDC);
        deposit(alice, 5_000_000 * USDC, toTao(alice));
        credit(Wire.ASSET_STABLE, 0, bob, SafeCast.toUint64(amount * 1000), 0);
        uint256 paid = stable.balanceOf(bob);
        assertEq(paid, amount < 1_000_000 * USDC ? amount : 1_000_000 * USDC);
        assertEq(paid + portal.owed(bob), amount);

        for (uint256 i = 0; i < waits.length; ++i) {
            skip(bound(waits[i], 0, 2 days));
            uint256 owed = portal.owed(bob);
            uint256 available = vault.releasable();
            vm.prank(stranger);
            uint256 part = portal.claim(bob);
            assertEq(part, owed < available ? owed : available);
            paid += part;
            assertEq(stable.balanceOf(bob), paid);
            assertEq(paid + portal.owed(bob), amount);
            assertEq(portal.totalOwed(), portal.owed(bob));
        }

        // The allowance refills in full in a day: five days pay the largest payment.
        for (uint256 i = 0; i < 5; ++i) {
            skip(1 days);
            portal.claim(bob);
        }
        assertEq(portal.owed(bob), 0);
        assertEq(stable.balanceOf(bob), amount);
        assertEq(stable.balanceOf(address(vault)), 5_000_000 * USDC - amount);
    }

    function test_AStoppedVaultOwesWhatTheHubToldItToPay() public {
        deposit(alice, 250 * USDC, toTao(alice));
        vm.prank(guardian);
        vault.pause();

        // The answer is not lost and is not tried again and again: it is on record.
        credit(Wire.ASSET_STABLE, 0, bob, 100 * UNIT, 0);
        assertEq(stable.balanceOf(bob), 0);
        assertEq(portal.owed(bob), 100 * USDC);
        assertEq(portal.claim(bob), 0);

        timelocked(address(vault), abi.encodeCall(vault.unpause, ()));
        assertEq(portal.claim(bob), 100 * USDC);
        assertEq(stable.balanceOf(bob), 100 * USDC);
    }

    function test_ARecipientTheStablecoinBlocksHoldsNobodyElse() public {
        deposit(alice, 250 * USDC, toTao(alice));
        stable.setBlocked(bob, true);
        bytes memory toBob = creditBody(
            hubNonce++, Wire.ASSET_STABLE, 0, bob, uint64(100 * UNIT), false, 0, false, 0
        );

        // The stablecoin refuses the transfer. The answer is not carried out: it stays
        // with the transport, and it used none of the vault's allowance.
        vm.expectRevert(MockStable.Blocked.selector);
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, toBob);
        assertFalse(portal.processed(0));
        assertEq(portal.owed(bob), 0);
        assertEq(vault.releasable(), 1_000_000 * USDC);
        // Answers are carried out in any order: alice is paid as if nothing had happened.
        credit(Wire.ASSET_STABLE, 0, alice, 50 * UNIT, 0);
        assertEq(stable.balanceOf(alice), 50 * USDC);

        // When the stablecoin lets bob receive again, the transport delivers his answer.
        stable.setBlocked(bob, false);
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, toBob);
        assertEq(stable.balanceOf(bob), 100 * USDC);
        assertEq(portal.totalOwed(), 0);
    }

    function test_AClaimForABlockedAddressFails() public {
        deposit(alice, 3_000_000 * USDC, toTao(alice));
        credit(Wire.ASSET_STABLE, 0, bob, 1_500_000 * UNIT, 0);
        assertEq(portal.owed(bob), 500_000 * USDC);
        skip(1 days);

        // Nothing is hidden from the caller: the claim fails, and the debt stays.
        stable.setBlocked(bob, true);
        vm.expectRevert(MockStable.Blocked.selector);
        portal.claim(bob);
        assertEq(portal.owed(bob), 500_000 * USDC);

        stable.setBlocked(bob, false);
        assertEq(portal.claim(bob), 500_000 * USDC);
    }

    // ---------------------------------------------------------------- pause

    function test_APausedPortalTakesNoOrderAndNoMessage() public {
        credit(Wire.ASSET_TAO, 0, alice, 5 * UNIT, 0);
        vm.prank(guardian);
        portal.pause();

        stable.mint(alice, 100 * USDC);
        vm.startPrank(alice);
        stable.approve(address(vault), 100 * USDC);
        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        vault.deposit(100 * USDC, toTao(alice), 0, type(uint64).max);
        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        tao.sell(uint64(UNIT), toStable(alice), 0, type(uint64).max);
        vm.stopPrank();
        // Nothing was taken.
        assertEq(stable.balanceOf(alice), 100 * USDC);
        assertEq(tao.balanceOf(alice), 5 * UNIT);

        // A message of the hub stays with the transport, which delivers it later.
        bytes memory body = creditBody(hubNonce, Wire.ASSET_TAO, 0, bob, UNIT, false, 0, false, 0);
        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        timelocked(address(portal), abi.encodeCall(portal.unpause, ()));
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertEq(tao.balanceOf(bob), UNIT);
    }

    function test_APausedPortalPaysNoDebt() public {
        deposit(alice, 3_000_000 * USDC, toTao(alice));
        credit(Wire.ASSET_STABLE, 0, bob, 1_500_000 * UNIT, 0);
        skip(1 days);
        vm.prank(guardian);
        portal.pause();

        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        portal.claim(bob);
        assertEq(portal.owed(bob), 500_000 * USDC);

        timelocked(address(portal), abi.encodeCall(portal.unpause, ()));
        assertEq(portal.claim(bob), 500_000 * USDC);
    }

    /// Whatever a payer deposits, the order carries that amount in 9 decimals, the vault
    /// holds it, and the portal holds nothing.
    function testFuzz_ADepositIsAnOrderForExactlyWhatWasLocked(uint256 amount, uint8 kind) public {
        amount = bound(amount, 1, 10_000_000_000 * USDC);
        kind = uint8(bound(kind, 1, 5));
        Wire.Destination memory destination = kind <= Wire.TO_ALPHA
            ? Wire.Destination(
                kind, kind == Wire.TO_ALPHA ? CHUTES : 0, bytes32(uint256(uint160(bob))), address(0)
            )
            : Wire.Destination(
                kind,
                kind == Wire.TO_CREDITS ? CHUTES : 0,
                keccak256("account"),
                kind == Wire.TO_CREDITS ? bob : address(0)
            );
        deposit(alice, amount, destination);
        Wire.Order memory order = lastOrder();
        assertEq(uint256(order.amount), amount * 1000);
        assertEq(order.destination, kind);
        assertEq(stable.balanceOf(address(vault)), amount);
        assertEq(stable.balanceOf(address(portal)), 0);
    }
}

/// @notice A portal that the hub gave two order slots.
contract PortalSlotsTest is Fixture {
    event OrderSlotsGranted(uint64 indexed nonce, uint32 slots);

    function initialSlots() internal pure override returns (uint32) {
        return 2;
    }

    function test_EachOrderUsesASlot() public {
        assertEq(portal.orderSlots(), 2);
        deposit(alice, 100 * USDC, toTao(alice));
        assertEq(portal.orderSlots(), 1);
        deposit(alice, 100 * USDC, toCredits(51, keccak256("service"), alice));
        assertEq(portal.orderSlots(), 0);
        assertEq(portal.nextOrder(), 2);
    }

    function test_WithoutASlotThePortalTakesNoOrderAndThePayerKeepsTheFunds() public {
        // Alice holds canonical TAO, and the portal's two slots are used.
        mailbox.deliver(
            portal, HUB_DOMAIN, GATEWAY, creditBody(0, Wire.ASSET_TAO, alice, uint64(5 * UNIT), 0)
        );
        deposit(bob, 100 * USDC, toTao(bob));
        deposit(bob, 100 * USDC, toTao(bob));
        assertEq(portal.orderSlots(), 0);
        uint256 locked = stable.balanceOf(address(vault));

        // The hub has not promised to answer another order. A deposit takes nothing.
        stable.mint(alice, 100 * USDC);
        vm.startPrank(alice);
        stable.approve(address(vault), 100 * USDC);
        vm.expectRevert(Portal.NoOrderSlot.selector);
        vault.deposit(100 * USDC, toTao(alice), 0, type(uint64).max);

        // A sale burns nothing.
        vm.expectRevert(Portal.NoOrderSlot.selector);
        tao.sell(uint64(UNIT), toStable(alice), 0, type(uint64).max);
        vm.stopPrank();

        assertEq(stable.balanceOf(alice), 100 * USDC);
        assertEq(stable.balanceOf(address(vault)), locked);
        assertEq(tao.balanceOf(alice), 5 * UNIT);
        assertEq(portal.nextOrder(), 2);
        assertEq(mailbox.sentCount(), 2);
    }

    function test_TheHubsPaymentsBringSlots() public {
        deposit(bob, 100 * USDC, toTao(bob));
        deposit(bob, 100 * USDC, toTao(bob));

        // The answer to the first order brings one slot, and the portal takes an order.
        vm.expectEmit(address(portal));
        emit OrderSlotsGranted(0, 1);
        mailbox.deliver(
            portal, HUB_DOMAIN, GATEWAY, creditBody(0, Wire.ASSET_TAO, bob, uint64(UNIT), 1)
        );
        assertEq(portal.orderSlots(), 1);
        deposit(bob, 100 * USDC, toTao(bob));
        assertEq(portal.orderSlots(), 0);

        // A payment can bring more than one, and one that brings none changes nothing.
        mailbox.deliver(
            portal, HUB_DOMAIN, GATEWAY, creditBody(1, Wire.ASSET_TAO, bob, uint64(UNIT), 3)
        );
        mailbox.deliver(
            portal, HUB_DOMAIN, GATEWAY, creditBody(2, Wire.ASSET_TAO, bob, uint64(UNIT), 0)
        );
        assertEq(portal.orderSlots(), 3);
    }

    function test_AMessageOfSlotsAloneBringsThemOnce() public {
        bytes memory body = slotsBody(7, 5);
        vm.expectEmit(address(portal));
        emit OrderSlotsGranted(7, 5);
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertEq(portal.orderSlots(), 7);

        // The transport cannot bring the same slots again.
        vm.expectRevert(abi.encodeWithSelector(Portal.AlreadyProcessed.selector, 7));
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        // A payment under the same number is refused too, with its slots.
        vm.expectRevert(abi.encodeWithSelector(Portal.AlreadyProcessed.selector, 7));
        mailbox.deliver(
            portal, HUB_DOMAIN, GATEWAY, creditBody(7, Wire.ASSET_TAO, bob, uint64(UNIT), 9)
        );
        assertEq(portal.orderSlots(), 7);
    }

    function test_OnlyTheHubGivesSlots() public {
        bytes memory body = slotsBody(0, 5);
        vm.prank(alice);
        vm.expectRevert(Portal.OnlyMailbox.selector);
        portal.handle(HUB_DOMAIN, GATEWAY, body);

        bytes32 other = bytes32(uint256(0xBAD));
        vm.expectRevert(abi.encodeWithSelector(Portal.NotTheHub.selector, HUB_DOMAIN, other));
        mailbox.deliver(portal, HUB_DOMAIN, other, body);
        assertEq(portal.orderSlots(), 2);
    }

    function test_APaymentThatCannotBeMadeYetBringsItsSlotsWhenItIsMade() public {
        // A payment of an alpha token arrives before the token exists. The portal refuses
        // it whole, slots and all, and the transport delivers it again.
        bytes memory body = abi.encode(
            Wire.KIND_CREDIT,
            uint64(1),
            Wire.ASSET_ALPHA,
            CHUTES,
            alice,
            uint64(UNIT),
            false,
            uint64(0),
            false,
            uint128(1e18),
            uint32(4)
        );
        vm.expectRevert(abi.encodeWithSelector(Portal.NoSuchToken.selector, CHUTES));
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertEq(portal.orderSlots(), 2);

        registerToken(CHUTES, "Chutes", "CHUTES");
        mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
        assertEq(portal.orderSlots(), 6);
    }
}
