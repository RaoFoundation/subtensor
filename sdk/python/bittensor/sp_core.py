"""Chain-matched key primitives backed by the in-repo ``bittensor_core`` binding."""

from __future__ import annotations

import bittensor_core as _backend

BACKEND = "bittensor_core"

CRYPTO_ED25519 = _backend.CRYPTO_ED25519
CRYPTO_SR25519 = _backend.CRYPTO_SR25519
CRYPTO_HASHED = _backend.CRYPTO_HASHED
try:
    CRYPTO_HASHED_ED25519 = _backend.CRYPTO_HASHED_ED25519
    CRYPTO_MLDSA65 = _backend.CRYPTO_MLDSA65
    CRYPTO_MLDSA65_FIXED = _backend.CRYPTO_MLDSA65_FIXED
except AttributeError as error:
    raise ImportError(
        "This SDK requires the matching bittensor-core build with ML-DSA v2 account support; "
        "rebuild or upgrade bittensor-core together with bittensor."
    ) from error
HASHED_CRYPTO_TYPES = (CRYPTO_HASHED, CRYPTO_HASHED_ED25519, CRYPTO_MLDSA65)
CLASSICAL_HASHED_CRYPTO_TYPES = (CRYPTO_HASHED, CRYPTO_HASHED_ED25519)
MLDSA_CRYPTO_TYPES = (CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED)
REGISTERED_CRYPTO_TYPES = (*CLASSICAL_HASHED_CRYPTO_TYPES, *MLDSA_CRYPTO_TYPES)


def account_crypto_type(crypto_type: int, account_type: str | None = None) -> int:
    """Compose account mode and signing scheme; None preserves legacy key codes."""
    if crypto_type in MLDSA_CRYPTO_TYPES:
        if account_type is None:
            return crypto_type
        if account_type == "hashed":
            return CRYPTO_MLDSA65
        if account_type == "standard":
            return CRYPTO_MLDSA65_FIXED
        raise ValueError("account type must be standard or hashed")
    if account_type is None and crypto_type in (0, 1, 4, 6):
        return crypto_type
    modes = {
        "standard": {0: 0, 1: 1, 4: 1, 6: 0},
        "hashed": {0: 6, 1: 4, 4: 4, 6: 6},
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
decode_mldsa_receiving_address = _backend.decode_mldsa_receiving_address
encode_mldsa_receiving_address = _backend.encode_mldsa_receiving_address
verify_mldsa_capability = _backend.verify_mldsa_capability
ROLE_ORDER = _backend.ROLE_ORDER
ROLE_MESSAGE = _backend.ROLE_MESSAGE
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


def encode_hashed_receiving_address(descriptor: bytes) -> str:
    """Encode a network-independent descriptor and checksum."""
    return _backend.encode_hashed_receiving_address(descriptor)


def verify_mldsa_authorization(
    proof: bytes, payload: bytes, descriptor: bytes, generation: int, expected_commitment: bytes
) -> bool:
    """Verify a complete transaction proof against an independently trusted record."""
    return bool(
        _backend.verify_mldsa_authorization(
            proof, payload, descriptor, generation, expected_commitment
        )
    )
