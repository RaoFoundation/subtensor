// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

import {RateLimit} from "./RateLimit.sol";
import {Wire} from "./Wire.sol";
import {IPortal} from "./interfaces/IPortal.sol";

/// @title The vault that holds the stablecoin behind tUSD.
/// @notice Not upgradeable. It is where a payer deposits, so an approval of the stablecoin
/// is only ever given to code that cannot change, and that only moves the approver's funds
/// when the approver calls. The portal, which can be upgraded, can do one thing here:
/// release the stablecoin, within a rate limit that only the owner can change. The limit
/// bounds what a faulty or forged release can take before the guardian stops releases.
contract Vault {
    using SafeERC20 for IERC20;
    using RateLimit for RateLimit.Window;

    /// @notice The stablecoin.
    IERC20 public immutable stable;
    /// @notice The portal: the only caller of `release`.
    IPortal public immutable portal;
    /// @notice Sets the release limit and the guardian, and resumes releases. A timelock.
    address public immutable owner;
    /// @notice May stop releases, and nothing else.
    address public guardian;
    /// @notice Whether releases are stopped. Deposits follow the portal's pause.
    bool public paused;
    /// @notice The allowance for releases, in the stablecoin's units.
    RateLimit.Window public releases;

    event Deposited(address indexed payer, uint256 amount, uint64 indexed nonce);
    event Released(address indexed to, uint256 amount);
    event ReleaseLimitSet(uint128 limit, uint64 period);
    event GuardianSet(address guardian);
    event PausedSet(bool paused);

    error OnlyOwner();
    error OnlyPortal();
    error OnlyGuardianOrOwner();
    error Paused();
    error NothingReceived();
    error ZeroAddress();

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    /// @param releaseLimit The most that can be released at once, in the stablecoin's units.
    /// @param releasePeriod The seconds over which a used limit refills.
    constructor(
        IERC20 stable_,
        IPortal portal_,
        address owner_,
        address guardian_,
        uint128 releaseLimit,
        uint64 releasePeriod
    ) {
        // None of the three can be set again.
        if (
            address(stable_) == address(0) || address(portal_) == address(0) || owner_ == address(0)
        ) revert ZeroAddress();
        stable = stable_;
        portal = portal_;
        owner = owner_;
        guardian = guardian_;
        releases.retarget(releaseLimit, releasePeriod);
        emit GuardianSet(guardian_);
        emit ReleaseLimitSet(releaseLimit, releasePeriod);
    }

    /// @notice Locks `amount` of the caller's stablecoin and orders `destination` for it
    /// from the hub. If the hub cannot settle the order, it has the stablecoin released
    /// back to the caller. `msg.value` pays the transport; what it does not need returns
    /// to the caller.
    /// @param amount In the stablecoin's units. The caller must have approved it to this
    /// vault.
    /// @param minOut The least of the destination asset the caller accepts, with 9 decimals.
    /// @param deadline Unix seconds after which the hub refunds the order.
    /// @return nonce The order's sequence number at the portal.
    function deposit(
        uint256 amount,
        Wire.Destination calldata destination,
        uint64 minOut,
        uint64 deadline
    ) external payable returns (uint64 nonce) {
        // Count what arrived, not what was asked for: some stablecoins can take a fee.
        uint256 held = stable.balanceOf(address(this));
        stable.safeTransferFrom(msg.sender, address(this), amount);
        uint256 received = stable.balanceOf(address(this)) - held;
        if (received == 0) revert NothingReceived();
        nonce =
            portal.deposited{value: msg.value}(msg.sender, received, destination, minOut, deadline);
        emit Deposited(msg.sender, received, nonce);
    }

    /// @notice Pays `amount` of the stablecoin to `to`. Only the portal, on a credit from
    /// the hub, and within the release limit.
    function release(address to, uint256 amount) external {
        if (msg.sender != address(portal)) revert OnlyPortal();
        if (paused) revert Paused();
        releases.consume(amount);
        stable.safeTransfer(to, amount);
        emit Released(to, amount);
    }

    /// @notice What can be released now.
    function releasable() external view returns (uint256) {
        return releases.availableAt(block.timestamp);
    }

    function setReleaseLimit(uint128 limit, uint64 period) external onlyOwner {
        releases.retarget(limit, period);
        emit ReleaseLimitSet(limit, period);
    }

    function setGuardian(address guardian_) external onlyOwner {
        guardian = guardian_;
        emit GuardianSet(guardian_);
    }

    /// @notice Stops releases. The guardian or the owner.
    function pause() external {
        if (msg.sender != guardian && msg.sender != owner) revert OnlyGuardianOrOwner();
        paused = true;
        emit PausedSet(true);
    }

    /// @notice Resumes releases. Only the owner.
    function unpause() external onlyOwner {
        paused = false;
        emit PausedSet(false);
    }
}
