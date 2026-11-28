// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {Script} from "forge-std/Script.sol";

import {Gateway} from "../src/Gateway.sol";
import {IMailbox} from "../src/interfaces/IMailbox.sol";

/// @notice Deploys the gateway on Bittensor's EVM, and the timelock that owns it.
///
/// Environment:
/// - `MAILBOX`: the transport's Mailbox on Bittensor's EVM.
/// - `MULTISIG`: proposes and executes through the timelock.
/// - `GUARDIAN`: may pause.
/// - `TIMELOCK_DELAY`: seconds. Defaults to 48 hours.
/// - `OUTPUT`: where to write the addresses, under `deployments/`.
contract DeployGateway is Script {
    function run() external {
        IMailbox mailbox = IMailbox(vm.envAddress("MAILBOX"));
        address multisig = vm.envAddress("MULTISIG");
        address guardian = vm.envAddress("GUARDIAN");
        uint256 delay = vm.envOr("TIMELOCK_DELAY", uint256(48 hours));

        vm.startBroadcast();
        address[] memory holders = new address[](1);
        holders[0] = multisig;
        TimelockController timelock = new TimelockController(delay, holders, holders, address(0));
        Gateway implementation = new Gateway();
        address gateway = address(
            new ERC1967Proxy(
                address(implementation),
                abi.encodeCall(Gateway.initialize, (address(timelock), guardian, mailbox))
            )
        );
        vm.stopBroadcast();

        string memory json = "gateway";
        vm.serializeAddress(json, "timelock", address(timelock));
        vm.serializeAddress(json, "gatewayImplementation", address(implementation));
        string memory output = vm.serializeAddress(json, "gateway", gateway);
        vm.writeJson(output, string.concat("deployments/", vm.envString("OUTPUT")));
    }
}
