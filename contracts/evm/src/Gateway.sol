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
/// It holds TAO only to pay the transport for the hub's messages. The hub divides that
/// TAO between the bridges: each has a budget, paid in on the hub, and the TAO arrives at
/// this contract's account on Bittensor, which is this contract's balance. For each
/// message the gateway tells the hub what the transport charges, and the hub takes that
/// from the budget of the message's bridge or refuses. So the messages to one spoke never
/// spend what was paid in for another.
///
/// An order and the answers of its bridge go through in one transaction or not at all.
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
    event InterchainSecurityModuleSet(address module);
    event GuardianSet(address guardian);
    event FlushLimitSet(uint256 limit);

    error OnlyMailbox();
    error OnlyGuardianOrOwner();

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

    /// @notice Accepts TAO, so that a refund from the transport cannot fail. TAO sent here
    /// is in no bridge's budget and pays for no message: pay in on the hub instead
    /// (`Rails.fund_messages`).
    receive() external payable {}

    /// @inheritdoc IMessageRecipient
    /// @dev Reverts if the hub does not take the order now (it is paused, the order is out
    /// of sequence, its bridge is disabled, or the bridge's budget does not cover one
    /// more message), or if the answers of the order's bridge cannot be handed over. The
    /// transport then keeps the message and delivers it again, and what the payer locked
    /// stays on the spoke.
    ///
    /// The hand-over is not a call that may fail in silence. Then a transaction with too
    /// little gas for it would still succeed, and a relayer that estimates gas would
    /// send exactly such transactions: the answers would wait for somebody else.
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
        _flush(bridge, flushLimit);
    }

    /// @notice Hands up to `limit` of the messages that wait in the hub's queue for
    /// `bridge` to the transport. Anyone may call this: it does what the hub already
    /// decided. Reverts as a whole if one of them cannot leave, because the bridge's
    /// budget on the hub does not cover what the transport charges or because the
    /// transport refuses it: call again with a smaller `limit` to hand over the ones
    /// before it. Each bridge has its own queue and its own budget, so such a message
    /// holds only the messages to the same spoke.
    /// @return sent How many were handed over.
    function flush(uint32 bridge, uint256 limit) external whenNotPaused returns (uint256 sent) {
        return _flush(bridge, limit);
    }

    function _flush(uint32 bridge, uint256 limit) private returns (uint256 sent) {
        for (; sent < limit; ++sent) {
            (bool pending, uint64 nonce, uint32 domain, bytes32 recipient, bytes memory body) =
                RAILS.nextOutbound(bridge);
            if (!pending) break;
            uint256 fee = mailbox.quoteDispatch(domain, recipient, body);
            // The hub takes the fee from the budget of this bridge, and reverts if the budget
            // holds less. The message leaves the queue, is paid for and enters the transport
            // in one transaction.
            RAILS.ackOutbound(bridge, nonce, fee);
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
