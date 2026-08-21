// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {Test} from "forge-std/Test.sol";
import {Chutes} from "../src/Chutes.sol";
import {MockUSDC} from "../src/MockUSDC.sol";
import {RailsPortal} from "../src/RailsPortal.sol";
import {MockMailbox} from "./mocks/MockMailbox.sol";

/// CHUTES rebasing display, hub-only index, sell path, and the Solidity
/// envelope encoders replayed against the Rust golden vectors
/// (`common/src/rails.rs::envelope_golden_vectors`).
contract ChutesTest is Test {
    uint32 constant BASE_DOMAIN = 8453;
    uint32 constant HUB_DOMAIN = 964;
    uint16 constant NETUID = 64;

    MockMailbox mailbox;
    MockUSDC usdc;
    RailsPortal portal;
    Chutes chutes;

    address owner = makeAddr("owner");
    address user = makeAddr("user");
    address other = makeAddr("other");
    /// Anvil account #0: the recipient pinned in the Rust golden vectors.
    address golden = 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266;

    bytes32 hubSender = bytes32(uint256(uint160(makeAddr("hubSystem"))));
    bytes32 hubGateway = bytes32(uint256(uint160(makeAddr("hubGateway"))));

    function setUp() public {
        mailbox = new MockMailbox(BASE_DOMAIN);
        usdc = new MockUSDC();
        portal = new RailsPortal(
            owner, address(usdc), address(mailbox), HUB_DOMAIN, 0, hubGateway
        );
        chutes = new Chutes(owner, "Chutes", "CHUTES");
        vm.startPrank(owner);
        chutes.configureHub(
            address(mailbox), HUB_DOMAIN, hubSender, hubGateway, address(portal), NETUID, 0
        );
        portal.setToken(address(chutes), true);
        vm.stopPrank();
    }

    function mintShares(address to, uint64 shares, uint64 indexE9) internal {
        mailbox.deliver(address(chutes), HUB_DOMAIN, hubSender, abi.encode(to, shares, indexE9));
    }

    function testHubMintAndRebasingBalance() public {
        mintShares(user, 100_000_000_000, 1_000_000_000);
        assertEq(chutes.sharesOf(user), 100_000_000_000);
        assertEq(chutes.balanceOf(user), 100_000_000_000);

        // Heartbeat: index rises, balance ticks up with zero transfers.
        mailbox.deliver(
            address(chutes),
            HUB_DOMAIN,
            hubSender,
            abi.encode(address(0), uint64(0), uint64(1_500_000_000))
        );
        assertEq(chutes.sharesOf(user), 100_000_000_000);
        assertEq(chutes.balanceOf(user), 150_000_000_000);
        assertEq(chutes.totalSupply(), 150_000_000_000);
    }

    function testIndexOnlySettableByHubMessage() public {
        bytes memory body = abi.encode(address(0), uint64(0), uint64(2_000_000_000));
        vm.expectRevert("untrusted hub");
        mailbox.deliver(address(chutes), HUB_DOMAIN, bytes32(uint256(1)), body);

        vm.expectRevert("not mailbox");
        chutes.handle(HUB_DOMAIN, hubSender, body);

        assertEq(chutes.indexE9(), 1_000_000_000);
    }

    function testTransfersMoveShares() public {
        mintShares(user, 100_000_000_000, 2_000_000_000);
        assertEq(chutes.balanceOf(user), 200_000_000_000);

        vm.prank(user);
        chutes.transfer(other, 100_000_000_000); // display units
        assertEq(chutes.sharesOf(user), 50_000_000_000);
        assertEq(chutes.sharesOf(other), 50_000_000_000);
        assertEq(chutes.balanceOf(other), 100_000_000_000);
    }

    function testSellBurnsSharesAndDispatches() public {
        mintShares(user, 100_000_000_000, 1_000_000_000);
        vm.prank(user);
        chutes.sell(40_000_000_000, 0);

        assertEq(chutes.sharesOf(user), 60_000_000_000);
        assertEq(chutes.totalShares(), 60_000_000_000);
        assertEq(mailbox.lastDestination(), HUB_DOMAIN);
        assertEq(mailbox.lastRecipient(), hubGateway);
        (uint64 amount, bytes memory env) = abi.decode(mailbox.lastBody(), (uint64, bytes));
        assertEq(amount, 40_000_000_000);
        assertGt(env.length, 0);
    }

    function testAssignNonceOnlyRegisteredTokens() public {
        vm.prank(user);
        vm.expectRevert("not token");
        portal.assignNonce();
    }

    /// The Solidity encoders must reproduce the Rust golden vectors
    /// byte-for-byte: buy at nonce 0, sell at nonce 1, sharing one counter.
    function testGoldenEnvelopes() public {
        usdc.mint(golden, 500_000_000_000);
        vm.startPrank(golden);
        usdc.approve(address(portal), type(uint256).max);
        (, uint64 buyNonce) = portal.buy(500_000_000_000, NETUID, 1);
        vm.stopPrank();
        assertEq(buyNonce, 0);
        (uint64 buyAmount, bytes memory buyEnv) =
            abi.decode(mailbox.lastBody(), (uint64, bytes));
        assertEq(buyAmount, 500_000_000_000);
        assertEq(
            buyEnv,
            hex"0103000000000088526a740000000000000000000000000000000000000000000000000000000000000000000000044000f39fd6e51aad88f6f4ce6ab8827279cfffb922660100000000000000052100000000000000000000"
        );

        mintShares(golden, 2_000_000_000, 1_000_000_000);
        vm.prank(golden);
        chutes.sell(2_000_000_000, 1_000_000_000);
        (uint64 sellAmount, bytes memory sellEnv) =
            abi.decode(mailbox.lastBody(), (uint64, bytes));
        assertEq(sellAmount, 2_000_000_000);
        assertEq(
            sellEnv,
            hex"0102400000943577000000000000000000000000000000000000000000000000000000000000000000000000054000f39fd6e51aad88f6f4ce6ab8827279cfffb922660000000000ca9a3b00000000052100000100000000000000"
        );
        assertEq(portal.nextNonce(), 2);
    }
}
