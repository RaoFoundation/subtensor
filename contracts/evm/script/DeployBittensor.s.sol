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
        // Skip creation when code already sits at the CREATE2 address, so a
        // re-run against a live chain re-wires instead of reverting with a
        // create collision.
        address canonicalUsd = vm.computeCreate2Address(
            salt,
            keccak256(
                abi.encodePacked(
                    type(CanonicalShareToken).creationCode,
                    abi.encode("Canonical USD", "cUSD", owner)
                )
            )
        );
        if (canonicalUsd.code.length == 0) {
            canonicalUsd =
                address(new CanonicalShareToken{salt: salt}("Canonical USD", "cUSD", owner));
        }
        address gateway = vm.computeCreate2Address(
            salt,
            keccak256(
                abi.encodePacked(
                    type(Gateway).creationCode,
                    abi.encode(owner, mailbox, canonicalUsd, psmEscrow)
                )
            )
        );
        if (gateway.code.length == 0) {
            gateway = address(new Gateway{salt: salt}(owner, mailbox, canonicalUsd, psmEscrow));
        }
        vm.stopBroadcast();

        // Consumed by the rig manifest writer.
        console2.log("CANONICAL_USD", canonicalUsd);
        console2.log("GATEWAY", gateway);
    }
}
