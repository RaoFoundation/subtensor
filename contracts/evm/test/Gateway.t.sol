// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {
    OwnableUpgradeable
} from "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import {
    PausableUpgradeable
} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {Test} from "forge-std/Test.sol";
import {IRAILS_ADDRESS} from "precompiles/rails.sol";

import {Gateway} from "../src/Gateway.sol";
import {Wire} from "../src/Wire.sol";
import {MockMailbox} from "./mocks/MockMailbox.sol";
import {MockRails} from "./mocks/MockRails.sol";

contract GatewayTest is Test {
    uint32 internal constant BASE = 8453;
    uint32 internal constant ETHEREUM = 1;
    bytes32 internal constant PORTAL = bytes32(uint256(0xB0));
    uint256 internal constant TIMELOCK_DELAY = 48 hours;

    address internal multisig = makeAddr("multisig");
    address internal guardian = makeAddr("guardian");
    address internal stranger = makeAddr("stranger");

    MockMailbox internal mailbox;
    MockRails internal rails = MockRails(IRAILS_ADDRESS);
    TimelockController internal timelock;
    Gateway internal gateway;

    event OrderExecuted(
        uint32 indexed origin,
        bytes32 indexed portal,
        uint64 indexed nonce,
        uint8 status,
        uint64 amountOut
    );
    event CreditsPurchased(
        address indexed beneficiary,
        uint16 indexed netuid,
        bytes32 indexed service,
        uint32 origin,
        uint64 nonce,
        uint64 gamma
    );
    event MessageSent(
        uint32 indexed bridge,
        uint64 indexed nonce,
        uint32 domain,
        bytes32 recipient,
        bytes32 messageId
    );

    function setUp() public {
        vm.warp(1_800_000_000);
        // The hub's precompile, at its address.
        vm.etch(IRAILS_ADDRESS, address(new MockRails()).code);
        rails.setResult(1, 0);
        mailbox = new MockMailbox(964);
        address[] memory holders = new address[](1);
        holders[0] = multisig;
        timelock = new TimelockController(TIMELOCK_DELAY, holders, holders, address(0));
        gateway = Gateway(
            payable(address(
                    new ERC1967Proxy(
                        address(new Gateway()),
                        abi.encodeCall(
                            Gateway.initialize, (address(timelock), guardian, mailbox, 4)
                        )
                    )
                ))
        );
    }

    function order(uint64 nonce, uint8 destination) internal pure returns (bytes memory) {
        return Wire.encode(
            Wire.Order({
                version: Wire.ORDER_VERSION,
                nonce: nonce,
                payer: address(0xAA),
                source: Wire.ASSET_STABLE,
                sourceNetuid: 0,
                amount: 100e9,
                destination: destination,
                destinationNetuid: destination == Wire.TO_CREDITS ? 51 : 0,
                recipient: keccak256("service"),
                beneficiary: destination == Wire.TO_CREDITS ? address(0xBB) : address(0),
                minOut: 0,
                deadline: type(uint64).max
            })
        );
    }

    function timelocked(bytes memory data) internal {
        vm.startPrank(multisig);
        timelock.schedule(address(gateway), 0, data, bytes32(0), bytes32(0), TIMELOCK_DELAY);
        skip(TIMELOCK_DELAY);
        timelock.execute(address(gateway), 0, data, bytes32(0), bytes32(0));
        vm.stopPrank();
    }

    function test_AnOrderIsPassedToTheHubUnchanged() public {
        bytes memory body = order(7, Wire.TO_NATIVE_TAO);
        rails.setResult(1, 42e9);

        vm.expectEmit(address(gateway));
        emit OrderExecuted(BASE, PORTAL, 7, 1, 42e9);
        mailbox.deliver(gateway, BASE, PORTAL, body);

        assertEq(rails.executedCount(), 1);
        (address caller, uint32 domain, bytes32 sender, bytes memory passed) = rails.executed(0);
        // The hub sees the gateway as the caller, and the transport's own word on where
        // the message came from.
        assertEq(caller, address(gateway));
        assertEq(domain, BASE);
        assertEq(sender, PORTAL);
        assertEq(passed, body);
    }

    function test_OnlyTheTransportDeliversOrders() public {
        bytes memory body = order(0, Wire.TO_NATIVE_TAO);
        for (uint256 i = 0; i < 4; ++i) {
            address caller = [stranger, guardian, multisig, address(timelock)][i];
            vm.prank(caller);
            vm.expectRevert(Gateway.OnlyMailbox.selector);
            gateway.handle(BASE, PORTAL, body);
        }
        assertEq(rails.executedCount(), 0);
    }

    function test_AnOrderTheHubDoesNotTakeStaysWithTheTransport() public {
        rails.setRefuse(true);
        vm.expectRevert("dispatch execution failed: OrderOutOfSequence");
        mailbox.deliver(gateway, BASE, PORTAL, order(3, Wire.TO_NATIVE_TAO));
        // The transport delivers it again when the hub is ready.
        rails.setRefuse(false);
        mailbox.deliver(gateway, BASE, PORTAL, order(3, Wire.TO_NATIVE_TAO));
        assertEq(rails.executedCount(), 1);
    }

    function test_CreditsAreLoggedForTheAddressTheyAreFor() public {
        rails.setResult(1, 99e9);
        vm.expectEmit(address(gateway));
        emit OrderExecuted(BASE, PORTAL, 5, 1, 99e9);
        vm.expectEmit(address(gateway));
        emit CreditsPurchased(address(0xBB), 51, keccak256("service"), BASE, 5, 99e9);
        mailbox.deliver(gateway, BASE, PORTAL, order(5, Wire.TO_CREDITS));
    }

    function test_RefundedCreditsAreNotLoggedAsBought() public {
        rails.setResult(2, 0);
        vm.recordLogs();
        mailbox.deliver(gateway, BASE, PORTAL, order(5, Wire.TO_CREDITS));
        assertEq(vm.getRecordedLogs().length, 1, "only OrderExecuted");
    }

    function test_TheHubsAnswerLeavesWithTheOrderThatCausedIt() public {
        bytes memory answer = abi.encode(Wire.KIND_CREDIT, uint64(0));
        rails.setAnswer(BASE, PORTAL, answer);

        vm.expectEmit(true, true, false, false, address(gateway));
        emit MessageSent(BASE, 0, BASE, PORTAL, bytes32(0));
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));

        assertEq(mailbox.sentCount(), 1);
        (uint32 destination, bytes32 recipient, bytes memory body,) = mailbox.sent(0);
        assertEq(destination, BASE);
        assertEq(recipient, PORTAL);
        assertEq(body, answer);
        (bool pending,,,,) = rails.nextOutbound(BASE);
        assertFalse(pending);
    }

    function test_AnswersLeaveInOrderAndAtMostTheLimitAtOnce() public {
        for (uint256 i = 0; i < 6; ++i) {
            rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(i)));
        }
        // An order hands over four, the limit.
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_NATIVE_TAO));
        assertEq(mailbox.sentCount(), 4);
        assertEq(rails.head(BASE), 4);

        // Anyone hands over the rest.
        vm.prank(stranger);
        assertEq(gateway.flush(BASE, 10), 2);
        assertEq(mailbox.sentCount(), 6);
        for (uint256 i = 0; i < 6; ++i) {
            (,, bytes memory body,) = mailbox.sent(i);
            assertEq(body, abi.encode(Wire.KIND_CREDIT, uint64(i)));
        }
        assertEq(gateway.flush(BASE, 10), 0);
    }

    function test_AnOrderHandsOverOnlyTheQueueOfItsOwnBridge() public {
        rails.enqueue(ETHEREUM, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        rails.setAnswer(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(7)));

        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));

        // Base's answer left. Ethereum's message waits for an order of Ethereum, or a flush.
        assertEq(mailbox.sentCount(), 1);
        (uint32 destination,,,) = mailbox.sent(0);
        assertEq(destination, BASE);
        assertEq(rails.head(ETHEREUM), 0);
        assertEq(gateway.flush(ETHEREUM, 10), 1);
    }

    function test_AMessageThatCannotLeaveHoldsOnlyItsOwnBridge() public {
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        rails.enqueue(ETHEREUM, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        // The transport refuses messages to Base. Ethereum's leave.
        mailbox.setBlocked(BASE, true);

        vm.expectRevert("MockMailbox: destination blocked");
        gateway.flush(BASE, 1);
        assertEq(gateway.flush(ETHEREUM, 1), 1);
        assertEq(rails.head(BASE), 0);
        assertEq(rails.head(ETHEREUM), 1);

        mailbox.setBlocked(BASE, false);
        assertEq(gateway.flush(BASE, 1), 1);
    }

    function test_ABridgesMessagesArePaidForFromItsBudgetAlone() public {
        // On the hub, 1 TAO was paid in for Ethereum's messages and nothing for Base's.
        // The gateway holds that TAO.
        rails.fund(ETHEREUM, 1 ether);
        vm.deal(address(gateway), 1 ether);
        mailbox.setFee(0.4 ether);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        for (uint64 i = 0; i < 3; ++i) {
            rails.enqueue(ETHEREUM, PORTAL, abi.encode(Wire.KIND_CREDIT, i));
        }

        // Base's message does not leave on Ethereum's TAO, though the gateway holds enough.
        vm.expectRevert("dispatch execution failed: MessagesNotFunded");
        gateway.flush(BASE, 1);
        assertEq(mailbox.sentCount(), 0);
        assertEq(address(gateway).balance, 1 ether);

        // Ethereum's budget pays for two of its three messages. A hand-over of all three
        // fails as a whole. The two that are paid for leave.
        vm.expectRevert("dispatch execution failed: MessagesNotFunded");
        gateway.flush(ETHEREUM, 3);
        assertEq(rails.head(ETHEREUM), 0);
        assertEq(gateway.flush(ETHEREUM, 2), 2);
        assertEq(rails.budget(ETHEREUM), 0.2 ether);
        assertEq(address(gateway).balance, 0.2 ether);
        assertEq(address(mailbox).balance, 0.8 ether);

        // Someone pays in for Base on the hub: its message leaves, and Ethereum's budget
        // is as it was.
        rails.fund(BASE, 0.4 ether);
        vm.deal(address(gateway), 0.6 ether);
        assertEq(gateway.flush(BASE, 1), 1);
        assertEq(rails.budget(BASE), 0);
        assertEq(rails.budget(ETHEREUM), 0.2 ether);
        assertEq(address(gateway).balance, 0.2 ether);
    }

    function test_ADearerDestinationSpendsOnlyItsOwnBudget() public {
        // The transport charges ten times as much for Ethereum as for Base.
        mailbox.setFee(0.1 ether);
        mailbox.setExtraFee(ETHEREUM, 0.9 ether);
        rails.fund(BASE, 0.2 ether);
        rails.fund(ETHEREUM, 0.5 ether);
        vm.deal(address(gateway), 0.7 ether);
        rails.enqueue(ETHEREUM, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(1)));

        // Ethereum's message costs more than Ethereum's budget: it waits. Base's leave.
        vm.expectRevert("dispatch execution failed: MessagesNotFunded");
        gateway.flush(ETHEREUM, 1);
        assertEq(gateway.flush(BASE, 2), 2);
        assertEq(address(gateway).balance, 0.5 ether);
        assertEq(rails.budget(ETHEREUM), 0.5 ether);
    }

    function test_AnOrderWaitsWhileItsAnswerCannotLeave() public {
        // The transport charges, and nothing was paid in for Base.
        mailbox.setFee(1 ether);
        rails.setAnswer(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));

        // The order and its answer go through together or not at all: the hub has not
        // taken the order, and the transport delivers it again later.
        vm.expectRevert("dispatch execution failed: MessagesNotFunded");
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 0);
        assertEq(mailbox.sentCount(), 0);

        // Once Base's budget is paid in, the order settles and its answer leaves with it.
        rails.fund(BASE, 3 ether);
        vm.deal(address(gateway), 3 ether);
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 1);
        assertEq(mailbox.sentCount(), 1);
        assertEq(address(gateway).balance, 2 ether);
        assertEq(address(mailbox).balance, 1 ether);
        assertEq(rails.budget(BASE), 2 ether);
    }

    function test_AMessageIsNotTakenFromTheQueueIfTheTransportIsNotPaid() public {
        // The hub's budget covers the fee, but the gateway does not hold the TAO. That
        // cannot happen while the gateway spends only what it tells the hub. If it does,
        // the message stays in the queue and the budget is not charged.
        mailbox.setFee(1 ether);
        rails.fund(BASE, 1 ether);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));

        vm.expectRevert();
        gateway.flush(BASE, 1);
        assertEq(rails.head(BASE), 0);
        assertEq(rails.budget(BASE), 1 ether);
    }

    function test_TheGuardianPausesAndOnlyTheOwnerResumes() public {
        vm.prank(stranger);
        vm.expectRevert(Gateway.OnlyGuardianOrOwner.selector);
        gateway.pause();
        vm.prank(guardian);
        gateway.pause();

        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_NATIVE_TAO));
        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        gateway.flush(BASE, 1);

        vm.prank(guardian);
        vm.expectRevert(
            abi.encodeWithSelector(OwnableUpgradeable.OwnableUnauthorizedAccount.selector, guardian)
        );
        gateway.unpause();
        timelocked(abi.encodeCall(gateway.unpause, ()));
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_NATIVE_TAO));
    }

    function test_OnlyTheOwnerConfigures() public {
        for (uint256 i = 0; i < 3; ++i) {
            address caller = [stranger, guardian, multisig][i];
            vm.startPrank(caller);
            bytes memory unauthorized = abi.encodeWithSelector(
                OwnableUpgradeable.OwnableUnauthorizedAccount.selector, caller
            );
            vm.expectRevert(unauthorized);
            gateway.setInterchainSecurityModule(caller);
            vm.expectRevert(unauthorized);
            gateway.setGuardian(caller);
            vm.expectRevert(unauthorized);
            gateway.setFlushLimit(0);
            vm.expectRevert(unauthorized);
            gateway.upgradeToAndCall(address(1), "");
            vm.stopPrank();
        }
        timelocked(abi.encodeCall(gateway.setInterchainSecurityModule, (address(0x15))));
        assertEq(gateway.interchainSecurityModule(), address(0x15));
        timelocked(abi.encodeCall(gateway.setFlushLimit, (9)));
        assertEq(gateway.flushLimit(), 9);
    }
}
