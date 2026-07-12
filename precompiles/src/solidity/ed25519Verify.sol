// SPDX-License-Identifier: GPL-3.0
pragma solidity ^0.8.0;

address constant IED25519VERIFY_ADDRESS = 0x0000000000000000000000000000000000000402;

interface IEd25519Verify {
    /**
     * @dev Verifies Ed25519 signature using provided message and public key.
     *
     * @param message The 32-byte signature payload message.
     * @param publicKey 32-byte public key matching to private key used to sign the message.
     * @param r The Ed25519 signature commitment (first 32 bytes).
     * @param s The Ed25519 signature response (second 32 bytes).
     * @return bool Returns true if the signature is valid for the given message and public key, false otherwise.
     */
    function verify(bytes32 message, bytes32 publicKey, bytes32 r, bytes32 s) external pure returns (bool);
}
