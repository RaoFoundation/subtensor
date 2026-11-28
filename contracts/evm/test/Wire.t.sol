// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";

import {Wire} from "../src/Wire.sol";

/// @notice Exposes the library's calldata functions to the tests.
contract WireHarness {
    function check(Wire.Destination calldata destination) external pure {
        Wire.check(destination);
    }

    function kind(bytes calldata body) external pure returns (uint8) {
        return Wire.kind(body);
    }

    function decodeOrder(bytes calldata body) external pure returns (Wire.Order memory) {
        return Wire.decodeOrder(body);
    }

    function decodeCredit(bytes calldata body) external pure returns (Wire.Credit memory) {
        return Wire.decodeCredit(body);
    }

    function decodeRegisterToken(bytes calldata body)
        external
        pure
        returns (Wire.RegisterToken memory)
    {
        return Wire.decodeRegisterToken(body);
    }
}

/// @notice The vectors of `golden_vectors` in `precompiles/src/rails_wire.rs`: the hub
/// writes and reads exactly these bytes.
contract WireTest is Test {
    WireHarness internal wire = new WireHarness();

    bytes internal constant ORDER = hex"0000000000000000000000000000000000000000000000000000000000000001"
        hex"0000000000000000000000000000000000000000000000000000000000000007"
        hex"000000000000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        hex"0000000000000000000000000000000000000000000000000000000000000000"
        hex"0000000000000000000000000000000000000000000000000000000000000000"
        hex"0000000000000000000000000000000000000000000000000000003a35294400"
        hex"0000000000000000000000000000000000000000000000000000000000000002"
        hex"0000000000000000000000000000000000000000000000000000000000000040"
        hex"000000000000000000000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        hex"0000000000000000000000000000000000000000000000000000000000000000"
        hex"00000000000000000000000000000000000000000000000000000000000003e8"
        hex"00000000000000000000000000000000000000000000000000000000713fb300";

    bytes internal constant CREDIT = hex"0000000000000000000000000000000000000000000000000000000000000001"
        hex"0000000000000000000000000000000000000000000000000000000000000009"
        hex"0000000000000000000000000000000000000000000000000000000000000002"
        hex"0000000000000000000000000000000000000000000000000000000000000040"
        hex"000000000000000000000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        hex"00000000000000000000000000000000000000000000000000000000075bcd15"
        hex"0000000000000000000000000000000000000000000000000000000000000001"
        hex"0000000000000000000000000000000000000000000000000000000000000007"
        hex"0000000000000000000000000000000000000000000000000000000000000000"
        hex"0000000000000000000000000000000000000000000000000f43fc2c04ee0000";

    bytes internal constant REGISTER_TOKEN = hex"0000000000000000000000000000000000000000000000000000000000000002"
        hex"0000000000000000000000000000000000000000000000000000000000000003"
        hex"0000000000000000000000000000000000000000000000000000000000000040"
        hex"0000000000000000000000006464646464646464646464646464646464646464"
        hex"00000000000000000000000000000000000000000000000000000000000000c0"
        hex"0000000000000000000000000000000000000000000000000000000000000100"
        hex"0000000000000000000000000000000000000000000000000000000000000006"
        hex"4368757465730000000000000000000000000000000000000000000000000000"
        hex"0000000000000000000000000000000000000000000000000000000000000006"
        hex"4348555445530000000000000000000000000000000000000000000000000000";

    function test_AnOrderEncodesToTheBytesTheHubReads() public pure {
        Wire.Order memory order = Wire.Order({
            version: Wire.ORDER_VERSION,
            nonce: 7,
            payer: 0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa,
            source: Wire.ASSET_STABLE,
            sourceNetuid: 0,
            amount: 250_000_000_000,
            destination: Wire.TO_ALPHA,
            destinationNetuid: 64,
            recipient: bytes32(uint256(uint160(0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB))),
            beneficiary: address(0),
            minOut: 1_000,
            deadline: 1_900_000_000
        });
        assertEq(Wire.encode(order), ORDER);
        assertEq(Wire.encode(order).length, 384);
    }

    function test_AnOrderDecodesFromTheBytesTheHubReads() public view {
        Wire.Order memory order = wire.decodeOrder(ORDER);
        assertEq(order.nonce, 7);
        assertEq(order.payer, 0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa);
        assertEq(order.amount, 250_000_000_000);
        assertEq(order.destination, Wire.TO_ALPHA);
        assertEq(order.destinationNetuid, 64);
        assertEq(order.deadline, 1_900_000_000);
    }

    function test_ACreditDecodesFromTheBytesTheHubWrites() public view {
        assertEq(wire.kind(CREDIT), Wire.KIND_CREDIT);
        Wire.Credit memory credit = wire.decodeCredit(CREDIT);
        assertEq(credit.nonce, 9);
        assertEq(credit.asset, Wire.ASSET_ALPHA);
        assertEq(credit.netuid, 64);
        assertEq(credit.to, 0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB);
        assertEq(credit.amount, 123_456_789);
        assertTrue(credit.answersOrder);
        assertEq(credit.order, 7);
        assertFalse(credit.refund);
        assertEq(credit.rate, 1.1e18);
    }

    function test_ATokenRegistrationDecodesFromTheBytesTheHubWrites() public view {
        assertEq(wire.kind(REGISTER_TOKEN), Wire.KIND_REGISTER_TOKEN);
        Wire.RegisterToken memory register = wire.decodeRegisterToken(REGISTER_TOKEN);
        assertEq(register.nonce, 3);
        assertEq(register.netuid, 64);
        assertEq(register.token, 0x6464646464646464646464646464646464646464);
        assertEq(register.name, "Chutes");
        assertEq(register.symbol, "CHUTES");
    }

    function test_AValueAboveItsTypeDoesNotDecode() public {
        // A nonce of 2^64.
        bytes memory dirty = CREDIT;
        dirty[32 + 23] = 0x01;
        vm.expectRevert();
        wire.decodeCredit(dirty);
        // A kind of 256.
        dirty = CREDIT;
        dirty[30] = 0x01;
        vm.expectRevert();
        wire.kind(dirty);
    }

    function test_DestinationsTheHubCannotDecodeAreRefused() public {
        bytes32 someone = bytes32(uint256(uint160(address(0xB0B))));
        wire.check(Wire.Destination(Wire.TO_STABLE, 0, someone, address(0)));
        wire.check(Wire.Destination(Wire.TO_ALPHA, 64, someone, address(0)));
        wire.check(Wire.Destination(Wire.TO_CREDITS, 51, keccak256("service"), address(0xB0B)));
        wire.check(Wire.Destination(Wire.TO_NATIVE_TAO, 0, keccak256("account"), address(0)));

        // An address with bytes above its twenty.
        bytes32 wide = bytes32(uint256(1) << 160);
        vm.expectRevert(abi.encodeWithSelector(Wire.NotAnAddress.selector, wide));
        wire.check(Wire.Destination(Wire.TO_TAO, 0, wide, address(0)));
        // A kind that does not exist.
        vm.expectRevert(abi.encodeWithSelector(Wire.UnknownDestination.selector, 6));
        wire.check(Wire.Destination(6, 0, someone, address(0)));
        // A netuid or a beneficiary where the destination has none.
        vm.expectRevert(Wire.UnusedField.selector);
        wire.check(Wire.Destination(Wire.TO_TAO, 64, someone, address(0)));
        vm.expectRevert(Wire.UnusedField.selector);
        wire.check(Wire.Destination(Wire.TO_NATIVE_TUSD, 0, someone, address(0xB0B)));
    }

    /// The hub would settle such an order, and the spoke could never pay it out.
    function test_NothingIsOrderedForTheZeroAddress() public {
        for (uint8 kind = Wire.TO_STABLE; kind <= Wire.TO_NATIVE_TAO; ++kind) {
            uint16 netuid = kind == Wire.TO_ALPHA || kind == Wire.TO_CREDITS ? 64 : 0;
            address beneficiary = kind == Wire.TO_CREDITS ? address(0xB0B) : address(0);
            vm.expectRevert(Wire.NoRecipient.selector);
            wire.check(Wire.Destination(kind, netuid, bytes32(0), beneficiary));
        }
        // Credits for nobody.
        vm.expectRevert(Wire.NoRecipient.selector);
        wire.check(Wire.Destination(Wire.TO_CREDITS, 51, keccak256("service"), address(0)));
    }

    function testFuzz_AnOrderRoundTrips(Wire.Order memory order) public view {
        Wire.Order memory decoded = wire.decodeOrder(Wire.encode(order));
        assertEq(abi.encode(decoded), abi.encode(order));
    }
}
