// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {EnvelopeLib} from "./EnvelopeLib.sol";
import {IMailbox} from "./interfaces/IMailbox.sol";
import {IMessageRecipient} from "./interfaces/IMessageRecipient.sol";
import {RailsPortal} from "./RailsPortal.sol";

/// @notice The canonical share token for one subnet's staked alpha, held in
/// the hub escrow on Bittensor (name/symbol per subnet, e.g. Chutes/CHUTES
/// for netuid 64). Rebasing display (stETH-style):
/// accounts hold shares, `balanceOf` returns `shares * indexE9 / 1e9`, and
/// the index — escrowed alpha per share — only ever moves via hub messages,
/// so the wallet balance ticks upward as the escrow earns emissions.
///
/// There is no owner-settable price and no local mint authority: shares are
/// minted exclusively by hub message (a completed buy) and burned by
/// `sell()`, which dispatches the sell envelope to the hub.
contract Chutes is IMessageRecipient {
    string public name;
    string public symbol;
    uint8 public constant decimals = 9;

    address public owner;

    /// Hyperlane wiring: mint/index messages come from the keyless runtime
    /// identity (`hubSender`); sell envelopes go to the Gateway contract on
    /// the hub (`hubRecipient`).
    IMailbox public mailbox;
    uint32 public hubDomain;
    bytes32 public hubSender;
    bytes32 public hubRecipient;
    /// The portal on this chain: shared sequential-nonce source (the hub
    /// executes envelopes in strict nonce order across buys and sells).
    RailsPortal public portal;
    /// The one subnet this token wraps.
    uint16 public netuid;
    /// PSM asset released on sells (the portal's USDC).
    uint32 public usdAssetId;

    /// Share index in 1e9 fixed point: alpha per share at the hub. Rises as
    /// the escrow earns emissions; set only via hub message in `handle`.
    uint64 public indexE9 = 1_000_000_000;

    mapping(address => uint256) public sharesOf;
    uint256 public totalShares;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event HubConfigured(
        address mailbox,
        uint32 hubDomain,
        bytes32 hubSender,
        bytes32 hubRecipient,
        address portal,
        uint16 netuid,
        uint32 usdAssetId
    );
    event HubMint(address indexed to, uint64 shares, uint64 indexE9);
    event IndexUpdated(uint64 indexE9);
    event SoldToHub(
        address indexed from, uint64 shares, uint64 minUsd, uint64 indexed nonce, bytes32 messageId
    );

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }

    constructor(address owner_, string memory name_, string memory symbol_) {
        require(owner_ != address(0), "owner zero");
        require(bytes(name_).length > 0 && bytes(symbol_).length > 0, "name empty");
        owner = owner_;
        name = name_;
        symbol = symbol_;
    }

    // ---------------------------------------------------------------- admin

    function transferOwnership(address next) external onlyOwner {
        require(next != address(0), "owner zero");
        owner = next;
    }

    /// @notice Wire the hub path. Deployment-time only; there is no other
    /// admin surface (no local mint, no owner-set index).
    function configureHub(
        address mailbox_,
        uint32 hubDomain_,
        bytes32 hubSender_,
        bytes32 hubRecipient_,
        address portal_,
        uint16 netuid_,
        uint32 usdAssetId_
    ) external onlyOwner {
        mailbox = IMailbox(mailbox_);
        hubDomain = hubDomain_;
        hubSender = hubSender_;
        hubRecipient = hubRecipient_;
        portal = RailsPortal(portal_);
        netuid = netuid_;
        usdAssetId = usdAssetId_;
        emit HubConfigured(
            mailbox_, hubDomain_, hubSender_, hubRecipient_, portal_, netuid_, usdAssetId_
        );
    }

    // ------------------------------------------------------ rebasing ERC-20

    /// @notice Display balance: shares priced at the current hub index.
    function balanceOf(address account) public view returns (uint256) {
        return (sharesOf[account] * uint256(indexE9)) / 1e9;
    }

    function totalSupply() external view returns (uint256) {
        return (totalShares * uint256(indexE9)) / 1e9;
    }

    /// @notice Shares represented by a display `amount` at the current index.
    function sharesForAmount(uint256 amount) public view returns (uint256) {
        return (amount * 1e9) / uint256(indexE9);
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _transfer(msg.sender, to, amount);
        return true;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= amount, "ERC20: insufficient allowance");
            allowance[from][msg.sender] = allowed - amount;
        }
        _transfer(from, to, amount);
        return true;
    }

    function _transfer(address from, address to, uint256 amount) internal {
        require(to != address(0), "ERC20: transfer to zero");
        uint256 shares = sharesForAmount(amount);
        uint256 fromShares = sharesOf[from];
        require(fromShares >= shares, "ERC20: insufficient balance");
        unchecked {
            sharesOf[from] = fromShares - shares;
            sharesOf[to] += shares;
        }
        emit Transfer(from, to, amount);
    }

    // ------------------------------------------------------------- hub path

    /// @notice Hyperlane delivery from the hub. Body:
    /// abi.encode(address to, uint64 shares, uint64 indexE9). A zero `to`
    /// with zero shares is a pure index heartbeat. This is the only way the
    /// index moves.
    function handle(uint32 origin, bytes32 sender, bytes calldata body) external payable override {
        require(msg.sender == address(mailbox), "not mailbox");
        require(origin == hubDomain && sender == hubSender, "untrusted hub");
        (address to, uint64 shares, uint64 newIndexE9) =
            abi.decode(body, (address, uint64, uint64));
        if (newIndexE9 > 0) {
            indexE9 = newIndexE9;
            emit IndexUpdated(newIndexE9);
        }
        if (shares > 0 && to != address(0)) {
            sharesOf[to] += shares;
            totalShares += shares;
            emit HubMint(to, shares, indexE9);
            emit Transfer(address(0), to, (uint256(shares) * uint256(indexE9)) / 1e9);
        }
    }

    /// @notice Burn `shares` and dispatch the sell envelope: the hub
    /// unstakes escrow alpha, swaps to USD, and releases USDC to the caller
    /// through the portal on this chain. Use `sharesOf(you)` to sell all.
    function sell(uint64 shares, uint64 minUsd) external payable returns (bytes32 messageId) {
        require(address(mailbox) != address(0) && hubRecipient != bytes32(0), "hub not configured");
        require(shares > 0, "shares zero");
        uint256 fromShares = sharesOf[msg.sender];
        require(fromShares >= shares, "sell exceeds balance");
        unchecked {
            sharesOf[msg.sender] = fromShares - shares;
            totalShares -= shares;
        }
        emit Transfer(msg.sender, address(0), (uint256(shares) * uint256(indexE9)) / 1e9);

        uint64 nonce = portal.assignNonce();
        bytes memory envelope = EnvelopeLib.sellShares(
            netuid, shares, msg.sender, usdAssetId, minUsd, mailbox.localDomain(), nonce
        );
        messageId = mailbox.dispatch{value: msg.value}(
            hubDomain, hubRecipient, abi.encode(uint64(shares), envelope)
        );
        emit SoldToHub(msg.sender, shares, minUsd, nonce, messageId);
    }
}
