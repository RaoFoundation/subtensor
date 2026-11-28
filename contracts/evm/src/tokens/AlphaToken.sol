// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

import {RailsToken} from "./RailsToken.sol";

/// @title The alpha token of one subnet.
/// @notice One token is one share of the hub's vault for the subnet, which stakes alpha at
/// the subnet's index hotkey. The number of tokens never changes by itself: a token is worth
/// more alpha as the stake earns, as wstETH is worth more stETH.
///
/// The live rate is on Bittensor (`IRails.alphaRate`). `rate` here is what the hub
/// reported with the newest of its mints that arrived: use it for display, not to price a
/// trade.
contract AlphaToken is RailsToken {
    /// @notice The scale of `rate`: `RATE_SCALE` is one alpha for one token.
    uint256 public constant RATE_SCALE = 1e18;

    /// @notice The subnet.
    uint16 public netuid;
    /// @notice The alpha that one token was worth at the hub's newest mint that arrived,
    /// times 1e18.
    uint256 public rate;
    /// @notice When `rate` arrived, in Unix seconds.
    uint256 public rateUpdatedAt;
    /// @notice The hub's number of the message that reported `rate`. The hub numbers its
    /// messages in the order it makes them.
    uint64 public rateNonce;
    /// @notice The alpha that a holder's tokens were worth when the holder got them: the
    /// base for "what are my tokens worth now, against what I paid". It moves with tokens
    /// that are transferred and leaves with tokens that are sold, in proportion.
    mapping(address holder => uint256 alpha) public principalOf;

    string private _name;
    string private _symbol;

    event RateReported(uint256 rate);

    error AlreadyInitialized();

    constructor() ERC20("", "") {}

    /// @notice Sets the subnet and the name once. Only the portal, when it creates the
    /// token. The creation code takes no arguments, so the token's address depends on the
    /// portal and the subnet alone.
    function initialize(uint16 netuid_, string calldata name_, string calldata symbol_)
        external
        onlyPortal
    {
        if (rate != 0) revert AlreadyInitialized();
        netuid = netuid_;
        _name = name_;
        _symbol = symbol_;
        rate = RATE_SCALE;
        rateUpdatedAt = block.timestamp;
    }

    function name() public view override returns (string memory) {
        return _name;
    }

    function symbol() public view override returns (string memory) {
        return _symbol;
    }

    /// @notice Mints `amount` to `to`. Only the portal, on a credit from the hub.
    /// @param reportedRate The rate the hub reports with the credit, or 0 if it reports
    /// none (a refund).
    /// @param nonce The hub's number of the credit. The hub's messages can arrive in
    /// another order than it made them, and one can arrive much later: the token keeps
    /// the rate of the newest.
    function mint(address to, uint256 amount, uint256 reportedRate, uint64 nonce)
        external
        onlyPortal
    {
        if (reportedRate != 0 && nonce >= rateNonce) {
            rate = reportedRate;
            rateNonce = nonce;
            rateUpdatedAt = block.timestamp;
            emit RateReported(reportedRate);
        }
        // New tokens are worth what the hub reported with them, whenever they arrive. A
        // refund reports nothing: it is worth the rate on record.
        principalOf[to] += amount * (reportedRate == 0 ? rate : reportedRate) / RATE_SCALE;
        _mint(to, amount);
    }

    /// @notice The alpha that `tokens` are worth at the last reported rate, rounded down.
    function convertToAssets(uint256 tokens) public view returns (uint256) {
        return tokens * rate / RATE_SCALE;
    }

    /// @notice The tokens that `alpha` is worth at the last reported rate, rounded down.
    function convertToShares(uint256 alpha) public view returns (uint256) {
        return alpha * RATE_SCALE / rate;
    }

    /// @dev Keeps `principalOf` with the tokens. `mint` adds the worth of new tokens. A
    /// transfer or a burn takes the sender's principal in the proportion of the tokens
    /// that leave, rounded down, so the principals never sum to more than was added.
    function _update(address from, address to, uint256 value) internal override {
        if (from == address(0)) {
            super._update(from, to, value);
            return;
        }
        uint256 held = balanceOf(from);
        super._update(from, to, value);
        uint256 moved = held == 0 ? 0 : principalOf[from] * value / held;
        principalOf[from] -= moved;
        if (to != address(0)) principalOf[to] += moved;
    }
}
