// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Script} from "forge-std/Script.sol";

import {Portal} from "../src/Portal.sol";
import {Vault} from "../src/Vault.sol";
import {IMailbox} from "../src/interfaces/IMailbox.sol";
import {IPortal} from "../src/interfaces/IPortal.sol";
import {MockStable} from "../src/mocks/MockStable.sol";

/// @notice Deploys a spoke: the timelock that owns it, the vault and the portal.
///
/// Environment:
/// - `MAILBOX`: the transport's Mailbox on this chain.
/// - `HUB_DOMAIN`: the transport domain of Bittensor.
/// - `GATEWAY`: the gateway contract on Bittensor's EVM.
/// - `MULTISIG`: proposes and executes through the timelock.
/// - `GUARDIAN`: may pause.
/// - `STABLE`: the stablecoin. Leave unset only on a local network: a mock is deployed.
/// - `TIMELOCK_DELAY`: seconds. Defaults to 48 hours.
/// - `RELEASE_LIMIT`, `RELEASE_PERIOD`: the vault's allowance, in the stablecoin's units
///   and in seconds.
/// - `ANSWER_FEE`: what each order pays for the hub's answer, in wei. It has no default:
///   decide it. Set it to at least the worth of what the hub charges this bridge for one
///   message (`Rails.AnswerCost`). Zero only if the transport carries the hub's
///   messages for nothing.
/// - `FEE_RECIPIENT`: who receives the answer fees: the operator that pays them into
///   the bridge's budget on Bittensor (`Rails.fund_answers`). It has no default.
/// - `OUTPUT`: where to write the addresses, under `deployments/`.
contract DeploySpoke is Script {
    function run() external {
        vm.startBroadcast();
        (, address deployer,) = vm.readCallers();
        address stable = vm.envOr("STABLE", address(0));
        if (stable == address(0)) {
            require(block.chainid != 8453 && block.chainid != 1, "set STABLE on a public network");
            stable = address(new MockStable());
        }
        TimelockController timelock = _timelock();
        Portal implementation = new Portal();
        // The vault is built for the portal's address: the contract after the vault.
        address proxy = vm.computeCreateAddress(deployer, vm.getNonce(deployer) + 1);
        Vault vault = new Vault(
            IERC20(stable),
            IPortal(proxy),
            address(timelock),
            vm.envAddress("GUARDIAN"),
            uint128(vm.envUint("RELEASE_LIMIT")),
            uint64(vm.envUint("RELEASE_PERIOD"))
        );
        Portal portal = _portal(implementation, timelock, vault);
        require(address(portal) == proxy, "the portal is not where the vault expects it");
        vm.stopBroadcast();

        string memory json = "spoke";
        vm.serializeAddress(json, "timelock", address(timelock));
        vm.serializeAddress(json, "stable", stable);
        vm.serializeAddress(json, "vault", address(vault));
        vm.serializeAddress(json, "portal", address(portal));
        vm.serializeAddress(json, "portalImplementation", address(implementation));
        string memory output = vm.serializeAddress(json, "tao", address(portal.tao()));
        vm.writeJson(output, string.concat("deployments/", vm.envString("OUTPUT")));
    }

    function _timelock() private returns (TimelockController) {
        address[] memory holders = new address[](1);
        holders[0] = vm.envAddress("MULTISIG");
        return new TimelockController(
            vm.envOr("TIMELOCK_DELAY", uint256(48 hours)), holders, holders, address(0)
        );
    }

    function _portal(Portal implementation, TimelockController timelock, Vault vault)
        private
        returns (Portal)
    {
        bytes memory initialize = abi.encodeCall(
            Portal.initialize,
            (
                address(timelock),
                vm.envAddress("GUARDIAN"),
                IMailbox(vm.envAddress("MAILBOX")),
                uint32(vm.envUint("HUB_DOMAIN")),
                bytes32(uint256(uint160(vm.envAddress("GATEWAY")))),
                vault,
                vm.envUint("ANSWER_FEE"),
                vm.envAddress("FEE_RECIPIENT")
            )
        );
        return Portal(address(new ERC1967Proxy(address(implementation), initialize)));
    }
}
