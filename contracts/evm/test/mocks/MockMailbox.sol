// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {IMailbox} from "../../src/interfaces/IMailbox.sol";
import {IMessageRecipient} from "../../src/interfaces/IMessageRecipient.sol";

/// @notice Test mailbox: records dispatches and can synchronously deliver a
/// message to a local recipient (playing both chains' mailboxes).
contract MockMailbox is IMailbox {
    uint32 public immutable domain;
    uint256 public dispatchCount;

    uint32 public lastDestination;
    bytes32 public lastRecipient;
    bytes public lastBody;

    constructor(uint32 domain_) {
        domain = domain_;
    }

    function dispatch(uint32 destinationDomain, bytes32 recipientAddress, bytes calldata body)
        external
        payable
        returns (bytes32)
    {
        dispatchCount += 1;
        lastDestination = destinationDomain;
        lastRecipient = recipientAddress;
        lastBody = body;
        return keccak256(abi.encode(dispatchCount, destinationDomain, recipientAddress, body));
    }

    function quoteDispatch(uint32, bytes32, bytes calldata) external pure returns (uint256) {
        return 0;
    }

    function localDomain() external view returns (uint32) {
        return domain;
    }

    /// @notice Deliver a message as if relayed from (origin, sender).
    function deliver(address recipient, uint32 origin, bytes32 sender, bytes calldata body)
        external
    {
        IMessageRecipient(recipient).handle(origin, sender, body);
    }
}
