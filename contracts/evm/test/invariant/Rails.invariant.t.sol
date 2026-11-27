// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";

import {Portal} from "../../src/Portal.sol";
import {Vault} from "../../src/Vault.sol";
import {Wire} from "../../src/Wire.sol";
import {MockStable} from "../../src/mocks/MockStable.sol";
import {AlphaToken} from "../../src/tokens/AlphaToken.sol";
import {CanonicalTao} from "../../src/tokens/CanonicalTao.sol";
import {Fixture} from "../Fixture.sol";
import {MockMailbox} from "../mocks/MockMailbox.sol";

/// @notice Payers, and a model of the hub's books for this spoke.
///
/// The model settles orders at fixed prices (a TAO is 400 dollars, a share of subnet 64 is
/// half a TAO) and refunds some at random. It keeps what the hub keeps: the tUSD the bridge
/// has outstanding and the supply of each canonical token that it let the spoke mint. Its
/// answers reach the portal in any order, as the transport may deliver them.
contract Handler is Test {
    uint32 internal constant HUB_DOMAIN = 964;
    bytes32 internal constant GATEWAY = bytes32(uint256(0x6A7E));
    uint16 internal constant CHUTES = 64;
    uint64 internal constant TAO_PRICE = 400;
    uint64 internal constant SHARES_PER_TAO = 2;

    MockStable internal stable;
    MockMailbox internal mailbox;
    Vault internal vault;
    Portal internal portal;
    CanonicalTao internal tao;
    AlphaToken internal chutes;
    address[3] internal actors;

    // ---- the hub's books
    uint256 public tusdOutstanding;
    uint256 public taoSupply;
    uint256 public chutesSupply;
    /// The next order of the portal that the hub settles.
    uint256 public nextOrder;
    /// The hub's messages, and whether each was delivered.
    bytes[] public messages;
    mapping(uint256 => bool) public delivered;

    // ---- what the invariants need to know of the path
    uint256 public releases;
    uint256 public principalAdded;

    constructor(
        MockStable stable_,
        MockMailbox mailbox_,
        Vault vault_,
        Portal portal_,
        AlphaToken chutes_,
        address[3] memory actors_
    ) {
        stable = stable_;
        mailbox = mailbox_;
        vault = vault_;
        portal = portal_;
        tao = portal_.tao();
        chutes = chutes_;
        actors = actors_;
        // The token's registration was the hub's first message.
        messages.push("");
        delivered[0] = true;
    }

    function messageCount() external view returns (uint256) {
        return messages.length;
    }

    // ---------------------------------------------------------------- payers

    function deposit(uint256 actor, uint256 amount, uint8 kind) external {
        address payer = actors[actor % 3];
        amount = bound(amount, 1, 1_000_000e6);
        stable.mint(payer, amount);
        vm.startPrank(payer);
        stable.approve(address(vault), amount);
        vault.deposit(amount, _destination(uint8(bound(kind, 1, 5)), payer), 0, type(uint64).max);
        vm.stopPrank();
    }

    function sellTao(uint256 actor, uint256 amount, uint8 kind) external {
        address payer = actors[actor % 3];
        uint256 held = tao.balanceOf(payer);
        if (held == 0) return;
        kind = uint8(bound(kind, 0, 5));
        if (kind == Wire.TO_TAO) kind = Wire.TO_STABLE;
        vm.prank(payer);
        tao.sell(uint64(bound(amount, 1, held)), _destination(kind, payer), 0, type(uint64).max);
    }

    function sellChutes(uint256 actor, uint256 amount, uint8 kind) external {
        address payer = actors[actor % 3];
        uint256 held = chutes.balanceOf(payer);
        if (held == 0) return;
        kind = uint8(bound(kind, 0, 5));
        if (kind == Wire.TO_ALPHA) kind = Wire.TO_TAO;
        vm.prank(payer);
        chutes.sell(uint64(bound(amount, 1, held)), _destination(kind, payer), 0, type(uint64).max);
    }

    function transferChutes(uint256 from, uint256 to, uint256 amount) external {
        address sender = actors[from % 3];
        amount = bound(amount, 0, chutes.balanceOf(sender));
        vm.prank(sender);
        chutes.transfer(actors[to % 3], amount);
    }

    function wait(uint256 time) external {
        skip(bound(time, 1, 12 hours));
    }

    // ---------------------------------------------------------------- the hub

    /// The hub settles the portal's next order, or refunds it.
    function settle(uint256 seed) external {
        if (nextOrder == mailbox.sentCount()) return;
        (,, bytes memory body,) = mailbox.sent(nextOrder);
        Wire.Order memory order = abi.decode(body, (Wire.Order));
        assertEq(order.nonce, nextOrder, "the portal's orders are in sequence");
        nextOrder++;

        // What the order pays with reaches the hub as TAO or tUSD.
        uint256 tusd;
        uint256 taoIn;
        if (order.source == Wire.ASSET_STABLE) {
            tusdOutstanding += order.amount;
            tusd = order.amount;
        } else if (order.source == Wire.ASSET_TAO) {
            taoSupply -= order.amount;
            taoIn = order.amount;
        } else {
            chutesSupply -= order.amount;
            taoIn = order.amount / SHARES_PER_TAO;
        }

        bool refund = seed % 4 == 0;
        uint256 owed = order.destination == Wire.TO_STABLE ? tusd + taoIn * TAO_PRICE : 0;
        // The bridge releases only the dollars it brought in.
        if (owed > tusdOutstanding) refund = true;
        if (refund) {
            _refund(order);
            return;
        }

        if (order.destination == Wire.TO_STABLE) {
            tusdOutstanding -= owed;
            _credit(order, Wire.ASSET_STABLE, 0, owed, 0, false);
        } else if (order.destination == Wire.TO_TAO) {
            uint256 out = taoIn + tusd / TAO_PRICE;
            taoSupply += out;
            _credit(order, Wire.ASSET_TAO, 0, out, 0, false);
        } else if (order.destination == Wire.TO_ALPHA) {
            uint256 shares = (taoIn + tusd / TAO_PRICE) * SHARES_PER_TAO;
            chutesSupply += shares;
            // The rate grows as the hub's vault earns.
            uint128 rate = uint128(1e18 + (block.timestamp % 1e6) * 1e12);
            principalAdded += shares * rate / 1e18;
            _credit(order, Wire.ASSET_ALPHA, CHUTES, shares, rate, false);
        }
        // Credits and assets on Bittensor leave nothing to pay out here.
    }

    /// The transport delivers one of the hub's messages, whichever comes first.
    function deliver(uint256 index) external {
        index = bound(index, 0, messages.length - 1);
        bytes memory body = messages[index];
        if (delivered[index]) {
            // A message is carried out once.
            vm.expectRevert(abi.encodeWithSelector(Portal.AlreadyProcessed.selector, uint64(index)));
            if (index == 0) {
                body = abi.encode(Wire.KIND_REGISTER_TOKEN, uint64(0), CHUTES, address(0), "", "");
            }
            mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body);
            return;
        }
        // A release beyond the vault's allowance waits with the transport.
        try mailbox.deliver(portal, HUB_DOMAIN, GATEWAY, body) {
            delivered[index] = true;
            (,, uint8 asset) = abi.decode(body, (uint8, uint64, uint8));
            if (asset == Wire.ASSET_STABLE) releases++;
        } catch {}
    }

    function _refund(Wire.Order memory order) private {
        if (order.source == Wire.ASSET_STABLE) {
            tusdOutstanding -= order.amount;
            _credit(order, Wire.ASSET_STABLE, 0, order.amount, 0, true);
        } else if (order.source == Wire.ASSET_TAO) {
            taoSupply += order.amount;
            _credit(order, Wire.ASSET_TAO, 0, order.amount, 0, true);
        } else {
            chutesSupply += order.amount;
            principalAdded += uint256(order.amount) * chutes.rate() / 1e18;
            _credit(order, Wire.ASSET_ALPHA, CHUTES, order.amount, 0, true);
        }
    }

    function _credit(
        Wire.Order memory order,
        uint8 asset,
        uint16 netuid,
        uint256 amount,
        uint128 rate,
        bool refund
    ) private {
        if (amount == 0) return;
        address to = refund ? order.payer : address(uint160(uint256(order.recipient)));
        messages.push(
            abi.encode(
                Wire.KIND_CREDIT,
                uint64(messages.length),
                asset,
                netuid,
                to,
                uint64(amount),
                true,
                order.nonce,
                refund,
                rate
            )
        );
    }

    function _destination(uint8 kind, address payer)
        private
        pure
        returns (Wire.Destination memory)
    {
        if (kind <= Wire.TO_ALPHA) {
            return Wire.Destination(
                kind,
                kind == Wire.TO_ALPHA ? CHUTES : 0,
                bytes32(uint256(uint160(payer))),
                address(0)
            );
        }
        return Wire.Destination(
            kind,
            kind == Wire.TO_CREDITS ? CHUTES : 0,
            keccak256("on Bittensor"),
            kind == Wire.TO_CREDITS ? payer : address(0)
        );
    }
}

contract RailsInvariantTest is Fixture {
    Handler internal handler;
    AlphaToken internal chutes;
    address[3] internal actors;

    function setUp() public override {
        super.setUp();
        chutes = registerToken(CHUTES, "Chutes", "CHUTES");
        actors = [alice, bob, stranger];
        handler = new Handler(stable, mailbox, vault, portal, chutes, actors);
        targetContract(address(handler));
    }

    /// What the hub has not yet read of the orders, and what the portal has not yet
    /// carried out of the answers: stablecoin, TAO and CHUTES in flight, with 9 decimals.
    function _inFlight()
        internal
        view
        returns (uint256[3] memory ordered, uint256[3] memory answered)
    {
        for (uint256 i = handler.nextOrder(); i < mailbox.sentCount(); ++i) {
            (,, bytes memory body,) = mailbox.sent(i);
            Wire.Order memory order = abi.decode(body, (Wire.Order));
            ordered[order.source] += order.amount;
        }
        for (uint256 i = 1; i < handler.messageCount(); ++i) {
            if (handler.delivered(i)) continue;
            (,, uint8 asset,,, uint64 amount) =
                abi.decode(handler.messages(i), (uint8, uint64, uint8, uint16, address, uint64));
            answered[asset] += amount;
        }
    }

    /// Every dollar the hub counts as outstanding, every dollar it told the portal to pay
    /// and every dollar of an order it has not read yet is in the vault. Beyond that the
    /// vault holds only what releases rounded down.
    function invariant_TheVaultCoversEveryDollarOwed() public view {
        (uint256[3] memory ordered, uint256[3] memory answered) = _inFlight();
        uint256 owed = handler.tusdOutstanding() + ordered[0] + answered[0];
        uint256 held = stable.balanceOf(address(vault)) * 1000;
        assertGe(held, owed, "the vault is short");
        assertLe(held - owed, handler.releases() * 999, "the vault holds dollars nobody is owed");
    }

    /// The tokens that exist, plus those the hub has ordered minted, are exactly those the
    /// hub counts, less the burns it has not read yet.
    function invariant_TokensMatchTheHubsSupply() public view {
        (uint256[3] memory ordered, uint256[3] memory answered) = _inFlight();
        assertEq(tao.totalSupply() + answered[1] + ordered[1], handler.taoSupply(), "TAO");
        assertEq(chutes.totalSupply() + answered[2] + ordered[2], handler.chutesSupply(), "CHUTES");
    }

    /// The portal passes funds on and keeps none.
    function invariant_ThePortalHoldsNothing() public view {
        assertEq(stable.balanceOf(address(portal)), 0);
        assertEq(tao.balanceOf(address(portal)), 0);
        assertEq(chutes.balanceOf(address(portal)), 0);
        assertEq(address(portal).balance, 0);
        assertEq(portal.nextOrder(), mailbox.sentCount());
    }

    /// Holders' principals never sum to more than the worth the hub's mints added.
    function invariant_PrincipalNeverExceedsWhatWasMinted() public view {
        uint256 total;
        for (uint256 i = 0; i < 3; ++i) {
            total += chutes.principalOf(actors[i]);
            if (chutes.balanceOf(actors[i]) == 0) assertEq(chutes.principalOf(actors[i]), 0);
        }
        assertLe(total, handler.principalAdded());
    }
}
