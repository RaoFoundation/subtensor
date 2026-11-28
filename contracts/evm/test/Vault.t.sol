// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

import {RateLimit} from "../src/RateLimit.sol";
import {Vault} from "../src/Vault.sol";
import {Wire} from "../src/Wire.sol";
import {IPortal} from "../src/interfaces/IPortal.sol";
import {Fixture} from "./Fixture.sol";

/// @notice A stablecoin that keeps a hundredth of every transfer.
contract FeeStable is ERC20 {
    constructor() ERC20("Fee Dollar", "FEE") {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }

    function _update(address from, address to, uint256 value) internal override {
        if (from == address(0) || to == address(0)) {
            super._update(from, to, value);
            return;
        }
        uint256 fee = value / 100;
        super._update(from, address(0xFEE), fee);
        super._update(from, to, value - fee);
    }
}

/// @notice A stablecoin that calls the vault again while a deposit pulls it.
contract ReentrantStable is ERC20 {
    Vault public vault;
    bool private entered;

    constructor() ERC20("Reentrant Dollar", "RE") {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }

    function aim(Vault vault_) external {
        vault = vault_;
        _approve(address(this), address(vault_), type(uint256).max);
    }

    function _update(address from, address to, uint256 value) internal override {
        super._update(from, to, value);
        // A second deposit inside the first: without a guard the outer deposit would
        // count both arrivals, and the inner one its own, for one balance.
        if (to == address(vault) && !entered) {
            entered = true;
            Wire.Destination memory destination =
                Wire.Destination(Wire.TO_TAO, 0, bytes32(uint256(uint160(from))), address(0));
            vault.deposit(value, destination, 0, type(uint64).max);
        }
    }
}

/// @notice A portal that records what the vault tells it.
contract RecordingPortal is IPortal {
    address public payer;
    uint256 public amount;
    uint256 public value;

    function deposited(address payer_, uint256 amount_, Wire.Destination calldata, uint64, uint64)
        external
        payable
        returns (uint64)
    {
        payer = payer_;
        amount = amount_;
        value = msg.value;
        return 41;
    }

    function sold(address, uint64, Wire.Destination calldata, uint64, uint64)
        external
        payable
        returns (uint64)
    {
        return 0;
    }
}

contract VaultTest is Fixture {
    event Deposited(address indexed payer, uint256 amount, uint64 indexed nonce);
    event Released(address indexed to, uint256 amount);

    function test_ADepositLocksTheStablecoinAndSendsAnOrder() public {
        stable.mint(alice, 250 * USDC);
        vm.startPrank(alice);
        stable.approve(address(vault), 250 * USDC);
        vm.expectEmit(address(vault));
        emit Deposited(alice, 250 * USDC, 0);
        uint64 nonce = vault.deposit(250 * USDC, toTao(alice), 0, type(uint64).max);
        vm.stopPrank();

        assertEq(nonce, 0);
        assertEq(stable.balanceOf(address(vault)), 250 * USDC);
        assertEq(stable.balanceOf(alice), 0);
        Wire.Order memory order = lastOrder();
        assertEq(order.payer, alice);
        assertEq(order.source, Wire.ASSET_STABLE);
        // The hub counts in 9 decimals.
        assertEq(order.amount, 250 * UNIT);
    }

    function test_ADepositNeedsAnApprovalOfThePayerAndMovesOnlyTheirs() public {
        stable.mint(alice, 100 * USDC);
        // No approval.
        vm.prank(alice);
        vm.expectRevert();
        vault.deposit(100 * USDC, toTao(alice), 0, type(uint64).max);

        // Alice approves. Nobody else can spend that approval: a deposit moves the
        // caller's funds.
        vm.prank(alice);
        stable.approve(address(vault), 100 * USDC);
        vm.prank(bob);
        vm.expectRevert();
        vault.deposit(100 * USDC, toTao(bob), 0, type(uint64).max);
        assertEq(stable.balanceOf(alice), 100 * USDC);
    }

    function test_ADepositCountsWhatArrived() public {
        FeeStable fee = new FeeStable();
        RecordingPortal recorder = new RecordingPortal();
        Vault feeVault = new Vault(fee, recorder, address(timelock), guardian, 0, 0);
        fee.mint(alice, 100 * USDC);
        vm.startPrank(alice);
        fee.approve(address(feeVault), 100 * USDC);
        uint64 nonce = feeVault.deposit(100 * USDC, toTao(alice), 0, type(uint64).max);
        vm.stopPrank();

        assertEq(nonce, 41);
        assertEq(recorder.payer(), alice);
        assertEq(recorder.amount(), 99 * USDC);
        assertEq(fee.balanceOf(address(feeVault)), 99 * USDC);

        vm.prank(alice);
        vm.expectRevert(Vault.NothingReceived.selector);
        feeVault.deposit(0, toTao(alice), 0, type(uint64).max);
    }

    function test_AStablecoinThatCallsBackCannotEnterADepositTwice() public {
        ReentrantStable reentrant = new ReentrantStable();
        RecordingPortal recorder = new RecordingPortal();
        Vault target = new Vault(reentrant, recorder, address(timelock), guardian, 0, 0);
        reentrant.aim(target);
        reentrant.mint(alice, 100 * USDC);
        reentrant.mint(address(reentrant), 100 * USDC);
        vm.startPrank(alice);
        reentrant.approve(address(target), 100 * USDC);
        vm.expectRevert(ReentrancyGuard.ReentrancyGuardReentrantCall.selector);
        target.deposit(100 * USDC, toTao(alice), 0, type(uint64).max);
        vm.stopPrank();

        assertEq(reentrant.balanceOf(address(target)), 0);
        assertEq(recorder.amount(), 0);
    }

    function test_OnlyThePortalReleases() public {
        deposit(alice, 250 * USDC, toTao(alice));
        for (uint256 i = 0; i < 4; ++i) {
            address caller = [alice, guardian, multisig, address(timelock)][i];
            vm.prank(caller);
            vm.expectRevert(Vault.OnlyPortal.selector);
            vault.release(caller, 1);
        }
        vm.expectEmit(address(vault));
        emit Released(bob, 100 * USDC);
        vm.prank(address(portal));
        vault.release(bob, 100 * USDC);
        assertEq(stable.balanceOf(bob), 100 * USDC);
    }

    function test_ReleasesAreRateLimited() public {
        deposit(alice, 3_000_000 * USDC, toTao(alice));
        assertEq(vault.releasable(), 1_000_000 * USDC);

        vm.startPrank(address(portal));
        vault.release(bob, 1_000_000 * USDC);
        vm.expectRevert(abi.encodeWithSelector(RateLimit.RateLimited.selector, 1, 0));
        vault.release(bob, 1);

        // A quarter of the day gives a quarter of the limit back.
        skip(6 hours);
        assertEq(vault.releasable(), 250_000 * USDC);
        vault.release(bob, 250_000 * USDC);
        vm.stopPrank();
        assertEq(stable.balanceOf(bob), 1_250_000 * USDC);
    }

    function test_OnlyTheOwnerSetsTheLimitAndTheGuardian() public {
        for (uint256 i = 0; i < 4; ++i) {
            address caller = [alice, guardian, multisig, address(portal)][i];
            vm.startPrank(caller);
            vm.expectRevert(Vault.OnlyOwner.selector);
            vault.setReleaseLimit(1, 1);
            vm.expectRevert(Vault.OnlyOwner.selector);
            vault.setGuardian(caller);
            vm.stopPrank();
        }
        timelocked(
            address(vault), abi.encodeCall(Vault.setReleaseLimit, (5 * uint128(USDC), 1 hours))
        );
        assertEq(vault.releasable(), 5 * USDC);
        timelocked(address(vault), abi.encodeCall(Vault.setGuardian, (bob)));
        assertEq(vault.guardian(), bob);
    }

    function test_TheGuardianStopsReleasesAndOnlyTheOwnerResumesThem() public {
        deposit(alice, 250 * USDC, toTao(alice));
        vm.prank(alice);
        vm.expectRevert(Vault.OnlyGuardianOrOwner.selector);
        vault.pause();

        vm.prank(guardian);
        vault.pause();
        vm.prank(address(portal));
        vm.expectRevert(Vault.Paused.selector);
        vault.release(bob, 1);

        vm.prank(guardian);
        vm.expectRevert(Vault.OnlyOwner.selector);
        vault.unpause();
        timelocked(address(vault), abi.encodeCall(Vault.unpause, ()));
        vm.prank(address(portal));
        vault.release(bob, 1);
    }

    function test_TheVaultIsNotBuiltWithoutItsFixedParts() public {
        vm.expectRevert(Vault.ZeroAddress.selector);
        new Vault(IERC20(address(0)), portal, address(timelock), guardian, 0, 0);
        vm.expectRevert(Vault.ZeroAddress.selector);
        new Vault(stable, IPortal(address(0)), address(timelock), guardian, 0, 0);
        vm.expectRevert(Vault.ZeroAddress.selector);
        new Vault(stable, portal, address(0), guardian, 0, 0);
        // A vault may have no guardian.
        new Vault(stable, portal, address(timelock), address(0), 0, 0);
    }

    function test_TheVaultHasNoOtherWayOut() public view {
        // The vault's whole interface: nothing but `release` moves the stablecoin out,
        // and it has no upgrade, no owner transfer and no arbitrary call.
        assertEq(address(vault.portal()), address(portal));
        assertEq(vault.owner(), address(timelock));
        assertEq(address(vault.stable()), address(stable));
    }
}
