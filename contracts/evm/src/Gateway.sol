// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {
    Ownable2StepUpgradeable
} from "@openzeppelin/contracts-upgradeable/access/Ownable2StepUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import {
    PausableUpgradeable
} from "@openzeppelin/contracts-upgradeable/utils/PausableUpgradeable.sol";

import {IRAILS_ADDRESS, IRails} from "precompiles/rails.sol";

import {Wire} from "./Wire.sol";
import {IMailbox} from "./interfaces/IMailbox.sol";
import {
    IMessageRecipient,
    ISpecifiesInterchainSecurityModule
} from "./interfaces/IMessageRecipient.sol";

/// @title The gateway of the canonical rails on Bittensor's EVM.
/// @notice The one contract the hub pallet takes orders from. It passes what the transport
/// delivers to the hub, and hands the hub's answers to the transport. It decides nothing:
/// the hub checks which portal sent an order and settles it. What makes the gateway
/// critical is that the hub trusts it to pass on only messages that the transport's
/// security module verified.
///
/// It holds TAO only to pay the transport for the hub's messages. The hub sees to it that
/// every message is paid for before it is queued: an order's answer from the budget of
/// the order's bridge, an exit by the account that exits. The TAO arrives at this
/// contract's account on Bittensor, which is this contract's balance.
///
/// Upgradeable (UUPS). The owner upgrades, configures and resumes. The guardian only
/// pauses.
contract Gateway is
    Initializable,
    UUPSUpgradeable,
    Ownable2StepUpgradeable,
    PausableUpgradeable,
    IMessageRecipient,
    ISpecifiesInterchainSecurityModule
{
    /// @notice The hub, through its precompile.
    IRails private constant RAILS = IRails(IRAILS_ADDRESS);

    /// @notice The transport.
    IMailbox public mailbox;
    /// @notice The security module the transport verifies portals' messages with. Zero
    /// selects the transport's default.
    address public interchainSecurityModule;
    /// @notice May pause, and nothing else.
    address public guardian;
    /// @notice The most answers handed to the transport after one order.
    uint256 public flushLimit;

    /// @notice An order was passed to the hub. `status`: 1 settled, 2 refunded, 3 rejected.
    event OrderExecuted(
        uint32 indexed origin,
        bytes32 indexed portal,
        uint64 indexed nonce,
        uint8 status,
        uint64 amountOut
    );
    /// @notice An order bought service credits: `gamma` was spent at `service` on `netuid`
    /// for `beneficiary`. `origin` and `nonce` name the order.
    event CreditsPurchased(
        address indexed beneficiary,
        uint16 indexed netuid,
        bytes32 indexed service,
        uint32 origin,
        uint64 nonce,
        uint64 gamma
    );
    /// @notice Message `nonce` of the hub's queue for `bridge` was handed to the transport.
    event MessageSent(
        uint32 indexed bridge,
        uint64 indexed nonce,
        uint32 domain,
        bytes32 recipient,
        bytes32 messageId
    );
    /// @notice The answers for `bridge` could not be handed over after an order. They
    /// wait in the hub's queue for `flush`.
    event FlushFailed(uint32 indexed bridge, bytes reason);
    event InterchainSecurityModuleSet(address module);
    event GuardianSet(address guardian);
    event FlushLimitSet(uint256 limit);

    error OnlyMailbox();
    error OnlyGuardianOrOwner();
    error OnlySelf();

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @param owner_ The timelock.
    function initialize(address owner_, address guardian_, IMailbox mailbox_, uint256 flushLimit_)
        external
        initializer
    {
        __Ownable_init(owner_);
        __Ownable2Step_init();
        __Pausable_init();
        __UUPSUpgradeable_init();
        mailbox = mailbox_;
        guardian = guardian_;
        flushLimit = flushLimit_;
        emit GuardianSet(guardian_);
        emit FlushLimitSet(flushLimit_);
    }

    /// @notice Accepts TAO to pay the transport with.
    receive() external payable {}

    /// @inheritdoc IMessageRecipient
    /// @dev Reverts if the hub does not take the order now (it is paused, the order is out
    /// of sequence, or its bridge is disabled): the transport keeps the message and
    /// delivers it again. A failure to hand over the answers does not revert: the order
    /// is settled, and the answers wait for `flush`.
    function handle(uint32 origin, bytes32 sender, bytes calldata body)
        external
        payable
        whenNotPaused
    {
        if (msg.sender != address(mailbox)) revert OnlyMailbox();
        (uint8 status, uint64 amountOut, uint32 bridge) = RAILS.execute(origin, sender, body);

        // The hub took the body, so it is an order.
        Wire.Order memory order = Wire.decodeOrder(body);
        emit OrderExecuted(origin, sender, order.nonce, status, amountOut);
        if (status == 1 && order.destination == Wire.TO_CREDITS) {
            emit CreditsPurchased(
                order.beneficiary,
                order.destinationNetuid,
                order.recipient,
                origin,
                order.nonce,
                amountOut
            );
        }

        // The answer to the order waits in the queue of the order's bridge.
        try this.flushFromHandle(bridge) {}
        catch (bytes memory reason) {
            emit FlushFailed(bridge, reason);
        }
    }

    /// @notice Hands up to `limit` of the messages that wait in the hub's queue for
    /// `bridge` to the transport. Anyone may call this: it does what the hub already
    /// decided. Reverts as a whole if the gateway cannot pay the transport for one of
    /// them. Each bridge has its own queue, so a message that cannot leave holds only the
    /// messages to the same spoke.
    /// @return sent How many were handed over.
    function flush(uint32 bridge, uint256 limit) external whenNotPaused returns (uint256 sent) {
        return _flush(bridge, limit);
    }

    /// @dev `handle` calls this through an external call to itself, so that a failure
    /// undoes the hand-over alone.
    function flushFromHandle(uint32 bridge) external {
        if (msg.sender != address(this)) revert OnlySelf();
        _flush(bridge, flushLimit);
    }

    function _flush(uint32 bridge, uint256 limit) private returns (uint256 sent) {
        for (; sent < limit; ++sent) {
            (bool pending, uint64 nonce, uint32 domain, bytes32 recipient, bytes memory body) =
                RAILS.nextOutbound(bridge);
            if (!pending) break;
            // The hub queued the message only after this contract's account was paid the
            // bridge's cost of one message.
            uint256 fee = mailbox.quoteDispatch(domain, recipient, body);
            // The message leaves the queue and enters the transport in one transaction.
            RAILS.ackOutbound(bridge, nonce);
            bytes32 messageId = mailbox.dispatch{value: fee}(domain, recipient, body);
            emit MessageSent(bridge, nonce, domain, recipient, messageId);
        }
    }

    function setInterchainSecurityModule(address module) external onlyOwner {
        interchainSecurityModule = module;
        emit InterchainSecurityModuleSet(module);
    }

    function setGuardian(address guardian_) external onlyOwner {
        guardian = guardian_;
        emit GuardianSet(guardian_);
    }

    function setFlushLimit(uint256 limit) external onlyOwner {
        flushLimit = limit;
        emit FlushLimitSet(limit);
    }

    /// @notice Stops orders and answers. The guardian or the owner.
    function pause() external {
        if (msg.sender != guardian && msg.sender != owner()) revert OnlyGuardianOrOwner();
        _pause();
    }

    /// @notice Resumes. Only the owner.
    function unpause() external onlyOwner {
        _unpause();
    }

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
