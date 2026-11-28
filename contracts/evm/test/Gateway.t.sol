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
    event MessageHeld(uint32 indexed bridge, uint64 indexed nonce, uint256 fee, uint256 reserve);

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
        (bool pending,,,,,) = rails.nextOutbound(BASE);
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

    /// On the hub, the account of `bridge` holds `reserve` for each of `messages` messages,
    /// and the hub pays no more than the reserve for one.
    function fundBridge(uint32 bridge, uint256 reserve, uint256 messages) internal {
        rails.fund{value: reserve * messages}(bridge);
        rails.setReserve(bridge, reserve, reserve);
    }

    function test_AMessageIsPaidForFromTheAccountOfItsBridge() public {
        // The hub pays up to 0.5 TAO for a message. Base's account holds that for one
        // message, Ethereum's for three.
        fundBridge(BASE, 0.5 ether, 1);
        fundBridge(ETHEREUM, 0.5 ether, 3);
        mailbox.setFee(0.4 ether);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        for (uint64 i = 0; i < 3; ++i) {
            rails.enqueue(ETHEREUM, PORTAL, abi.encode(Wire.KIND_CREDIT, i));
        }
        // The gateway holds nothing.
        assertEq(address(gateway).balance, 0);

        // Ethereum's messages leave. The hub hands the gateway what the transport charges
        // for each, from Ethereum's account, and the gateway pays it on.
        vm.prank(stranger);
        assertEq(gateway.flush(ETHEREUM, 3), 3);
        assertEq(address(mailbox).balance, 1.2 ether);
        assertEq(rails.budget(ETHEREUM), 0.3 ether);
        assertEq(rails.budget(BASE), 0.5 ether);
        assertEq(address(gateway).balance, 0);

        // Base's message leaves on Base's account.
        assertEq(gateway.flush(BASE, 1), 1);
        assertEq(rails.budget(BASE), 0.1 ether);
        assertEq(rails.budget(ETHEREUM), 0.3 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_AChargeAboveTheReserveWaitsForSomeoneWhoPaysTheDifference() public {
        fundBridge(BASE, 0.2 ether, 3);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(1)));
        // The transport charges more than the hub pays for one message.
        mailbox.setFee(0.5 ether);

        // Nobody pays the difference: the message waits, and nothing is spent.
        vm.expectEmit(address(gateway));
        emit MessageHeld(BASE, 0, 0.5 ether, 0.2 ether);
        assertEq(gateway.flush(BASE, 2), 0);
        assertEq(rails.head(BASE), 0);
        assertEq(rails.budget(BASE), 0.6 ether);

        // An order of the bridge is not held up: the hub settles it, and its answer waits
        // behind the others.
        rails.setAnswer(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(2)));
        vm.expectEmit(address(gateway));
        emit MessageHeld(BASE, 0, 0.5 ether, 0.2 ether);
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 1);
        assertEq(mailbox.sentCount(), 0);

        // Anyone pays the difference for the first message, 0.3 TAO, and gets the rest of
        // what was sent back. The second message waits.
        vm.deal(stranger, 1 ether);
        vm.prank(stranger);
        assertEq(gateway.flush{value: 0.4 ether}(BASE, 3), 1);
        assertEq(stranger.balance, 0.7 ether);
        assertEq(rails.budget(BASE), 0.4 ether);
        assertEq(address(mailbox).balance, 0.5 ether);
        assertEq(address(gateway).balance, 0);

        // The charge falls below the reserve again: the rest leaves, and the hub pays.
        mailbox.setFee(0.1 ether);
        assertEq(gateway.flush(BASE, 3), 2);
        assertEq(rails.budget(BASE), 0.2 ether);
        assertEq(address(mailbox).balance, 0.7 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_AQueueThatAHigherChargeHoldsUpDrainsFromWhatTheAccountCanSpare() public {
        // The hub sets 0.2 TAO aside for a message to Base and pays up to 0.6 for one.
        rails.fund{value: 0.6 ether}(BASE);
        rails.setReserve(BASE, 0.2 ether, 0.6 ether);
        mailbox.setFee(0.1 ether);
        for (uint64 i = 0; i < 3; ++i) {
            rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, i));
        }

        // The transport's charge rises above the reserve. Base's account holds the three
        // reserves and nothing to spare: the queue is held up.
        mailbox.setFee(0.5 ether);
        vm.expectEmit(address(gateway));
        emit MessageHeld(BASE, 0, 0.5 ether, 0.2 ether);
        assertEq(gateway.flush(BASE, 3), 0);

        // Anyone pays into Base's account on the hub. Nobody pays the gateway. The whole
        // queue leaves: each message uses its reserve, and the hub adds the rest from
        // what the account can spare.
        rails.fund{value: 0.9 ether}(BASE);
        vm.prank(stranger);
        assertEq(gateway.flush(BASE, 3), 3);
        assertEq(address(mailbox).balance, 1.5 ether);
        assertEq(rails.budget(BASE), 0);
        assertEq(address(gateway).balance, 0);

        // An order of the bridge now settles and its answer leaves with it, though the
        // charge is still above the reserve.
        rails.fund{value: 0.5 ether}(BASE);
        rails.setAnswer(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(3)));
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(mailbox.sentCount(), 4);
        assertEq(rails.budget(BASE), 0);
    }

    function test_AChargeAboveTheCeilingIsNotTheHubsToPay() public {
        // The hub pays at most 0.3 TAO for a message, and Base's account can spare much
        // more.
        rails.fund{value: 5 ether}(BASE);
        rails.setReserve(BASE, 0.2 ether, 0.3 ether);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        mailbox.setFee(0.5 ether);

        vm.expectEmit(address(gateway));
        emit MessageHeld(BASE, 0, 0.5 ether, 0.3 ether);
        assertEq(gateway.flush(BASE, 1), 0);
        assertEq(rails.budget(BASE), 5 ether);

        // A caller pays what is above the ceiling: 0.2 TAO. The hub pays its 0.3.
        vm.deal(stranger, 1 ether);
        vm.prank(stranger);
        assertEq(gateway.flush{value: 1 ether}(BASE, 1), 1);
        assertEq(stranger.balance, 0.8 ether);
        assertEq(rails.budget(BASE), 4.7 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_ACallerGetsBackWhatWasNotNeeded() public {
        fundBridge(BASE, 0.2 ether, 1);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        mailbox.setFee(0.1 ether);

        // The hub pays the whole charge. What the caller sent comes back.
        vm.deal(stranger, 1 ether);
        vm.prank(stranger);
        assertEq(gateway.flush{value: 1 ether}(BASE, 1), 1);
        assertEq(stranger.balance, 1 ether);
        assertEq(address(gateway).balance, 0);

        // A caller that cannot take its change gets nothing done. This test contract
        // takes no TAO.
        rails.fund{value: 0.2 ether}(BASE);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(1)));
        vm.expectRevert();
        gateway.flush{value: 1 ether}(BASE, 1);
        assertEq(rails.head(BASE), 1);
    }

    function test_AnOrderWaitsUntilItsBridgeHoldsTheReserveForItsAnswer() public {
        // The hub pays for messages to Base, and nobody paid into Base's account.
        rails.setReserve(BASE, 1 ether, 1 ether);
        mailbox.setFee(0.6 ether);
        rails.setAnswer(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));

        // The hub does not take the order: the transport delivers it again later.
        vm.expectRevert("dispatch execution failed: MessagesNotFunded");
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 0);
        assertEq(mailbox.sentCount(), 0);

        // Once Base's account holds the reserve, the order settles and its answer leaves
        // with it. What the transport did not charge stays in Base's account.
        rails.fund{value: 1 ether}(BASE);
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 1);
        assertEq(mailbox.sentCount(), 1);
        assertEq(address(mailbox).balance, 0.6 ether);
        assertEq(rails.budget(BASE), 0.4 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_TaoSentToTheGatewayPaysForNoMessage() public {
        fundBridge(BASE, 0.2 ether, 1);
        rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, uint64(0)));
        mailbox.setFee(0.5 ether);
        // Someone sends the gateway TAO. It is not the hub's, and the gateway does not
        // spend it: the message still waits for the difference.
        vm.deal(address(gateway), 5 ether);
        assertEq(gateway.flush(BASE, 1), 0);
        assertEq(address(gateway).balance, 5 ether);

        mailbox.setFee(0.2 ether);
        assertEq(gateway.flush(BASE, 1), 1);
        assertEq(address(gateway).balance, 5 ether);
    }

    /// Whatever the transport charges, whatever the hub's account for the bridge can
    /// spare and whatever a caller sends along: the gateway ends with what it began with,
    /// every TAO that reaches the transport comes from the hub's account or from the
    /// caller, the hub's account still holds the reserve for each message that waits, and
    /// the hub never pays more than the ceiling for a message.
    function testFuzz_TheGatewayPaysOnExactlyWhatItIsHanded(
        uint64 reserve,
        uint64 above,
        uint64 spare,
        uint64 fee,
        uint64 offered,
        uint8 messages
    ) public {
        messages = uint8(bound(messages, 1, 6));
        uint256 ceiling = uint256(reserve) + above;
        rails.fund{value: uint256(reserve) * messages + spare}(BASE);
        rails.setReserve(BASE, reserve, ceiling);
        for (uint64 i = 0; i < messages; ++i) {
            rails.enqueue(BASE, PORTAL, abi.encode(Wire.KIND_CREDIT, i));
        }
        mailbox.setFee(fee);
        uint256 budget = rails.budget(BASE);
        vm.deal(stranger, offered);

        vm.prank(stranger);
        uint256 sent = gateway.flush{value: offered}(BASE, messages);

        uint256 fromHub = budget - rails.budget(BASE);
        uint256 fromCaller = offered - stranger.balance;
        assertEq(rails.head(BASE), sent);
        assertEq(address(mailbox).balance, sent * uint256(fee));
        assertEq(fromHub + fromCaller, sent * uint256(fee));
        assertEq(address(gateway).balance, 0);
        assertLe(fromHub, sent * ceiling);
        assertGe(rails.budget(BASE), uint256(reserve) * (messages - sent));
        // A charge within the reserve needs nothing from the caller, and all messages go.
        if (fee <= reserve) {
            assertEq(sent, messages);
            assertEq(fromCaller, 0);
        }
        // The hand-over stops only where the next message needs more than the hub pays
        // and the caller has left.
        if (sent < messages) {
            assertGt(uint256(fee) - rails.limit(BASE), offered - fromCaller);
        }
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
