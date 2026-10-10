"""Chain-matched key primitives backed by the in-repo ``bittensor_core`` binding."""

from __future__ import annotations

import bittensor_core as _backend

BACKEND = "bittensor_core"

CRYPTO_ED25519 = _backend.CRYPTO_ED25519
CRYPTO_SR25519 = _backend.CRYPTO_SR25519
CRYPTO_HASHED = _backend.CRYPTO_HASHED
CRYPTO_MLDSA = _backend.CRYPTO_MLDSA
try:
    CRYPTO_HASHED_ED25519 = _backend.CRYPTO_HASHED_ED25519
    CRYPTO_MLDSA_STANDARD = _backend.CRYPTO_MLDSA_STANDARD
except AttributeError as error:
    raise ImportError(
        "This SDK requires the matching bittensor-core build with composable account modes; "
        "rebuild or upgrade bittensor-core together with bittensor."
    ) from error
# Legacy name: these all use registered-account authorization, including fixed-key MS.
HASHED_CRYPTO_TYPES = (CRYPTO_HASHED, CRYPTO_MLDSA, CRYPTO_HASHED_ED25519, CRYPTO_MLDSA_STANDARD)
MLDSA_CRYPTO_TYPES = (CRYPTO_MLDSA, CRYPTO_MLDSA_STANDARD)
CLASSICAL_HASHED_CRYPTO_TYPES = (CRYPTO_HASHED, CRYPTO_HASHED_ED25519)


def account_crypto_type(crypto_type: int, account_type: str | None = None) -> int:
    """Compose account mode and signing scheme; None preserves legacy key codes."""
    if account_type is None:
        return crypto_type
    modes = {
        "standard": {0: 0, 1: 1, 4: 1, 5: 7, 6: 0, 7: 7},
        "hashed": {0: 6, 1: 4, 4: 4, 5: 5, 6: 6, 7: 5},
    }
    try:
        return modes[account_type][crypto_type]
    except KeyError:
        raise ValueError(
            "account type must be standard or hashed with a supported signing scheme"
        ) from None


Keypair = _backend.Keypair
KeyfileError = _backend.KeyfileError
WrongPasswordError = _backend.WrongPasswordError

# Native bittensor_core names
verify = _backend.verify
ss58_decode = _backend.ss58_decode
ss58_encode = _backend.ss58_encode
decode_hashed_receiving_address = _backend.decode_hashed_receiving_address
decrypt_keyfile_data = _backend.decrypt_keyfile_data
deserialize_keypair_from_keyfile_data = _backend.deserialize_keypair_from_keyfile_data
encrypt_keyfile_data = _backend.encrypt_keyfile_data
get_password_from_environment = _backend.get_password_from_environment
keyfile_data_encryption_method = _backend.keyfile_data_encryption_method
keyfile_data_is_encrypted = _backend.keyfile_data_is_encrypted
keyfile_data_is_encrypted_ansible = _backend.keyfile_data_is_encrypted_ansible
keyfile_data_is_encrypted_legacy = _backend.keyfile_data_is_encrypted_legacy
keyfile_data_is_encrypted_nacl = _backend.keyfile_data_is_encrypted_nacl
save_password_to_environment = _backend.save_password_to_environment
serialized_keypair_to_keyfile_data = _backend.serialized_keypair_to_keyfile_data

# Backwards-compatible aliases (pre-migration bittensor.sp_core API)
verify_signature = verify
decode_ss58 = ss58_decode
encode_ss58 = ss58_encode


def sign(message: bytes, *, mnemonic: str, crypto_type: int = CRYPTO_SR25519) -> bytes:
    """Sign raw bytes with a key derived from ``mnemonic``."""
    keypair = _backend.Keypair.create_from_mnemonic(mnemonic, crypto_type)
    return bytes(keypair.sign(message))


def encode_hashed_receiving_address(descriptor: bytes, genesis_hash: bytes | None = None) -> str:
    """Encode a network-independent address, including with older native bindings.

    Keep the optional legacy argument for callers migrating from network-bound
    addresses. Reserved zero bytes preserve the existing checked wire format.
    """
    return _backend.encode_hashed_receiving_address(descriptor, bytes(32))
