// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Test} from "forge-std/Test.sol";
import {CanonicalShareToken} from "../src/CanonicalShareToken.sol";
import {Gateway} from "../src/Gateway.sol";
import {MockUSDC} from "../src/MockUSDC.sol";
import {RailsPortal} from "../src/RailsPortal.sol";
import {IUSD_RAILS_ADDRESS} from "../src/interfaces/IUsdRails.sol";
import {MockMailbox} from "./mocks/MockMailbox.sol";
import {MockUsdRails} from "./mocks/MockUsdRails.sol";

/// End-to-end inbound flow over mock mailboxes: portal (Base) locks USDC and
/// dispatches; gateway (Bittensor EVM) mints canonical USD backing to the PSM
/// escrow and forwards the envelope to the 0x814 stub.
contract GatewayPortalTest is Test {
    uint32 constant BASE_DOMAIN = 8453;
    uint32 constant BT_DOMAIN = 964;

    MockMailbox baseMailbox;
    MockMailbox btMailbox;
    MockUSDC usdc;
    RailsPortal portal;
    CanonicalShareToken canonicalUsd;
    Gateway gateway;
    MockUsdRails rails;

    address owner = makeAddr("owner");
    address alice = makeAddr("alice");
    address psmEscrow = makeAddr("psmEscrow");

    function setUp() public {
        baseMailbox = new MockMailbox(BASE_DOMAIN);
        btMailbox = new MockMailbox(BT_DOMAIN);

        // Etch the recording stub at the precompile address.
        MockUsdRails impl = new MockUsdRails();
        vm.etch(IUSD_RAILS_ADDRESS, address(impl).code);
        rails = MockUsdRails(IUSD_RAILS_ADDRESS);

        usdc = new MockUSDC();
        canonicalUsd = new CanonicalShareToken("Canonical USD", "cUSD", owner);
        gateway = new Gateway(owner, address(btMailbox), address(canonicalUsd), psmEscrow);
        portal = new RailsPortal(
            owner, address(usdc), address(baseMailbox), BT_DOMAIN, 0, addr32(address(gateway))
        );

        vm.startPrank(owner);
        canonicalUsd.setMinterLimits(address(gateway), 1_000_000, 0);
        gateway.setTrustedSender(
            BASE_DOMAIN, addr32(address(portal)), Gateway.SenderKind.UsdPortal
        );
        vm.stopPrank();

        usdc.mint(alice, 500);
    }

    function addr32(address a) internal pure returns (bytes32) {
        return bytes32(uint256(uint160(a)));
    }

    function testDepositLocksAndDispatchesWithSequentialNonce() public {
        bytes memory prefix = hex"01aa";
        vm.startPrank(alice);
        usdc.approve(address(portal), 500);
        (, uint64 nonce) = portal.deposit(500, prefix);
        vm.stopPrank();

        assertEq(nonce, 0);
        assertEq(portal.nextNonce(), 1);
        assertEq(usdc.balanceOf(alice), 0);
        assertEq(usdc.balanceOf(address(portal)), 500);
        assertEq(baseMailbox.lastDestination(), BT_DOMAIN);
        assertEq(baseMailbox.lastRecipient(), addr32(address(gateway)));
        // The dispatched envelope is the prefix plus the little-endian nonce.
        (, bytes memory env) = abi.decode(baseMailbox.lastBody(), (uint64, bytes));
        assertEq(env, abi.encodePacked(prefix, hex"0000000000000000"));
    }

    function testBuyLocksAndDispatchesEnvelope() public {
        vm.startPrank(alice);
        usdc.approve(address(portal), 500);
        (, uint64 nonce) = portal.buy(500, 64, 1);
        vm.stopPrank();

        assertEq(nonce, 0);
        assertEq(usdc.balanceOf(address(portal)), 500);
        assertEq(baseMailbox.lastRecipient(), addr32(address(gateway)));
        (uint64 amount, bytes memory env) = abi.decode(baseMailbox.lastBody(), (uint64, bytes));
        assertEq(amount, 500);
        assertGt(env.length, 0);
    }

    function testInboundDeliveryMintsBackingAndCallsPrecompile() public {
        bytes memory envelope = hex"01beef";
        bytes memory body = abi.encode(uint64(500), envelope);

        btMailbox.deliver(address(gateway), BASE_DOMAIN, addr32(address(portal)), body);

        assertEq(canonicalUsd.balanceOf(psmEscrow), 500);
        assertEq(rails.gatewayExecuteCalls(), 1);
        assertEq(rails.lastAmount(), 500);
        assertEq(rails.lastEnvelope(), envelope);
        assertEq(rails.lastCaller(), address(gateway));
    }

    function testZeroAmountPingSkipsMint() public {
        // The walking-skeleton ping: amount 0, no backing minted.
        bytes memory body = abi.encode(uint64(0), bytes(hex"01"));
        btMailbox.deliver(address(gateway), BASE_DOMAIN, addr32(address(portal)), body);
        assertEq(canonicalUsd.totalSupply(), 0);
        assertEq(rails.gatewayExecuteCalls(), 1);
        assertEq(rails.lastAmount(), 0);
    }

    function testUntrustedSenderReverts() public {
        bytes memory body = abi.encode(uint64(1), bytes(hex"01"));
        vm.expectRevert("untrusted sender");
        btMailbox.deliver(address(gateway), BASE_DOMAIN, bytes32(uint256(0xbad)), body);
    }

    function testMintWindowExhaustionRevertsDelivery() public {
        // Delivery must revert (so the relayer retries) when the gateway's
        // mint window cannot cover the deposit.
        vm.prank(owner);
        canonicalUsd.setMinterLimits(address(gateway), 100, 0);
        bytes memory body = abi.encode(uint64(500), bytes(hex"01"));
        vm.expectRevert("mint rate limited");
        btMailbox.deliver(address(gateway), BASE_DOMAIN, addr32(address(portal)), body);
    }

    function testHubReleasePaysOutLockedCollateral() public {
        // Lock collateral first.
        bytes memory prefix = hex"01aa";
        vm.startPrank(alice);
        usdc.approve(address(portal), 500);
        portal.deposit(500, prefix);
        vm.stopPrank();

        bytes32 releaser = addr32(makeAddr("hubSystem"));
        vm.prank(owner);
        portal.setHubReleaser(releaser);

        baseMailbox.deliver(
            address(portal), BT_DOMAIN, releaser, abi.encode(alice, uint64(200))
        );
        assertEq(usdc.balanceOf(alice), 200);
        assertEq(usdc.balanceOf(address(portal)), 300);
    }
}
