// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {BaseERC20} from "./BaseERC20.sol";
import {EnvelopeLib} from "./EnvelopeLib.sol";
import {IMailbox} from "./interfaces/IMailbox.sol";
import {IMessageRecipient} from "./interfaces/IMessageRecipient.sol";

/// @notice Origin-chain (Base) gateway: `buy()` locks USDC and sends a
/// BuyShares envelope to the Bittensor hub in one transaction; the hub
/// releases locked USDC back through `handle()` when shares are sold.
///
/// This contract owns the sequential envelope nonce for its chain: the hub
/// executes envelopes in strict nonce order, so every dispatcher (buys here,
/// sells on the subnet share tokens) draws from this one counter.
contract RailsPortal is IMessageRecipient {
    address public owner;
    BaseERC20 public immutable usd;
    IMailbox public immutable mailbox;
    uint32 public immutable hubDomain;
    /// PSM asset id of `usd` on the hub.
    uint32 public immutable usdAssetId;
    /// The Gateway contract on the Bittensor EVM, as bytes32.
    bytes32 public gateway;
    /// The hub-side sender allowed to release collateral (the keyless
    /// runtime identity), as bytes32.
    bytes32 public hubReleaser;
    /// Subnet share tokens on this chain (may draw nonces for sell
    /// envelopes), keyed by token address.
    mapping(address => bool) public isToken;

    /// Next sequential envelope nonce; mirrors the hub's NextNonce.
    uint64 public nextNonce;

    event Bought(
        address indexed from,
        uint64 amountUsd,
        uint16 netuid,
        uint64 indexed nonce,
        bytes32 messageId
    );
    event Deposited(
        address indexed from, uint64 amount, uint64 indexed nonce, bytes32 messageId
    );
    event Released(address indexed to, uint64 amount);

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }

    constructor(
        address owner_,
        address usd_,
        address mailbox_,
        uint32 hubDomain_,
        uint32 usdAssetId_,
        bytes32 gateway_
    ) {
        require(owner_ != address(0), "owner zero");
        owner = owner_;
        usd = BaseERC20(usd_);
        mailbox = IMailbox(mailbox_);
        hubDomain = hubDomain_;
        usdAssetId = usdAssetId_;
        gateway = gateway_;
    }

    function setGateway(bytes32 gateway_) external onlyOwner {
        gateway = gateway_;
    }

    function setHubReleaser(bytes32 releaser) external onlyOwner {
        hubReleaser = releaser;
    }

    function setToken(address token, bool allowed) external onlyOwner {
        isToken[token] = allowed;
    }

    /// @notice Draw the next sequential envelope nonce. Only registered
    /// share tokens may call (an open counter would let anyone burn nonces
    /// and stall the hub's strict ordering).
    function assignNonce() external returns (uint64 nonce) {
        require(isToken[msg.sender], "not token");
        nonce = nextNonce++;
    }

    /// @notice Buy CHUTES with USDC in one transaction: locks `amountUsd`,
    /// builds the BuyShares envelope, and dispatches it to the hub. The hub
    /// mints tUSD, swaps to TAO, stakes into the escrow, and messages the
    /// share mint back to `msg.sender` on this chain.
    function buy(uint64 amountUsd, uint16 netuid, uint64 minAlpha)
        external
        payable
        returns (bytes32 messageId, uint64 nonce)
    {
        require(amountUsd > 0, "amount zero");
        require(usd.transferFrom(msg.sender, address(this), amountUsd), "lock failed");
        nonce = nextNonce++;
        bytes memory envelope = EnvelopeLib.buyShares(
            usdAssetId, amountUsd, netuid, msg.sender, minAlpha, mailbox.localDomain(), nonce
        );
        messageId = mailbox.dispatch{value: msg.value}(
            hubDomain, gateway, abi.encode(amountUsd, envelope)
        );
        emit Bought(msg.sender, amountUsd, netuid, nonce, messageId);
    }

    /// @notice Generic deposit door (kept for drills and the tUSD-credit
    /// fallback path): locks `amount` USD and dispatches `envelopePrefix`
    /// with the portal-assigned sequential nonce appended. The prefix is the
    /// SCALE envelope minus its trailing u64 nonce.
    function deposit(uint64 amount, bytes calldata envelopePrefix)
        external
        payable
        returns (bytes32 messageId, uint64 nonce)
    {
        require(amount > 0, "amount zero");
        require(usd.transferFrom(msg.sender, address(this), amount), "lock failed");
        nonce = nextNonce++;
        bytes memory envelope = abi.encodePacked(envelopePrefix, EnvelopeLib.u64le(nonce));
        messageId = mailbox.dispatch{value: msg.value}(
            hubDomain, gateway, abi.encode(amount, envelope)
        );
        emit Deposited(msg.sender, amount, nonce, messageId);
    }

    /// @notice Hub releases locked collateral (sell proceeds).
    /// Body: abi.encode(address to, uint64 amount).
    function handle(uint32 origin, bytes32 sender, bytes calldata body) external payable override {
        require(msg.sender == address(mailbox), "not mailbox");
        require(origin == hubDomain && sender == hubReleaser, "untrusted releaser");
        (address to, uint64 amount) = abi.decode(body, (address, uint64));
        require(usd.transfer(to, amount), "release failed");
        emit Released(to, amount);
    }
}
