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
        bytes32 salt = keccak256(bytes(vm.envOr("RAILS_SALT", string("rails-local-v1"))));

        vm.startBroadcast();
        // Skip creation when code already sits at the CREATE2 address, so a
        // re-run against a live chain re-wires instead of reverting with a
        // create collision.
        address usdc = vm.computeCreate2Address(salt, keccak256(type(MockUSDC).creationCode));
        if (usdc.code.length == 0) {
            usdc = address(new MockUSDC{salt: salt}());
        }

        // Consumed by the rig manifest writer.
        console2.log("MOCK_USDC", usdc);
        console2.log("PORTAL", deployPortal(salt, owner, usdc));

        // One share token per "Name|SYMBOL" entry; the salt folds in the
        // symbol so every token gets a stable CREATE2 address.
        string[] memory entries = split(vm.envOr("RAILS_TOKENS", string("Chutes|CHUTES")), ",");
        for (uint256 i = 0; i < entries.length; i++) {
            string[] memory parts = split(entries[i], "|");
            require(parts.length == 2, "bad RAILS_TOKENS entry");
            console2.log(
                string.concat("TOKEN_", vm.toString(i)),
                deployToken(salt, owner, parts[0], parts[1])
            );
        }
        vm.stopBroadcast();
    }

    function deployPortal(bytes32 salt, address owner, address usdc)
        internal
        returns (address portal)
    {
        address mailbox = vm.envAddress("RAILS_MAILBOX");
        uint32 hubDomain = uint32(vm.envUint("RAILS_HUB_DOMAIN"));
        uint32 usdAssetId = uint32(vm.envOr("RAILS_USD_ASSET_ID", uint256(0)));
        bytes32 gateway32 = bytes32(uint256(uint160(vm.envAddress("RAILS_GATEWAY"))));
        portal = vm.computeCreate2Address(
            salt,
            keccak256(
                abi.encodePacked(
                    type(RailsPortal).creationCode,
                    abi.encode(owner, usdc, mailbox, hubDomain, usdAssetId, gateway32)
                )
            )
        );
        if (portal.code.length == 0) {
            portal = address(
                new RailsPortal{salt: salt}(owner, usdc, mailbox, hubDomain, usdAssetId, gateway32)
            );
        }
    }

    function deployToken(bytes32 salt, address owner, string memory name, string memory symbol)
        internal
        returns (address token)
    {
        bytes32 tokenSalt = keccak256(abi.encodePacked(salt, symbol));
        token = vm.computeCreate2Address(
            tokenSalt,
            keccak256(abi.encodePacked(type(Chutes).creationCode, abi.encode(owner, name, symbol)))
        );
        if (token.code.length == 0) {
            token = address(new Chutes{salt: tokenSalt}(owner, name, symbol));
        }
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
