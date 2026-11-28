// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IRails} from "precompiles/rails.sol";

/// @notice The hub's precompile for tests of the gateway: it records what it is asked and
/// answers what a test set.
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
    /// @notice What is left of what was paid in for each bridge's messages, in wei.
    mapping(uint32 bridge => uint256) public budget;
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

    /// @notice Pays `amount` into the budget of `bridge`, as `Rails.fund_messages` does.
    /// The test gives the gateway the TAO.
    function fund(uint32 bridge, uint256 amount) external {
        budget[bridge] += amount;
    }

    /// @notice Queues a message for `bridge`. In these tests a bridge's id is its domain.
    function enqueue(uint32 bridge, bytes32 recipient, bytes calldata body) external {
        outbox[bridge].push(Outbound(bridge, recipient, body));
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
            outbox[domain].push(_answer);
            _hasAnswer = false;
        }
        return (status, amountOut, domain);
    }

    function ackOutbound(uint32 bridge, uint64 nonce, uint256 paid) external {
        require(
            nonce == head[bridge] && nonce < outbox[bridge].length,
            "dispatch execution failed: NotOutboxHead"
        );
        require(paid <= budget[bridge], "dispatch execution failed: MessagesNotFunded");
        budget[bridge] -= paid;
        head[bridge]++;
    }

    function nextOutbound(uint32 bridge)
        external
        view
        returns (bool pending, uint64 nonce, uint32 domain, bytes32 recipient, bytes memory body)
    {
        nonce = head[bridge];
        if (nonce >= outbox[bridge].length) return (false, 0, 0, bytes32(0), "");
        Outbound storage next = outbox[bridge][nonce];
        return (true, nonce, next.domain, next.recipient, next.body);
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
