// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {Test} from "forge-std/Test.sol";

import {Portal} from "../src/Portal.sol";
import {Vault} from "../src/Vault.sol";
import {Wire} from "../src/Wire.sol";
import {IPortal} from "../src/interfaces/IPortal.sol";
import {MockStable} from "../src/mocks/MockStable.sol";
import {AlphaToken} from "../src/tokens/AlphaToken.sol";
import {CanonicalTao} from "../src/tokens/CanonicalTao.sol";
import {MockMailbox} from "./mocks/MockMailbox.sol";

/// @notice A spoke as it is deployed: the stablecoin, the transport, the timelock that owns
/// everything, the vault and the portal behind its proxy. The test contract plays the hub.
abstract contract Fixture is Test {
    uint32 internal constant HUB_DOMAIN = 964;
    uint32 internal constant LOCAL_DOMAIN = 8453;
    bytes32 internal constant GATEWAY = bytes32(uint256(0x6A7E));
    uint256 internal constant TIMELOCK_DELAY = 48 hours;
    /// One dollar of the stablecoin, and one unit of the hub's 9 decimals.
    uint256 internal constant USDC = 1e6;
    uint64 internal constant UNIT = 1e9;
    uint16 internal constant CHUTES = 64;

    address internal multisig = makeAddr("multisig");
    address internal guardian = makeAddr("guardian");
    address internal alice = makeAddr("alice");
    address internal bob = makeAddr("bob");
    address internal stranger = makeAddr("stranger");

    MockStable internal stable;
    MockMailbox internal mailbox;
    TimelockController internal timelock;
    Vault internal vault;
    Portal internal portal;
    CanonicalTao internal tao;

    /// The nonce of the hub's next message.
    uint64 internal hubNonce;

    function setUp() public virtual {
        vm.warp(1_800_000_000);
        stable = new MockStable();
        mailbox = new MockMailbox(LOCAL_DOMAIN);
        address[] memory holders = new address[](1);
        holders[0] = multisig;
        timelock = new TimelockController(TIMELOCK_DELAY, holders, holders, address(0));

        Portal implementation = new Portal();
        // The vault is built for the portal's address, which is the next contract after it.
        address proxy = vm.computeCreateAddress(address(this), vm.getNonce(address(this)) + 1);
        vault = new Vault(
            stable, IPortal(proxy), address(timelock), guardian, uint128(1_000_000 * USDC), 1 days
        );
        portal = Portal(
            address(
                new ERC1967Proxy(
                    address(implementation),
                    abi.encodeCall(
                        Portal.initialize,
                        (address(timelock), guardian, mailbox, HUB_DOMAIN, GATEWAY, vault)
                    )
                )
            )
        );
        assertEq(address(portal), proxy);
        tao = portal.tao();
    }

    // ---------------------------------------------------------------- destinations

    function toStable(address to) internal pure returns (Wire.Destination memory) {
        return Wire.Destination(Wire.TO_STABLE, 0, bytes32(uint256(uint160(to))), address(0));
    }

    function toTao(address to) internal pure returns (Wire.Destination memory) {
        return Wire.Destination(Wire.TO_TAO, 0, bytes32(uint256(uint160(to))), address(0));
    }

    function toAlpha(uint16 netuid, address to) internal pure returns (Wire.Destination memory) {
        return Wire.Destination(Wire.TO_ALPHA, netuid, bytes32(uint256(uint160(to))), address(0));
    }

    function toCredits(uint16 netuid, bytes32 service, address beneficiary)
        internal
        pure
        returns (Wire.Destination memory)
    {
        return Wire.Destination(Wire.TO_CREDITS, netuid, service, beneficiary);
    }

    // ---------------------------------------------------------------- payers

    /// `payer` deposits `amount` of the stablecoin for `destination`.
    function deposit(address payer, uint256 amount, Wire.Destination memory destination)
        internal
        returns (uint64 nonce)
    {
        stable.mint(payer, amount);
        vm.startPrank(payer);
        stable.approve(address(vault), amount);
        nonce = vault.deposit(amount, destination, 0, type(uint64).max);
        vm.stopPrank();
    }

    /// The order the portal sent last.
    function lastOrder() internal view returns (Wire.Order memory) {
        return abi.decode(mailbox.lastBody(), (Wire.Order));
    }

    // ---------------------------------------------------------------- the hub

    function creditBody(
        uint64 nonce,
        uint8 asset,
        uint16 netuid,
        address to,
        uint64 amount,
        bool answersOrder,
        uint64 order,
        bool refund,
        uint128 rate
    ) internal pure returns (bytes memory) {
        return abi.encode(
            Wire.KIND_CREDIT, nonce, asset, netuid, to, amount, answersOrder, order, refund, rate
        );
    }

    /// The hub pays `amount` of an asset to `to`, not in answer to an order.
    function credit(uint8 asset, uint16 netuid, address to, uint64 amount, uint128 rate)
        internal
        returns (uint64 nonce)
    {
        nonce = hubNonce++;
        mailbox.deliver(
            portal,
            HUB_DOMAIN,
            GATEWAY,
            creditBody(nonce, asset, netuid, to, amount, false, 0, false, rate)
        );
    }

    /// The hub has the alpha token of `netuid` created, and lists it where it will be.
    function registerToken(uint16 netuid, string memory name, string memory symbol)
        internal
        returns (AlphaToken)
    {
        mailbox.deliver(
            portal,
            HUB_DOMAIN,
            GATEWAY,
            abi.encode(
                Wire.KIND_REGISTER_TOKEN,
                hubNonce++,
                netuid,
                portal.alphaTokenAddress(netuid),
                name,
                symbol
            )
        );
        return portal.alphaTokens(netuid);
    }

    // ---------------------------------------------------------------- the owner

    /// The multisig has the timelock call `target`, after the delay.
    function timelocked(address target, bytes memory data) internal {
        vm.startPrank(multisig);
        timelock.schedule(target, 0, data, bytes32(0), bytes32(0), TIMELOCK_DELAY);
        skip(TIMELOCK_DELAY);
        timelock.execute(target, 0, data, bytes32(0), bytes32(0));
        vm.stopPrank();
    }
}
