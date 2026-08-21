// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Script} from "forge-std/Script.sol";
import {console2} from "forge-std/console2.sol";
import {CanonicalShareToken} from "../src/CanonicalShareToken.sol";
import {Gateway} from "../src/Gateway.sol";

/// Deploys the Bittensor-EVM side: canonical USD token + Gateway.
/// Env: RAILS_OWNER, RAILS_MAILBOX (Hyperlane mailbox on Bittensor EVM),
/// RAILS_PSM_ESCROW (H160 mirror of the pallet escrow), RAILS_SALT.
contract DeployBittensor is Script {
    function run() external {
        address owner = vm.envAddress("RAILS_OWNER");
        address mailbox = vm.envAddress("RAILS_MAILBOX");
        address psmEscrow = vm.envAddress("RAILS_PSM_ESCROW");
        bytes32 salt = keccak256(bytes(vm.envOr("RAILS_SALT", string("rails-local-v1"))));

        vm.startBroadcast();
        CanonicalShareToken canonicalUsd =
            new CanonicalShareToken{salt: salt}("Canonical USD", "cUSD", owner);
        Gateway gateway =
            new Gateway{salt: salt}(owner, mailbox, address(canonicalUsd), psmEscrow);
        vm.stopBroadcast();

        // Consumed by the rig manifest writer.
        console2.log("CANONICAL_USD", address(canonicalUsd));
        console2.log("GATEWAY", address(gateway));
    }
}
