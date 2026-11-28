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
        uint64 amountOut,
        uint32 bridge
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
    event MessageDeferred(uint32 indexed bridge, uint64 indexed nonce, uint256 fee, uint256 limit);

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
                        abi.encodeCall(Gateway.initialize, (address(timelock), guardian, mailbox))
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
        emit OrderExecuted(BASE, PORTAL, 7, 1, 42e9, BASE);
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
        emit OrderExecuted(BASE, PORTAL, 5, 1, 99e9, BASE);
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

    function credit(uint64 nonce) internal pure returns (bytes memory) {
        return abi.encode(Wire.KIND_CREDIT, nonce);
    }

    function test_AnOrderSettlesAndItsAnswerWaitsForAFlush() public {
        bytes memory answer = credit(0);
        rails.setAnswer(BASE, PORTAL, answer);

        // The order hands nothing to the transport: the hub's answer waits in the queue
        // of the order's bridge.
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 1);
        assertEq(mailbox.sentCount(), 0);
        (bool pending,,,,,) = rails.nextOutbound(BASE);
        assertTrue(pending);

        // Anyone has the gateway hand it over.
        vm.expectEmit(true, true, false, false, address(gateway));
        emit MessageSent(BASE, 0, BASE, PORTAL, bytes32(0));
        vm.prank(stranger);
        assertEq(gateway.flush(BASE, 1), 1);

        assertEq(mailbox.sentCount(), 1);
        (uint32 destination, bytes32 recipient, bytes memory body,) = mailbox.sent(0);
        assertEq(destination, BASE);
        assertEq(recipient, PORTAL);
        assertEq(body, answer);
        (pending,,,,,) = rails.nextOutbound(BASE);
        assertFalse(pending);
    }

    function test_AnOrderSettlesWhateverTheTransportDoesOnTheWayOut() public {
        // A message waits for Base. The transport refuses messages to Base, and charges
        // more for one than anyone pays.
        fundBridge(BASE, 0.2 ether, 2);
        rails.enqueue(BASE, PORTAL, credit(0));
        mailbox.setBlocked(BASE, true);
        mailbox.setFee(50 ether);
        vm.expectRevert("MockMailbox: destination blocked");
        gateway.flush{value: 50 ether}(BASE, 1);

        // Orders of Base still settle, one after the other.
        rails.setAnswer(BASE, PORTAL, credit(1));
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        mailbox.deliver(gateway, BASE, PORTAL, order(1, Wire.TO_NATIVE_TAO));
        assertEq(rails.executedCount(), 2);
        assertEq(mailbox.sentCount(), 0);

        // The answers leave when the transport takes messages again.
        mailbox.setBlocked(BASE, false);
        mailbox.setFee(0.1 ether);
        assertEq(gateway.flush(BASE, 5), 2);
    }

    function test_MessagesLeaveInOrderAndAtMostTheLimitAtOnce() public {
        for (uint64 i = 0; i < 6; ++i) {
            rails.enqueue(BASE, PORTAL, credit(i));
        }
        assertEq(gateway.flush(BASE, 0), 0);
        assertEq(gateway.flush(BASE, 4), 4);
        assertEq(rails.head(BASE), 4);

        // Anyone hands over the rest.
        vm.prank(stranger);
        assertEq(gateway.flush(BASE, 10), 2);
        assertEq(mailbox.sentCount(), 6);
        for (uint64 i = 0; i < 6; ++i) {
            (,, bytes memory body,) = mailbox.sent(i);
            assertEq(body, credit(i));
        }
        assertEq(gateway.flush(BASE, 10), 0);
    }

    function test_AFlushHandsOverOnlyTheQueueOfItsBridge() public {
        rails.enqueue(ETHEREUM, PORTAL, credit(0));
        rails.enqueue(BASE, PORTAL, credit(7));

        assertEq(gateway.flush(BASE, 10), 1);

        // Base's message left. Ethereum's waits for a flush of Ethereum.
        assertEq(mailbox.sentCount(), 1);
        (uint32 destination,,,) = mailbox.sent(0);
        assertEq(destination, BASE);
        assertEq(rails.head(ETHEREUM), 0);
        assertEq(gateway.flush(ETHEREUM, 10), 1);
    }

    function test_AMessageTheTransportRefusesHoldsOnlyItsOwnBridge() public {
        rails.enqueue(BASE, PORTAL, credit(0));
        rails.enqueue(ETHEREUM, PORTAL, credit(0));
        // The transport refuses messages to Base. Ethereum's leave.
        mailbox.setBlocked(BASE, true);

        // The message is not deferred: it is paid for. It stays at the head.
        vm.expectRevert("MockMailbox: destination blocked");
        gateway.flush(BASE, 1);
        vm.expectRevert("MockMailbox: destination blocked");
        gateway.send(BASE, 0);
        assertEq(gateway.flush(ETHEREUM, 1), 1);
        assertEq(rails.head(BASE), 0);
        assertEq(rails.deferredCount(BASE), 0);
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
        rails.enqueue(BASE, PORTAL, credit(0));
        for (uint64 i = 0; i < 3; ++i) {
            rails.enqueue(ETHEREUM, PORTAL, credit(i));
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

    function test_AMessageThatIsNotPaidForIsDeferredAndTheOnesBehindItLeave() public {
        // The hub pays up to 0.2 TAO for a message. The transport charges by the size of
        // the message: the first one is long, and costs more than that.
        fundBridge(BASE, 0.2 ether, 3);
        mailbox.setFee(0.1 ether);
        mailbox.setFeePerByte(0.001 ether);
        bytes memory long = abi.encode(Wire.KIND_REGISTER_TOKEN, uint64(0), new bytes(320));
        rails.enqueue(BASE, PORTAL, long);
        rails.enqueue(BASE, PORTAL, credit(1));
        rails.enqueue(BASE, PORTAL, credit(2));
        uint256 dear = mailbox.quoteDispatch(BASE, PORTAL, long);
        uint256 cheap = mailbox.quoteDispatch(BASE, PORTAL, credit(1));
        assertGt(dear, 0.2 ether);
        assertLt(cheap, 0.2 ether);

        // Nobody pays the difference for the first message: it is deferred, and the two
        // behind it leave in the same call.
        vm.expectEmit(address(gateway));
        emit MessageDeferred(BASE, 0, dear, 0.2 ether);
        vm.prank(stranger);
        assertEq(gateway.flush(BASE, 3), 2);
        assertEq(mailbox.sentCount(), 2);
        for (uint64 i = 0; i < 2; ++i) {
            (,, bytes memory body,) = mailbox.sent(i);
            assertEq(body, credit(i + 1));
        }
        (bool pending,,,,,) = rails.nextOutbound(BASE);
        assertFalse(pending);

        // The first message is still the hub's to send, and its reserve is still there.
        assertTrue(rails.deferred(BASE, 0));
        assertEq(rails.waiting(BASE), 1);
        assertEq(rails.budget(BASE), 0.6 ether - 2 * cheap);
        assertGe(rails.budget(BASE), 0.2 ether);
        assertEq(address(gateway).balance, 0);

        // It does not leave for less than the transport charges.
        vm.expectRevert(abi.encodeWithSelector(Gateway.ChargeNotCovered.selector, dear, 0.2 ether));
        gateway.send(BASE, 0);

        // Anyone pays the difference, and gets the rest of what was sent back.
        vm.deal(stranger, 1 ether);
        vm.expectEmit(true, true, false, false, address(gateway));
        emit MessageSent(BASE, 0, BASE, PORTAL, bytes32(0));
        vm.prank(stranger);
        gateway.send{value: 1 ether}(BASE, 0);
        assertEq(stranger.balance, 1 ether - (dear - 0.2 ether));
        assertEq(mailbox.lastBody(), long);
        assertEq(rails.waiting(BASE), 0);
        assertEq(address(gateway).balance, 0);

        // It leaves once.
        vm.expectRevert(abi.encodeWithSelector(Gateway.NoSuchMessage.selector, BASE, 0));
        gateway.send(BASE, 0);
    }

    function test_ADeferredMessageLeavesWhenTheHubPaysMore() public {
        fundBridge(BASE, 0.2 ether, 2);
        rails.enqueue(BASE, PORTAL, credit(0));
        rails.enqueue(BASE, PORTAL, credit(1));
        mailbox.setFee(0.5 ether);

        // The transport charges more than the hub pays for one message: both are deferred.
        assertEq(gateway.flush(BASE, 5), 0);
        assertEq(rails.deferredCount(BASE), 2);
        assertEq(rails.budget(BASE), 0.4 ether);

        // The hub's admin raises the ceiling, and someone pays into Base's account on the
        // hub. Nobody pays the gateway. Each message uses its reserve, and the hub adds
        // the rest from what the account can spare. They leave in any order.
        rails.setReserve(BASE, 0.2 ether, 0.6 ether);
        rails.fund{value: 0.6 ether}(BASE);
        vm.startPrank(stranger);
        gateway.send(BASE, 1);
        gateway.send(BASE, 0);
        vm.stopPrank();
        assertEq(mailbox.sentCount(), 2);
        assertEq(mailbox.lastBody(), credit(0));
        assertEq(rails.budget(BASE), 0);
        assertEq(rails.waiting(BASE), 0);
        assertEq(address(gateway).balance, 0);
    }

    function test_OnlyTheHeadAndDeferredMessagesCanBeSent() public {
        fundBridge(BASE, 0.2 ether, 3);
        for (uint64 i = 0; i < 3; ++i) {
            rails.enqueue(BASE, PORTAL, credit(i));
        }
        // A message behind the head waits its turn.
        vm.expectRevert(abi.encodeWithSelector(Gateway.NoSuchMessage.selector, BASE, 1));
        gateway.send(BASE, 1);
        vm.expectRevert(abi.encodeWithSelector(Gateway.NoSuchMessage.selector, BASE, 3));
        gateway.send(BASE, 3);

        // The head leaves by its nonce, as with a flush.
        gateway.send(BASE, 0);
        assertEq(rails.head(BASE), 1);
        assertEq(mailbox.lastBody(), credit(0));
    }

    function test_ACallerOfAFlushPaysTheDifference() public {
        fundBridge(BASE, 0.2 ether, 3);
        for (uint64 i = 0; i < 3; ++i) {
            rails.enqueue(BASE, PORTAL, credit(i));
        }
        mailbox.setFee(0.5 ether);

        // The caller sends enough for the difference of one message, 0.3 TAO, and a
        // little more. The first message leaves, the two behind it are deferred, and the
        // rest of what was sent comes back.
        vm.deal(stranger, 1 ether);
        vm.prank(stranger);
        assertEq(gateway.flush{value: 0.4 ether}(BASE, 3), 1);
        assertEq(stranger.balance, 0.7 ether);
        assertEq(rails.budget(BASE), 0.4 ether);
        assertEq(rails.deferredCount(BASE), 2);
        assertEq(address(mailbox).balance, 0.5 ether);
        assertEq(address(gateway).balance, 0);

        // The charge falls below the reserve again: the rest leaves, and the hub pays.
        mailbox.setFee(0.1 ether);
        gateway.send(BASE, 1);
        gateway.send(BASE, 2);
        assertEq(rails.budget(BASE), 0.2 ether);
        assertEq(address(mailbox).balance, 0.7 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_AChargeAboveTheCeilingIsNotTheHubsToPay() public {
        // The hub pays at most 0.3 TAO for a message, and Base's account can spare much
        // more.
        rails.fund{value: 5 ether}(BASE);
        rails.setReserve(BASE, 0.2 ether, 0.3 ether);
        rails.enqueue(BASE, PORTAL, credit(0));
        mailbox.setFee(0.5 ether);

        vm.expectEmit(address(gateway));
        emit MessageDeferred(BASE, 0, 0.5 ether, 0.3 ether);
        assertEq(gateway.flush(BASE, 1), 0);
        assertEq(rails.budget(BASE), 5 ether);

        // A caller pays what is above the ceiling: 0.2 TAO. The hub pays its 0.3.
        vm.deal(stranger, 1 ether);
        vm.prank(stranger);
        gateway.send{value: 1 ether}(BASE, 0);
        assertEq(stranger.balance, 0.8 ether);
        assertEq(rails.budget(BASE), 4.7 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_ACallerGetsBackWhatWasNotNeeded() public {
        fundBridge(BASE, 0.2 ether, 1);
        rails.enqueue(BASE, PORTAL, credit(0));
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
        rails.enqueue(BASE, PORTAL, credit(1));
        vm.expectRevert();
        gateway.flush{value: 1 ether}(BASE, 1);
        vm.expectRevert();
        gateway.send{value: 1 ether}(BASE, 1);
        assertEq(rails.head(BASE), 1);
    }

    function test_ACallerThatComesBackInWithItsChangeSpendsOnlyItsOwn() public {
        fundBridge(BASE, 0.2 ether, 2);
        rails.enqueue(BASE, PORTAL, credit(0));
        rails.enqueue(BASE, PORTAL, credit(1));
        mailbox.setFee(0.5 ether);
        ReenteringCaller caller = new ReenteringCaller(gateway);
        vm.deal(address(caller), 0.4 ether);

        // The caller pays the difference for the first message. When its change arrives
        // it calls `flush` again, with nothing: the second message is not paid for, and
        // is deferred.
        assertEq(caller.flush(BASE, 1, 0.4 ether), 1);

        assertEq(caller.reentered(), 1);
        assertEq(address(caller).balance, 0.1 ether);
        assertEq(mailbox.sentCount(), 1);
        assertEq(address(mailbox).balance, 0.5 ether);
        assertTrue(rails.deferred(BASE, 1));
        assertEq(rails.budget(BASE), 0.2 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_AnOrderWaitsUntilItsBridgeHoldsTheReserveForItsAnswer() public {
        // The hub pays for messages to Base, and nobody paid into Base's account.
        rails.setReserve(BASE, 1 ether, 1 ether);
        mailbox.setFee(0.6 ether);
        rails.setAnswer(BASE, PORTAL, credit(0));

        // The hub does not take the order: the transport delivers it again later.
        vm.expectRevert("dispatch execution failed: MessagesNotFunded");
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 0);

        // Once Base's account holds the reserve, the order settles. Its answer leaves
        // with a flush, and what the transport did not charge stays in Base's account.
        rails.fund{value: 1 ether}(BASE);
        mailbox.deliver(gateway, BASE, PORTAL, order(0, Wire.TO_TAO));
        assertEq(rails.executedCount(), 1);
        assertEq(gateway.flush(BASE, 1), 1);
        assertEq(address(mailbox).balance, 0.6 ether);
        assertEq(rails.budget(BASE), 0.4 ether);
        assertEq(address(gateway).balance, 0);
    }

    function test_TaoSentToTheGatewayPaysForNoMessage() public {
        fundBridge(BASE, 0.2 ether, 1);
        rails.enqueue(BASE, PORTAL, credit(0));
        mailbox.setFee(0.5 ether);
        // Someone sends the gateway TAO. It is not the hub's, and the gateway does not
        // spend it: the message is deferred, and still waits for the difference.
        vm.deal(address(gateway), 5 ether);
        assertEq(gateway.flush(BASE, 1), 0);
        vm.expectRevert(
            abi.encodeWithSelector(Gateway.ChargeNotCovered.selector, 0.5 ether, 0.2 ether)
        );
        gateway.send(BASE, 0);
        assertEq(address(gateway).balance, 5 ether);

        mailbox.setFee(0.2 ether);
        gateway.send(BASE, 0);
        assertEq(mailbox.sentCount(), 1);
        assertEq(address(gateway).balance, 5 ether);
    }

    /// Whatever the transport charges, whatever the hub's account for the bridge can
    /// spare and whatever a caller sends along: every message leaves or is deferred, the
    /// gateway ends with what it began with, every TAO that reaches the transport comes
    /// from the hub's account or from the caller, the hub's account still holds the
    /// reserve for each message that has not left, and the hub never pays more than the
    /// ceiling for a message. A deferred message leaves when someone pays the
    /// difference.
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
            rails.enqueue(BASE, PORTAL, credit(i));
        }
        mailbox.setFee(fee);
        uint256 budget = rails.budget(BASE);
        vm.deal(stranger, offered);

        vm.prank(stranger);
        uint256 sent = gateway.flush{value: offered}(BASE, messages);

        uint256 fromHub = budget - rails.budget(BASE);
        uint256 fromCaller = offered - stranger.balance;
        uint256 deferred = rails.deferredCount(BASE);
        // Every message was taken from the line: it left, or it was deferred.
        assertEq(rails.head(BASE), messages);
        assertEq(sent + deferred, messages);
        assertEq(mailbox.sentCount(), sent);
        assertEq(address(mailbox).balance, sent * uint256(fee));
        assertEq(fromHub + fromCaller, sent * uint256(fee));
        assertEq(address(gateway).balance, 0);
        assertLe(fromHub, sent * ceiling);
        assertGe(rails.budget(BASE), uint256(reserve) * deferred);
        // A charge within the reserve needs nothing from the caller, and all messages go.
        if (fee <= reserve) {
            assertEq(sent, messages);
            assertEq(fromCaller, 0);
        }
        // A message is deferred only if its charge is more than the hub pays and the
        // caller has left.
        if (deferred > 0) {
            assertGt(uint256(fee) - rails.limit(BASE), stranger.balance);
        }

        // Someone pays the difference for each deferred message, and it leaves.
        for (uint64 nonce = 0; nonce < messages; ++nonce) {
            if (!rails.deferred(BASE, nonce)) continue;
            uint256 difference = uint256(fee) - rails.limit(BASE);
            vm.deal(stranger, difference);
            vm.prank(stranger);
            gateway.send{value: difference}(BASE, nonce);
            assertEq(stranger.balance, 0);
        }
        assertEq(rails.waiting(BASE), 0);
        assertEq(mailbox.sentCount(), messages);
        assertEq(address(mailbox).balance, uint256(messages) * fee);
        assertEq(address(gateway).balance, 0);
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
        vm.expectRevert(PausableUpgradeable.EnforcedPause.selector);
        gateway.send(BASE, 0);

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
            gateway.upgradeToAndCall(address(1), "");
            vm.stopPrank();
        }
        timelocked(abi.encodeCall(gateway.setInterchainSecurityModule, (address(0x15))));
        assertEq(gateway.interchainSecurityModule(), address(0x15));
        timelocked(abi.encodeCall(gateway.setGuardian, (stranger)));
        assertEq(gateway.guardian(), stranger);
    }
}

/// @notice A caller that calls `flush` again, with nothing, when the gateway sends it its
/// change.
contract ReenteringCaller {
    Gateway private immutable GATEWAY;
    uint32 private _bridge;
    uint256 public reentered;

    constructor(Gateway gateway) {
        GATEWAY = gateway;
    }

    function flush(uint32 bridge, uint256 limit, uint256 value) external returns (uint256) {
        _bridge = bridge;
        return GATEWAY.flush{value: value}(bridge, limit);
    }

    receive() external payable {
        ++reentered;
        GATEWAY.flush(_bridge, 10);
    }
}
