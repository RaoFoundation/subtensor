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
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {Address} from "@openzeppelin/contracts/utils/Address.sol";
import {Create2} from "@openzeppelin/contracts/utils/Create2.sol";
import {SafeCast} from "@openzeppelin/contracts/utils/math/SafeCast.sol";
import {BitMaps} from "@openzeppelin/contracts/utils/structs/BitMaps.sol";

import {Vault} from "./Vault.sol";
import {Wire} from "./Wire.sol";
import {IMailbox} from "./interfaces/IMailbox.sol";
import {
    IMessageRecipient,
    ISpecifiesInterchainSecurityModule
} from "./interfaces/IMessageRecipient.sol";
import {IPortal} from "./interfaces/IPortal.sol";
import {AlphaToken} from "./tokens/AlphaToken.sol";
import {CanonicalTao} from "./tokens/CanonicalTao.sol";

/// @title The portal of the canonical rails on one chain.
/// @notice It turns what a payer locked in the vault or burned of a canonical token into an
/// order for the hub on Bittensor, and it carries out what the hub answers: release the
/// stablecoin, mint a canonical token, create a subnet's alpha token.
///
/// The portal holds no stablecoin and no tokens, and takes no approvals. Payers deposit at
/// the vault and sell at the tokens, which are not upgradeable. What an upgrade of the
/// portal could do is mint canonical tokens and release the stablecoin within the vault's
/// rate limit: that is why the owner is a timelock, and why a guardian can pause.
///
/// The stablecoin leaves the vault at a limited rate. The portal keeps what the hub told
/// it to pay and the vault has not released yet (`owed`), and anyone can have it paid as
/// the allowance refills (`claim`).
///
/// Upgradeable (UUPS). The owner upgrades, configures and resumes. The guardian only
/// pauses.
contract Portal is
    Initializable,
    UUPSUpgradeable,
    Ownable2StepUpgradeable,
    PausableUpgradeable,
    IPortal,
    IMessageRecipient,
    ISpecifiesInterchainSecurityModule
{
    using BitMaps for BitMaps.BitMap;

    /// @notice The salt of canonical TAO. An alpha token's salt is its netuid.
    bytes32 private constant TAO_SALT = keccak256("TAO");

    /// @notice The transport.
    IMailbox public mailbox;
    /// @notice The transport domain of Bittensor.
    uint32 public hubDomain;
    /// @notice The gateway contract on Bittensor: where orders go, and the only sender this
    /// portal obeys.
    bytes32 public gateway;
    /// @notice The security module the transport verifies the hub's messages with. Zero
    /// selects the transport's default.
    address public interchainSecurityModule;
    /// @notice May pause, and nothing else.
    address public guardian;

    /// @notice The vault that holds the stablecoin.
    Vault public vault;
    /// @notice What one unit of the stablecoin is in the hub's 9 decimals.
    uint256 public stableScale;
    /// @notice Canonical TAO.
    CanonicalTao public tao;
    /// @notice The alpha token of each subnet that has one.
    mapping(uint16 netuid => AlphaToken) public alphaTokens;
    /// @notice The netuid of each alpha token, plus one. Zero for any other address.
    mapping(address token => uint256) private _netuidPlusOne;

    /// @notice The sequence number of the next order.
    uint64 public nextOrder;
    /// @notice The hub's messages that were carried out, by nonce.
    BitMaps.BitMap private _processed;

    /// @notice The stablecoin that the hub told this portal to pay to an address and that
    /// the vault has not released yet, in the stablecoin's units. The vault releases at a
    /// limited rate. What does not fit when the hub's answer arrives is on record here
    /// and is paid by `claim` as the allowance refills.
    mapping(address recipient => uint256) public owed;
    /// @notice The sum of `owed`.
    uint256 public totalOwed;

    /// @notice What a payer pays with each order for the hub's answer, in this chain's coin,
    /// on top of the transport's fee for the order. Every order has an answer, a refund
    /// too, and the gateway on Bittensor pays the transport for it. This fee makes each
    /// order pay for its own answer, so that no payer's orders or refunds are sent at the
    /// cost of what the gateway holds for the others.
    uint256 public answerFee;
    /// @notice Who receives the answer fees: the operator that keeps the gateway funded.
    address public feeRecipient;

    event OrderSent(
        uint64 indexed nonce,
        address indexed payer,
        uint8 source,
        uint16 sourceNetuid,
        uint64 amount,
        Wire.Destination destination,
        uint64 minOut,
        uint64 deadline,
        bytes32 messageId
    );
    /// @notice A credit of the hub was carried out: a token was minted, or the stablecoin
    /// was paid or put on record as owed (see `StablePaid`). `answersOrder`, `order` and
    /// `refund` name the order it answers, if it answers one.
    event Credited(
        uint64 indexed nonce,
        address indexed to,
        uint8 asset,
        uint16 netuid,
        uint64 amount,
        bool answersOrder,
        uint64 indexed order,
        bool refund
    );
    event TokenCreated(uint16 indexed netuid, address token, string name, string symbol);
    /// @notice The hub's registry lists another address for a token than the one it has.
    event RegistryMismatch(uint16 indexed netuid, address listed, address token);
    event GatewaySet(bytes32 gateway);
    event InterchainSecurityModuleSet(address module);
    event GuardianSet(address guardian);
    /// @notice The vault paid `amount` of the stablecoin to `to`. `stillOwed` is what the
    /// vault's allowance did not cover yet.
    event StablePaid(address indexed to, uint256 amount, uint256 stillOwed);
    event AnswerFeeSet(uint256 fee, address recipient);
    event FeesSwept(address indexed recipient, uint256 amount);

    error OnlyMailbox();
    error OnlyVault();
    error OnlyGuardianOrOwner();
    error NotACanonicalToken(address caller);
    error NotTheHub(uint32 origin, bytes32 sender);
    error AlreadyProcessed(uint64 nonce);
    error UnknownMessage(uint8 kind);
    error UnknownAsset(uint8 asset);
    error NoSuchToken(uint16 netuid);
    error TokenExists(uint16 netuid);
    error AmountTooSmall();
    error SameAsset();
    error FeeNotPaid(uint256 fee, uint256 paid);
    error UnsupportedDecimals(uint8 decimals);
    error NoFeeRecipient();

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @param owner_ The timelock.
    /// @param vault_ The vault, deployed for this proxy's address.
    /// @param answerFee_ What each order pays for the hub's answer. See `answerFee`.
    /// @param feeRecipient_ Who receives the answer fees.
    function initialize(
        address owner_,
        address guardian_,
        IMailbox mailbox_,
        uint32 hubDomain_,
        bytes32 gateway_,
        Vault vault_,
        uint256 answerFee_,
        address feeRecipient_
    ) external initializer {
        __Ownable_init(owner_);
        __Ownable2Step_init();
        __Pausable_init();
        __UUPSUpgradeable_init();
        mailbox = mailbox_;
        hubDomain = hubDomain_;
        gateway = gateway_;
        guardian = guardian_;
        vault = vault_;
        _setAnswerFee(answerFee_, feeRecipient_);
        uint8 decimals = IERC20Metadata(address(vault_.stable())).decimals();
        if (decimals > 9) revert UnsupportedDecimals(decimals);
        stableScale = 10 ** (9 - decimals);
        tao = new CanonicalTao{salt: TAO_SALT}();
        emit GatewaySet(gateway_);
        emit GuardianSet(guardian_);
    }

    // ---------------------------------------------------------------- orders to the hub

    /// @inheritdoc IPortal
    function deposited(
        address payer,
        uint256 stableAmount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) external payable whenNotPaused returns (uint64 nonce) {
        if (msg.sender != address(vault)) revert OnlyVault();
        uint64 amount = SafeCast.toUint64(stableAmount * stableScale);
        return _send(payer, Wire.ASSET_STABLE, 0, amount, destination, minOut, deadline);
    }

    /// @inheritdoc IPortal
    function sold(
        address payer,
        uint64 amount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) external payable whenNotPaused returns (uint64 nonce) {
        if (msg.sender == address(tao)) {
            return _send(payer, Wire.ASSET_TAO, 0, amount, destination, minOut, deadline);
        }
        uint256 netuidPlusOne = _netuidPlusOne[msg.sender];
        if (netuidPlusOne == 0) revert NotACanonicalToken(msg.sender);
        // forge-lint: disable-next-line(unsafe-typecast)
        uint16 netuid = uint16(netuidPlusOne - 1);
        return _send(payer, Wire.ASSET_ALPHA, netuid, amount, destination, minOut, deadline);
    }

    /// @notice What an order costs now, on top of what it pays with: the transport's fee
    /// to carry it, and the answer fee. A payer sends at least this as `msg.value` with a
    /// deposit or a sale.
    function orderFee() external view returns (uint256) {
        Wire.Order memory order;
        return mailbox.quoteDispatch(hubDomain, gateway, Wire.encode(order)) + answerFee;
    }

    function _send(
        address payer,
        uint8 source,
        uint16 sourceNetuid,
        uint64 amount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) private returns (uint64 nonce) {
        if (amount == 0) revert AmountTooSmall();
        Wire.check(destination);
        // The first three destinations are the three assets. The hub would refund an order
        // that asks for what it pays with.
        if (
            destination.kind == source
                && (source != Wire.ASSET_ALPHA || destination.netuid == sourceNetuid)
        ) revert SameAsset();
        nonce = nextOrder++;
        bytes memory body = Wire.encode(
            Wire.Order({
                version: Wire.ORDER_VERSION,
                nonce: nonce,
                payer: payer,
                source: source,
                sourceNetuid: sourceNetuid,
                amount: amount,
                destination: destination.kind,
                destinationNetuid: destination.netuid,
                recipient: destination.recipient,
                beneficiary: destination.beneficiary,
                minOut: minOut,
                deadline: deadline
            })
        );
        uint256 transport = mailbox.quoteDispatch(hubDomain, gateway, body);
        // The answer fee stays here until `sweepFees` pays it out.
        uint256 fee = transport + answerFee;
        if (msg.value < fee) revert FeeNotPaid(fee, msg.value);
        bytes32 messageId = mailbox.dispatch{value: transport}(hubDomain, gateway, body);
        emit OrderSent(
            nonce, payer, source, sourceNetuid, amount, destination, minOut, deadline, messageId
        );
        // Last, after every effect: the payer may be a contract.
        if (msg.value > fee) Address.sendValue(payable(payer), msg.value - fee);
    }

    /// @notice Pays the answer fees that orders left here to `feeRecipient`. Anyone may
    /// call this: the fees can go nowhere else.
    function sweepFees() external {
        address recipient = feeRecipient;
        if (recipient == address(0)) revert NoFeeRecipient();
        uint256 amount = address(this).balance;
        emit FeesSwept(recipient, amount);
        Address.sendValue(payable(recipient), amount);
    }

    // ---------------------------------------------------------------- answers of the hub

    /// @inheritdoc IMessageRecipient
    /// @dev Reverts while paused: the transport keeps the message and delivers it again.
    function handle(uint32 origin, bytes32 sender, bytes calldata body)
        external
        payable
        whenNotPaused
    {
        if (msg.sender != address(mailbox)) revert OnlyMailbox();
        if (origin != hubDomain || sender != gateway) revert NotTheHub(origin, sender);
        uint8 kind = Wire.kind(body);
        if (kind == Wire.KIND_CREDIT) {
            _credit(Wire.decodeCredit(body));
        } else if (kind == Wire.KIND_REGISTER_TOKEN) {
            _registerToken(Wire.decodeRegisterToken(body));
        } else {
            revert UnknownMessage(kind);
        }
    }

    /// @notice Whether the hub's message `nonce` was carried out.
    function processed(uint64 nonce) external view returns (bool) {
        return _processed.get(nonce);
    }

    /// @dev The transport delivers a message once. This also holds if the transport or its
    /// security module is replaced.
    function _markProcessed(uint64 nonce) private {
        if (_processed.get(nonce)) revert AlreadyProcessed(nonce);
        _processed.set(nonce);
    }

    function _credit(Wire.Credit memory credit) private {
        _markProcessed(credit.nonce);
        emit Credited(
            credit.nonce,
            credit.to,
            credit.asset,
            credit.netuid,
            credit.amount,
            credit.answersOrder,
            credit.order,
            credit.refund
        );
        if (credit.asset == Wire.ASSET_STABLE) {
            // The hub sends whole units of the stablecoin. Were it not to, the part below
            // one unit would stay in the vault.
            uint256 amount = credit.amount / stableScale;
            owed[credit.to] += amount;
            totalOwed += amount;
            _pay(credit.to);
        } else if (credit.asset == Wire.ASSET_TAO) {
            tao.mint(credit.to, credit.amount);
        } else if (credit.asset == Wire.ASSET_ALPHA) {
            AlphaToken token = alphaTokens[credit.netuid];
            // Reverts until the token's registration arrives; the transport tries again.
            if (address(token) == address(0)) revert NoSuchToken(credit.netuid);
            token.mint(credit.to, credit.amount, credit.rate, credit.nonce);
        } else {
            revert UnknownAsset(credit.asset);
        }
    }

    /// @notice Pays `to` the stablecoin it is owed, as far as the vault's allowance goes
    /// now. Anyone may call this: the payment can only go to `to`.
    /// @return paid What was paid now.
    function claim(address to) external whenNotPaused returns (uint256 paid) {
        return _pay(to);
    }

    /// @dev Pays what the vault can release now of what `to` is owed, and leaves the rest
    /// on record. A payment larger than the vault's allowance, and a payment while the
    /// vault is stopped, do not fail: they wait here, in part or in whole.
    ///
    /// The debt is lowered before the vault is called. If the stablecoin refuses the
    /// transfer, the whole call reverts: nothing is hidden from a caller that estimates
    /// gas, and an answer of the hub stays with the transport, which delivers it again.
    function _pay(address to) private returns (uint256 paid) {
        uint256 amount = owed[to];
        uint256 available = vault.releasable();
        paid = amount < available ? amount : available;
        if (paid == 0) return 0;
        owed[to] = amount - paid;
        totalOwed -= paid;
        emit StablePaid(to, paid, amount - paid);
        vault.release(to, paid);
    }

    function _registerToken(Wire.RegisterToken memory register) private {
        _markProcessed(register.nonce);
        if (address(alphaTokens[register.netuid]) != address(0)) {
            revert TokenExists(register.netuid);
        }
        AlphaToken token = new AlphaToken{salt: bytes32(uint256(register.netuid))}();
        alphaTokens[register.netuid] = token;
        _netuidPlusOne[address(token)] = uint256(register.netuid) + 1;
        emit TokenCreated(register.netuid, address(token), register.name, register.symbol);
        // The token is where this portal creates it, whatever the hub lists. A difference
        // is a mistake in the hub's registry, which its admin corrects there.
        if (register.token != address(token)) {
            emit RegistryMismatch(register.netuid, register.token, address(token));
        }
        token.initialize(register.netuid, register.name, register.symbol);
    }

    /// @notice The address of the alpha token of `netuid`, whether it exists yet or not.
    function alphaTokenAddress(uint16 netuid) external view returns (address) {
        return
            Create2.computeAddress(
                bytes32(uint256(netuid)), keccak256(type(AlphaToken).creationCode)
            );
    }

    // ---------------------------------------------------------------- owner and guardian

    function setGateway(bytes32 gateway_) external onlyOwner {
        gateway = gateway_;
        emit GatewaySet(gateway_);
    }

    function setInterchainSecurityModule(address module) external onlyOwner {
        interchainSecurityModule = module;
        emit InterchainSecurityModuleSet(module);
    }

    function setGuardian(address guardian_) external onlyOwner {
        guardian = guardian_;
        emit GuardianSet(guardian_);
    }

    /// @notice Sets what each order pays for the hub's answer, and who receives it. Set
    /// the fee to at least what the transport charges the gateway for one message.
    function setAnswerFee(uint256 fee, address recipient) external onlyOwner {
        _setAnswerFee(fee, recipient);
    }

    function _setAnswerFee(uint256 fee, address recipient) private {
        if (fee != 0 && recipient == address(0)) revert NoFeeRecipient();
        answerFee = fee;
        feeRecipient = recipient;
        emit AnswerFeeSet(fee, recipient);
    }

    /// @notice Stops orders and the hub's messages. The guardian or the owner.
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
