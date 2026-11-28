// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IMailbox} from "../../src/interfaces/IMailbox.sol";
import {IMessageRecipient} from "../../src/interfaces/IMessageRecipient.sol";

/// @notice A transport for tests: it keeps what was dispatched and delivers what a test
/// tells it to.
contract MockMailbox is IMailbox {
    struct Message {
        uint32 destination;
        bytes32 recipient;
        bytes body;
        uint256 paid;
    }

    uint32 public immutable localDomain;
    /// @notice What `dispatch` charges.
    uint256 public fee;
    /// @notice Destinations that `dispatch` refuses.
    mapping(uint32 destination => bool) public blocked;
    Message[] public sent;

    constructor(uint32 localDomain_) {
        localDomain = localDomain_;
    }

    function setFee(uint256 fee_) external {
        fee = fee_;
    }

    function setBlocked(uint32 destination, bool blocked_) external {
        blocked[destination] = blocked_;
    }

    function sentCount() external view returns (uint256) {
        return sent.length;
    }

    function lastBody() external view returns (bytes memory) {
        return sent[sent.length - 1].body;
    }

    function dispatch(uint32 destination, bytes32 recipient, bytes calldata body)
        external
        payable
        returns (bytes32)
    {
        require(msg.value == fee, "MockMailbox: wrong fee");
        require(!blocked[destination], "MockMailbox: destination blocked");
        sent.push(Message(destination, recipient, body, msg.value));
        return keccak256(abi.encode(sent.length, destination, recipient, body));
    }

    function quoteDispatch(uint32, bytes32, bytes calldata) external view returns (uint256) {
        return fee;
    }

    /// @notice Delivers `body` to `recipient` as a message from `sender` on `origin`.
    function deliver(
        IMessageRecipient recipient,
        uint32 origin,
        bytes32 sender,
        bytes calldata body
    ) external {
        recipient.handle(origin, sender, body);
    }
}
