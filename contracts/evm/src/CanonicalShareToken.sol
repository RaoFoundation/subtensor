// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {BaseERC20} from "./BaseERC20.sol";
import {RateLimits} from "./RateLimits.sol";

/// @notice Chain-owned canonical token: rate-limited multi-minter ERC-20
/// (xERC-20 style). Used as the canonical USD backing token on the Bittensor
/// EVM, minted by the Gateway when portal deposits arrive.
///
/// Trust model: the owner (chain governance; locally the deployer) grants
/// each bridge adapter a linear-refill mint window. A compromised minter is
/// bounded by its window and revocable instantly.
contract CanonicalShareToken is BaseERC20 {
    using RateLimits for RateLimits.Window;

    address public owner;

    /// Per-minter mint windows for local bridge adapters.
    mapping(address => RateLimits.Window) public minterWindows;
    mapping(address => bool) public isMinter;

    event MinterLimitsSet(address indexed minter, uint64 limit, uint64 refillPerSecond);
    event MinterRemoved(address indexed minter);

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }

    constructor(string memory name_, string memory symbol_, address owner_)
        BaseERC20(name_, symbol_)
    {
        require(owner_ != address(0), "owner zero");
        owner = owner_;
    }

    // ---------------------------------------------------------------- admin

    function transferOwnership(address next) external onlyOwner {
        require(next != address(0), "owner zero");
        owner = next;
    }

    /// @notice Grant or update a bridge adapter's mint window.
    function setMinterLimits(address minter, uint64 limit, uint64 refillPerSecond)
        external
        onlyOwner
    {
        isMinter[minter] = true;
        RateLimits.Window storage w = minterWindows[minter];
        w.limit = limit;
        w.refillPerTick = refillPerSecond;
        w.lastTick = uint64(block.timestamp);
        emit MinterLimitsSet(minter, limit, refillPerSecond);
    }

    /// @notice Instantly revoke a minter (the emergency path).
    function removeMinter(address minter) external onlyOwner {
        isMinter[minter] = false;
        delete minterWindows[minter];
        emit MinterRemoved(minter);
    }

    // -------------------------------------------------------------- minting

    /// @notice Mint within the caller's rate window.
    function mint(address to, uint64 amount) external {
        require(isMinter[msg.sender], "not minter");
        require(
            minterWindows[msg.sender].tryReserve(uint64(block.timestamp), amount),
            "mint rate limited"
        );
        _mint(to, amount);
    }

    /// @notice Burn from `from` with allowance semantics (bridge withdraws).
    function burnFrom(address from, uint64 amount) external {
        require(isMinter[msg.sender], "not minter");
        if (from != msg.sender) {
            uint256 allowed = allowance[from][msg.sender];
            require(allowed >= amount, "burn allowance");
            if (allowed != type(uint256).max) {
                allowance[from][msg.sender] = allowed - amount;
            }
        }
        _burn(from, amount);
        // Burning frees mint headroom for the adapter that owns the flow.
        minterWindows[msg.sender].release(uint64(block.timestamp), amount);
    }

}
