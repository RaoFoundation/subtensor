// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Script} from "forge-std/Script.sol";
import {console2} from "forge-std/console2.sol";
import {Chutes} from "../src/Chutes.sol";
import {MockUSDC} from "../src/MockUSDC.sol";
import {RailsPortal} from "../src/RailsPortal.sol";

/// Deploys the fake-Base (anvil) side: mock USDC, the buy/sell portal, and
/// one rebasing share token per catalog subnet. Hub wiring (configureHub,
/// setToken, setHubReleaser) happens in deploy-contracts.sh once the hub
/// facts are known.
/// Env: RAILS_OWNER, RAILS_MAILBOX (Hyperlane mailbox on anvil),
/// RAILS_HUB_DOMAIN (Bittensor domain id), RAILS_GATEWAY (Gateway address on
/// the Bittensor EVM), RAILS_USD_ASSET_ID, RAILS_SALT, and RAILS_TOKENS as
/// "Name|SYMBOL,Name|SYMBOL,..." in catalog order.
contract DeployBase is Script {
    function run() external {
        address owner = vm.envAddress("RAILS_OWNER");
        address mailbox = vm.envAddress("RAILS_MAILBOX");
        uint32 hubDomain = uint32(vm.envUint("RAILS_HUB_DOMAIN"));
        address gateway = vm.envAddress("RAILS_GATEWAY");
        uint32 usdAssetId = uint32(vm.envOr("RAILS_USD_ASSET_ID", uint256(0)));
        bytes32 salt = keccak256(bytes(vm.envOr("RAILS_SALT", string("rails-local-v1"))));
        string memory tokens = vm.envOr("RAILS_TOKENS", string("Chutes|CHUTES"));

        vm.startBroadcast();
        MockUSDC usdc = new MockUSDC{salt: salt}();
        RailsPortal portal = new RailsPortal{salt: salt}(
            owner,
            address(usdc),
            mailbox,
            hubDomain,
            usdAssetId,
            bytes32(uint256(uint160(gateway)))
        );

        // Consumed by the rig manifest writer.
        console2.log("MOCK_USDC", address(usdc));
        console2.log("PORTAL", address(portal));

        // One share token per "Name|SYMBOL" entry; the salt folds in the
        // symbol so every token gets a stable CREATE2 address.
        string[] memory entries = split(tokens, ",");
        for (uint256 i = 0; i < entries.length; i++) {
            string[] memory parts = split(entries[i], "|");
            require(parts.length == 2, "bad RAILS_TOKENS entry");
            Chutes token = new Chutes{salt: keccak256(abi.encodePacked(salt, parts[1]))}(
                owner, parts[0], parts[1]
            );
            console2.log(string.concat("TOKEN_", vm.toString(i)), address(token));
        }
        vm.stopBroadcast();
    }

    function split(string memory input, string memory sep)
        internal
        pure
        returns (string[] memory parts)
    {
        bytes memory data = bytes(input);
        bytes1 delim = bytes(sep)[0];
        uint256 count = 1;
        for (uint256 i = 0; i < data.length; i++) {
            if (data[i] == delim) count++;
        }
        parts = new string[](count);
        uint256 part = 0;
        uint256 start = 0;
        for (uint256 i = 0; i <= data.length; i++) {
            if (i == data.length || data[i] == delim) {
                bytes memory chunk = new bytes(i - start);
                for (uint256 j = start; j < i; j++) {
                    chunk[j - start] = data[j];
                }
                parts[part++] = string(chunk);
                start = i + 1;
            }
        }
    }
}
