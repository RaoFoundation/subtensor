// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {
    OwnableUpgradeable
} from "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

import {Portal} from "../src/Portal.sol";
import {Wire} from "../src/Wire.sol";
import {IMailbox} from "../src/interfaces/IMailbox.sol";
import {Fixture} from "./Fixture.sol";

/// @notice A later version of the portal, to upgrade to.
contract PortalV2 is Portal {
    function version() external pure returns (uint256) {
        return 2;
    }
}

contract GovernanceTest is Fixture {
    function test_TheTimelockOwnsThePortalAndTheVault() public view {
        assertEq(portal.owner(), address(timelock));
        assertEq(vault.owner(), address(timelock));
        assertEq(timelock.getMinDelay(), 48 hours);
        assertTrue(timelock.hasRole(timelock.PROPOSER_ROLE(), multisig));
        assertTrue(timelock.hasRole(timelock.EXECUTOR_ROLE(), multisig));
        // Nobody administers the timelock but itself.
        assertFalse(timelock.hasRole(timelock.DEFAULT_ADMIN_ROLE(), multisig));
        assertFalse(timelock.hasRole(timelock.DEFAULT_ADMIN_ROLE(), address(this)));
    }

    function test_AnUpgradeWaitsFortyEightHours() public {
        address next = address(new PortalV2());
        bytes memory upgrade = abi.encodeCall(portal.upgradeToAndCall, (next, ""));

        // Not the multisig directly, nor the guardian.
        for (uint256 i = 0; i < 3; ++i) {
            address caller = [multisig, guardian, alice][i];
            vm.prank(caller);
            vm.expectRevert(
                abi.encodeWithSelector(
                    OwnableUpgradeable.OwnableUnauthorizedAccount.selector, caller
                )
            );
            portal.upgradeToAndCall(next, "");
        }

        vm.startPrank(multisig);
        // Not with a shorter delay.
        vm.expectRevert(
            abi.encodeWithSelector(
                TimelockController.TimelockInsufficientDelay.selector, 47 hours, 48 hours
            )
        );
        timelock.schedule(address(portal), 0, upgrade, bytes32(0), bytes32(0), 47 hours);

        timelock.schedule(address(portal), 0, upgrade, bytes32(0), bytes32(0), 48 hours);
        // Not before the delay has passed.
        skip(48 hours - 1);
        vm.expectRevert();
        timelock.execute(address(portal), 0, upgrade, bytes32(0), bytes32(0));
        skip(1);
        timelock.execute(address(portal), 0, upgrade, bytes32(0), bytes32(0));
        vm.stopPrank();

        assertEq(PortalV2(address(portal)).version(), 2);
    }

    function test_AnUpgradeKeepsTheState() public {
        deposit(alice, 250 * USDC, toTao(alice));
        credit(Wire.ASSET_TAO, 0, alice, 5 * UNIT, 0);
        address taoBefore = address(portal.tao());

        timelocked(
            address(portal), abi.encodeCall(portal.upgradeToAndCall, (address(new PortalV2()), ""))
        );

        assertEq(address(portal.tao()), taoBefore);
        assertEq(address(portal.vault()), address(vault));
        assertEq(portal.nextOrder(), 1);
        assertTrue(portal.processed(0));
        assertEq(portal.gateway(), GATEWAY);
        // The tokens and the vault still know the portal by its address.
        credit(Wire.ASSET_TAO, 0, alice, 5 * UNIT, 0);
        assertEq(tao.balanceOf(alice), 10 * UNIT);
    }

    function test_ThePendingUpgradeCanBeCancelled() public {
        bytes memory upgrade =
            abi.encodeCall(portal.upgradeToAndCall, (address(new PortalV2()), ""));
        vm.startPrank(multisig);
        timelock.schedule(address(portal), 0, upgrade, bytes32(0), bytes32(0), 48 hours);
        timelock.cancel(timelock.hashOperation(address(portal), 0, upgrade, bytes32(0), bytes32(0)));
        skip(48 hours);
        vm.expectRevert();
        timelock.execute(address(portal), 0, upgrade, bytes32(0), bytes32(0));
        vm.stopPrank();
    }

    function test_TheGuardianOnlyPauses() public {
        vm.startPrank(guardian);
        portal.pause();
        vault.pause();
        bytes memory unauthorized = abi.encodeWithSelector(
            OwnableUpgradeable.OwnableUnauthorizedAccount.selector, guardian
        );
        vm.expectRevert(unauthorized);
        portal.unpause();
        vm.expectRevert(unauthorized);
        portal.setGateway(bytes32(uint256(1)));
        vm.expectRevert(unauthorized);
        portal.setInterchainSecurityModule(guardian);
        vm.expectRevert(unauthorized);
        portal.setGuardian(guardian);
        vm.expectRevert(unauthorized);
        portal.transferOwnership(guardian);
        vm.stopPrank();
    }

    function test_OnlyTheOwnerConfiguresThePortal() public {
        timelocked(address(portal), abi.encodeCall(portal.setGateway, (bytes32(uint256(0x99)))));
        assertEq(portal.gateway(), bytes32(uint256(0x99)));
        timelocked(
            address(portal), abi.encodeCall(portal.setInterchainSecurityModule, (address(0x15)))
        );
        assertEq(portal.interchainSecurityModule(), address(0x15));
        timelocked(address(portal), abi.encodeCall(portal.setGuardian, (bob)));
        vm.prank(guardian);
        vm.expectRevert(Portal.OnlyGuardianOrOwner.selector);
        portal.pause();
        vm.prank(bob);
        portal.pause();
    }

    function test_ThePortalIsInitializedOnceAndItsLogicNever() public {
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        portal.initialize(alice, alice, IMailbox(address(mailbox)), 1, bytes32(0), vault);

        Portal logic = new Portal();
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        logic.initialize(alice, alice, IMailbox(address(mailbox)), 1, bytes32(0), vault);
    }

    function test_OwnershipMovesInTwoSteps() public {
        timelocked(address(portal), abi.encodeCall(portal.transferOwnership, (bob)));
        // Still the timelock, until the new owner accepts.
        assertEq(portal.owner(), address(timelock));
        vm.prank(bob);
        portal.acceptOwnership();
        assertEq(portal.owner(), bob);
    }
}
