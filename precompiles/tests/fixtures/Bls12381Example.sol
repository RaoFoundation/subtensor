// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.21;

/// @notice Single-signature BLS verification with public keys in G1 and signatures in G2.
/// @dev Demonstrates BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_. Keys and signatures
/// use EIP-2537 uncompressed encoding, not the 48/96-byte compressed wire format.
/// This example does not implement aggregate verification or proof-of-possession registration.
contract Bls12381Example {
    bytes private constant DST = "BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";
    bytes private constant MODULUS = hex"1a0111ea397fe69a4b1ba7b6434bacd764774b84f38512bf6730d2a0f6b0f6241eabfffeb153ffffb9feffffffffaaab";
    bytes private constant NEGATIVE_GENERATOR = hex"0000000000000000000000000000000017f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb00000000000000000000000000000000114d1d6855d545a8aa7d76c8cf2e21f267816aef1db507c96655b9d5caac42364e6f38ba0ecb751bad54dcd6b939c2ca";

    function verify(bytes calldata publicKey, bytes calldata signature, bytes calldata message)
        external view returns (bool)
    {
        if (publicKey.length != 128 || signature.length != 256) return false;
        if (keccak256(publicKey) == keccak256(new bytes(128))) return false;
        if (keccak256(signature) == keccak256(new bytes(256))) return false;
        bytes memory point = hashToG2(message);
        (bool ok, bytes memory result) = address(0x0f).staticcall(
            bytes.concat(publicKey, point, NEGATIVE_GENERATOR, signature)
        );
        // Check output length: an unavailable precompile can succeed with empty output.
        return ok && result.length == 32 && abi.decode(result, (uint256)) == 1;
    }

    // RFC 9380 expand_message_xmd(SHA-256), hash_to_field(count=2, m=2, L=64),
    // then map both Fp2 elements and add their cofactor-cleared G2 points.
    function hashToG2(bytes memory message) private view returns (bytes memory) {
        bytes memory dstPrime = bytes.concat(DST, bytes1(uint8(DST.length)));
        bytes32 b0 = sha256(bytes.concat(new bytes(64), message, hex"010000", dstPrime));
        bytes32 previous = sha256(bytes.concat(b0, hex"01", dstPrime));
        bytes memory uniform = abi.encodePacked(previous);
        for (uint8 i = 2; i <= 8; ++i) {
            previous = sha256(bytes.concat(b0 ^ previous, bytes1(i), dstPrime));
            uniform = bytes.concat(uniform, previous);
        }
        bytes memory p0 = invoke(0x11, bytes.concat(field(uniform, 0), field(uniform, 64)), 256);
        bytes memory p1 = invoke(0x11, bytes.concat(field(uniform, 128), field(uniform, 192)), 256);
        return invoke(0x0d, bytes.concat(p0, p1), 256);
    }

    function field(bytes memory uniform, uint256 offset) private view returns (bytes memory) {
        bytes32 high;
        bytes32 low;
        assembly {
            high := mload(add(add(uniform, 32), offset))
            low := mload(add(add(uniform, 64), offset))
        }
        // x^1 mod p reduces the full 512-bit hash-to-field input without truncation.
        bytes memory reduced = invoke(0x05, abi.encodePacked(
            uint256(64), uint256(1), uint256(48), high, low, hex"01", MODULUS
        ), 48);
        return bytes.concat(new bytes(16), reduced);
    }

    function invoke(uint160 target, bytes memory input, uint256 outputLength)
        private view returns (bytes memory)
    {
        (bool ok, bytes memory output) = address(target).staticcall(input);
        require(ok && output.length == outputLength, "precompile failed");
        return output;
    }
}

/// @dev Test-only adapter for CALL, STATICCALL, DELEGATECALL and CALLCODE semantics.
contract Bls12381CallHarness {
    function invoke(address target, bytes memory input, uint256 limit, uint256 mode)
        external returns (bool ok, bytes memory output, uint256 spent)
    {
        uint256 beforeGas = gasleft();
        assembly {
            switch mode
            case 0 { ok := call(limit, target, 0, add(input, 32), mload(input), 0, 0) }
            case 1 { ok := staticcall(limit, target, add(input, 32), mload(input), 0, 0) }
            case 2 { ok := delegatecall(limit, target, add(input, 32), mload(input), 0, 0) }
            case 3 { ok := callcode(limit, target, 0, add(input, 32), mload(input), 0, 0) }
            output := mload(0x40)
            mstore(output, returndatasize())
            returndatacopy(add(output, 32), 0, returndatasize())
            mstore(0x40, and(add(add(output, 63), returndatasize()), not(31)))
        }
        spent = beforeGas - gasleft();
    }
}
