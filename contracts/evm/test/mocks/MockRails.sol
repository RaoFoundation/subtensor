// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IRails} from "precompiles/rails.sol";

/// @notice The hub's precompile for tests of the gateway: it records what it is asked and
/// answers what a test set. It keeps the hub's rules for who pays the transport: each
/// bridge has its own TAO, a reserve and a ceiling, a message is queued only if that TAO
/// covers the reserve for every message that has not left, and the caller of `ackOutbound`
/// is handed what it pays the transport, up to the reserve and what the bridge's TAO can
/// spare above the reserves of the waiting messages, and never more than the ceiling. The
/// head of a queue can be deferred, and leaves later.
contract MockRails is IRails {
    struct Outbound {
        uint32 domain;
        bytes32 recipient;
        bytes body;
    }

    struct Executed {
        address caller;
        uint32 domain;
        bytes32 sender;
        bytes order;
    }

    Executed[] public executed;
    /// @notice The queue of each bridge, and how far the gateway has taken it.
    mapping(uint32 bridge => Outbound[]) public outbox;
    mapping(uint32 bridge => uint64) public head;
    /// @notice The deferred messages: before the head, and not yet out of the hub.
    mapping(uint32 bridge => mapping(uint64 nonce => bool)) public deferred;
    mapping(uint32 bridge => uint256) public deferredCount;
    /// @notice What the account of each bridge holds for its messages, in wei. This
    /// contract holds it, as the hub's accounts do.
    mapping(uint32 bridge => uint256) public budget;
    /// @notice What the hub sets aside for one message to each bridge's spoke, in wei.
    mapping(uint32 bridge => uint256) public reserve;
    /// @notice The most the hub pays for one message to each bridge's spoke, in wei.
    mapping(uint32 bridge => uint256) public ceiling;
    uint8 public status = 1;
    uint64 public amountOut;
    bool public refuse;
    /// @notice An answer queued by the next `execute`, as the hub queues a credit.
    Outbound private _answer;
    bool private _hasAnswer;

    function setResult(uint8 status_, uint64 amountOut_) external {
        status = status_;
        amountOut = amountOut_;
    }

    function setRefuse(bool refuse_) external {
        refuse = refuse_;
    }

    function setAnswer(uint32 domain, bytes32 recipient, bytes calldata body) external {
        _answer = Outbound(domain, recipient, body);
        _hasAnswer = true;
    }

    /// @notice Pays into the account of `bridge`, as `Rails.fund_messages` does.
    function fund(uint32 bridge) external payable {
        budget[bridge] += msg.value;
    }

    /// @notice Sets the reserve and the ceiling of `bridge`, as
    /// `Rails.set_message_reserve` does.
    function setReserve(uint32 bridge, uint256 reserve_, uint256 ceiling_) external {
        require(ceiling_ >= reserve_, "dispatch execution failed: CeilingBelowReserve");
        require(
            budget[bridge] >= reserve_ * _queued(bridge),
            "dispatch execution failed: MessagesNotFunded"
        );
        reserve[bridge] = reserve_;
        ceiling[bridge] = ceiling_;
    }

    /// @notice The most the hub pays for one message of `bridge` now, as
    /// `Rails.message_limit` computes it.
    function limit(uint32 bridge) public view returns (uint256) {
        uint256 spare = budget[bridge] - reserve[bridge] * _queued(bridge);
        uint256 pays = reserve[bridge] + spare;
        return pays < ceiling[bridge] ? pays : ceiling[bridge];
    }

    /// @notice The messages of `bridge` that have not left: in line, or deferred.
    function waiting(uint32 bridge) external view returns (uint256) {
        return _queued(bridge);
    }

    /// @notice Queues a message for `bridge`. In these tests a bridge's id is its domain.
    function enqueue(uint32 bridge, bytes32 recipient, bytes calldata body) external {
        _enqueue(bridge, Outbound(bridge, recipient, body));
    }

    function _queued(uint32 bridge) private view returns (uint256) {
        return outbox[bridge].length - head[bridge] + deferredCount[bridge];
    }

    /// The gateway can take the head of the queue and the deferred messages.
    function _canLeave(uint32 bridge, uint64 nonce) private view returns (bool) {
        return nonce < outbox[bridge].length && (nonce == head[bridge] || deferred[bridge][nonce]);
    }

    /// The hub queues a message only if the bridge's account holds the reserve for every
    /// message in its queue and for this one.
    function _enqueue(uint32 bridge, Outbound memory message) private {
        require(
            budget[bridge] >= reserve[bridge] * (_queued(bridge) + 1),
            "dispatch execution failed: MessagesNotFunded"
        );
        outbox[bridge].push(message);
    }

    function executedCount() external view returns (uint256) {
        return executed.length;
    }

    function execute(uint32 domain, bytes32 sender, bytes calldata order)
        external
        returns (uint8, uint64, uint32)
    {
        require(!refuse, "dispatch execution failed: OrderOutOfSequence");
        executed.push(Executed(msg.sender, domain, sender, order));
        if (_hasAnswer) {
            _enqueue(domain, _answer);
            _hasAnswer = false;
        }
        return (status, amountOut, domain);
    }

    function ackOutbound(uint32 bridge, uint64 nonce, uint256 paid) external {
        require(_canLeave(bridge, nonce), "dispatch execution failed: NotOutboxHead");
        require(paid <= limit(bridge), "dispatch execution failed: FeeAboveLimit");
        budget[bridge] -= paid;
        if (deferred[bridge][nonce]) {
            deferred[bridge][nonce] = false;
            deferredCount[bridge]--;
        } else {
            head[bridge]++;
        }
        // The hub moves what the caller pays the transport from the bridge's account to
        // the caller.
        (bool handed,) = msg.sender.call{value: paid}("");
        require(handed, "MockRails: the caller takes no TAO");
    }

    function deferOutbound(uint32 bridge, uint64 nonce) external {
        require(
            nonce == head[bridge] && nonce < outbox[bridge].length,
            "dispatch execution failed: NotOutboxHead"
        );
        head[bridge]++;
        deferred[bridge][nonce] = true;
        deferredCount[bridge]++;
    }

    function outbound(uint32 bridge, uint64 nonce)
        external
        view
        returns (bool pending, uint32 domain, bytes32 recipient, bytes memory body, uint256 limit_)
    {
        if (!_canLeave(bridge, nonce)) return (false, 0, bytes32(0), "", 0);
        Outbound storage message = outbox[bridge][nonce];
        return (true, message.domain, message.recipient, message.body, limit(bridge));
    }

    function nextOutbound(uint32 bridge)
        external
        view
        returns (
            bool pending,
            uint64 nonce,
            uint32 domain,
            bytes32 recipient,
            bytes memory body,
            uint256 limit_
        )
    {
        nonce = head[bridge];
        if (nonce >= outbox[bridge].length) return (false, 0, 0, bytes32(0), "", 0);
        Outbound storage next = outbox[bridge][nonce];
        return (true, nonce, next.domain, next.recipient, next.body, limit(bridge));
    }

    function quote(uint32, bytes calldata) external pure returns (bool, uint64, string memory) {
        return (true, 0, "");
    }

    function receipt(uint32, uint64) external pure returns (uint8, uint64, uint32) {
        return (0, 0, 0);
    }

    function nextOrder(uint32) external pure returns (uint64) {
        return 0;
    }

    function alphaRate(uint16) external pure returns (uint256) {
        return 1e18;
    }

    function creditsOf(uint16, bytes32, address) external pure returns (uint64) {
        return 0;
    }
}
