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
import {Address} from "@openzeppelin/contracts/utils/Address.sol";

import {IRAILS_ADDRESS, IRails} from "precompiles/rails.sol";

import {Wire} from "./Wire.sol";
import {IMailbox} from "./interfaces/IMailbox.sol";
import {
    IMessageRecipient,
    ISpecifiesInterchainSecurityModule
} from "./interfaces/IMessageRecipient.sol";

/// @title The gateway of the canonical rails on Bittensor's EVM.
/// @notice The one contract the hub pallet takes orders from. It passes what the transport
/// delivers to the hub, and hands the hub's messages to the transport. It decides nothing:
/// the hub checks which portal sent an order and settles it. What makes the gateway
/// critical is that the hub trusts it to pass on only messages that the transport's
/// security module verified.
///
/// The two directions never share a transaction. `handle` settles an order and stops.
/// The hub's messages, among them the answer to that order, leave when someone calls
/// `flush`: a keeper, or anyone. So nothing that goes wrong on the way out can undo an
/// order or hold it up.
///
/// It holds no TAO between transactions. The TAO that pays the transport for the hub's
/// messages is in accounts of the hub, one for each bridge. For each message the gateway
/// tells the hub what the transport charges. The hub hands the gateway that much from the
/// account of the message's bridge, up to a limit, and the gateway pays it on in the same
/// transaction.
///
/// The limit is the hub's to set. The hub keeps a reserve for every message that has not
/// left, so a message can always leave at a charge up to the reserve. Above the reserve
/// the hub pays from what the bridge's account can spare, up to the bridge's ceiling. If
/// the transport charges more than the limit, and the caller does not pay the difference,
/// the message is deferred: it leaves the line and stays in the hub with its reserve. The
/// messages behind it leave, and `send` hands it to the transport later.
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

    /// @notice An order was passed to the hub. `status`: 1 settled, 2 refunded, 3 rejected.
    /// `bridge` is the hub's id of the portal's bridge: the answer to the order waits in
    /// the hub's queue for that bridge until someone calls `flush` for it.
    event OrderExecuted(
        uint32 indexed origin,
        bytes32 indexed portal,
        uint64 indexed nonce,
        uint8 status,
        uint64 amountOut,
        uint32 bridge
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
    /// @notice Message `nonce` of the hub's queue for `bridge` was deferred: the transport
    /// charges `fee` for it, the hub pays at most `limit` now, and nobody paid the
    /// difference. It is still in the hub, and `send` hands it to the transport later.
    event MessageDeferred(uint32 indexed bridge, uint64 indexed nonce, uint256 fee, uint256 limit);
    event InterchainSecurityModuleSet(address module);
    event GuardianSet(address guardian);

    error OnlyMailbox();
    error OnlyGuardianOrOwner();
    /// @notice The hub has no message `nonce` of `bridge` that can leave now.
    error NoSuchMessage(uint32 bridge, uint64 nonce);
    /// @notice The transport charges `fee` for the message. The hub pays at most `limit`
    /// of it now, and the caller did not send the rest.
    error ChargeNotCovered(uint256 fee, uint256 limit);

    /// @dev One message of the hub, and the most the hub pays the transport for it now.
    struct Outbound {
        uint32 bridge;
        uint64 nonce;
        uint32 domain;
        bytes32 recipient;
        bytes body;
        uint256 limit;
    }

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @param owner_ The timelock.
    function initialize(address owner_, address guardian_, IMailbox mailbox_) external initializer {
        __Ownable_init(owner_);
        __Ownable2Step_init();
        __Pausable_init();
        __UUPSUpgradeable_init();
        mailbox = mailbox_;
        guardian = guardian_;
        emit GuardianSet(guardian_);
    }

    /// @notice Accepts TAO, so that a refund from the transport cannot fail. TAO sent here
    /// pays for no message: pay into a bridge's account on the hub instead
    /// (`Rails.fund_messages`).
    receive() external payable {}

    /// @inheritdoc IMessageRecipient
    /// @dev Reverts if the hub does not take the order now: it is paused, the order is out
    /// of sequence, its bridge is disabled, or the bridge's account on the hub does not
    /// hold the reserve for one more message. The transport then keeps the message and
    /// delivers it again, and what the payer locked stays on the spoke.
    ///
    /// This hands nothing to the transport. The answer to the order waits in the hub's
    /// queue for the order's bridge until someone calls `flush`. An order that the hub can
    /// settle is settled, whatever the transport does on the way out.
    function handle(uint32 origin, bytes32 sender, bytes calldata body)
        external
        payable
        whenNotPaused
    {
        if (msg.sender != address(mailbox)) revert OnlyMailbox();
        (uint8 status, uint64 amountOut, uint32 bridge) = RAILS.execute(origin, sender, body);

        // The hub took the body, so it is an order.
        Wire.Order memory order = Wire.decodeOrder(body);
        emit OrderExecuted(origin, sender, order.nonce, status, amountOut, bridge);
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
    }

    /// @notice Takes up to `limit` messages from the head of the hub's queue for `bridge`
    /// and hands them to the transport. Anyone may call this: it does what the hub already
    /// decided. A keeper calls it when messages wait.
    ///
    /// The hub pays the transport's charge for a message up to a limit: the bridge's
    /// reserve, and above it what the bridge's account can spare, up to the bridge's
    /// ceiling. If the transport charges more, the caller pays the difference with this
    /// call. What the caller sent and was not needed goes back.
    ///
    /// A message whose charge neither covers is deferred: it leaves the line and stays in
    /// the hub with its reserve. `send` hands it over later, and the messages behind it do
    /// not wait for it.
    ///
    /// Reverts if the transport refuses a message: that message stays at the head of the
    /// queue. Nothing here catches a failed call. A caught call would let a caller with
    /// too little gas defer a message that is paid for.
    /// @return sent How many were handed to the transport.
    function flush(uint32 bridge, uint256 limit)
        external
        payable
        whenNotPaused
        returns (uint256 sent)
    {
        uint256 left = msg.value;
        for (uint256 taken; taken < limit; ++taken) {
            Outbound memory message;
            bool pending;
            (
                pending,
                message.nonce,
                message.domain,
                message.recipient,
                message.body,
                message.limit
            ) = RAILS.nextOutbound(bridge);
            if (!pending) break;
            message.bridge = bridge;
            (uint256 fee, uint256 fromHub) = _charge(message);
            if (fee - fromHub > left) {
                RAILS.deferOutbound(bridge, message.nonce);
                emit MessageDeferred(bridge, message.nonce, fee, message.limit);
                continue;
            }
            left -= fee - fromHub;
            _handOver(message, fee, fromHub);
            ++sent;
        }
        if (left > 0) Address.sendValue(payable(msg.sender), left);
    }

    /// @notice Hands message `nonce` of `bridge` to the transport: a deferred message, or
    /// the head of the hub's queue. Anyone may call this. The caller pays what the
    /// transport charges above what the hub pays, and gets back what was not needed.
    /// Reverts if the charge is not covered: the message stays where it is.
    function send(uint32 bridge, uint64 nonce) external payable whenNotPaused {
        Outbound memory message;
        bool pending;
        (pending, message.domain, message.recipient, message.body, message.limit) =
            RAILS.outbound(bridge, nonce);
        if (!pending) revert NoSuchMessage(bridge, nonce);
        message.bridge = bridge;
        message.nonce = nonce;
        (uint256 fee, uint256 fromHub) = _charge(message);
        uint256 fromCaller = fee - fromHub;
        if (fromCaller > msg.value) revert ChargeNotCovered(fee, message.limit);
        _handOver(message, fee, fromHub);
        if (msg.value > fromCaller) Address.sendValue(payable(msg.sender), msg.value - fromCaller);
    }

    /// @dev What the transport charges for `message` now, and how much of it the hub pays.
    function _charge(Outbound memory message) private view returns (uint256 fee, uint256 fromHub) {
        fee = mailbox.quoteDispatch(message.domain, message.recipient, message.body);
        fromHub = fee < message.limit ? fee : message.limit;
    }

    /// @dev The hub hands this contract `fromHub` from the account of the message's
    /// bridge. The message leaves the hub, is paid for and enters the transport in one
    /// transaction: if one step fails, none happened.
    function _handOver(Outbound memory message, uint256 fee, uint256 fromHub) private {
        RAILS.ackOutbound(message.bridge, message.nonce, fromHub);
        bytes32 messageId =
            mailbox.dispatch{value: fee}(message.domain, message.recipient, message.body);
        emit MessageSent(
            message.bridge, message.nonce, message.domain, message.recipient, messageId
        );
    }

    function setInterchainSecurityModule(address module) external onlyOwner {
        interchainSecurityModule = module;
        emit InterchainSecurityModuleSet(module);
    }

    function setGuardian(address guardian_) external onlyOwner {
        guardian = guardian_;
        emit GuardianSet(guardian_);
    }

    /// @notice Stops orders and messages. The guardian or the owner.
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
