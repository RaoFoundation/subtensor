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
    CRYPTO_FALCON512 = _backend.CRYPTO_FALCON512
    CRYPTO_FALCON512_HASHED = _backend.CRYPTO_FALCON512_HASHED
    CRYPTO_FALCON1024 = _backend.CRYPTO_FALCON1024
    CRYPTO_FALCON1024_HASHED = _backend.CRYPTO_FALCON1024_HASHED
except AttributeError as error:
    raise ImportError(
        "This SDK requires the matching bittensor-core build with post-quantum account support; "
        "rebuild or upgrade bittensor-core together with bittensor."
    ) from error
CLASSICAL_HASHED_CRYPTO_TYPES = (CRYPTO_HASHED, CRYPTO_HASHED_ED25519)
MLDSA_CRYPTO_TYPES = (CRYPTO_MLDSA65, CRYPTO_MLDSA65_FIXED)
FALCON512_CRYPTO_TYPES = (CRYPTO_FALCON512, CRYPTO_FALCON512_HASHED)
FALCON1024_CRYPTO_TYPES = (CRYPTO_FALCON1024, CRYPTO_FALCON1024_HASHED)
FALCON_CRYPTO_TYPES = (*FALCON512_CRYPTO_TYPES, *FALCON1024_CRYPTO_TYPES)
PQ_CRYPTO_TYPES = (*MLDSA_CRYPTO_TYPES, *FALCON_CRYPTO_TYPES)
FIXED_PQ_CRYPTO_TYPES = (CRYPTO_MLDSA65_FIXED, CRYPTO_FALCON512, CRYPTO_FALCON1024)
HASHED_CRYPTO_TYPES = (
    *CLASSICAL_HASHED_CRYPTO_TYPES,
    CRYPTO_MLDSA65,
    CRYPTO_FALCON512_HASHED,
    CRYPTO_FALCON1024_HASHED,
)
# Legacy name for the variable-proof wire family; use requires_registration()
# for lifecycle decisions because fixed PQ keys have no authority registry.
REGISTERED_CRYPTO_TYPES = (*CLASSICAL_HASHED_CRYPTO_TYPES, *PQ_CRYPTO_TYPES)
PQ_PROFILES = {
    CRYPTO_MLDSA65: 4,
    CRYPTO_MLDSA65_FIXED: 5,
    CRYPTO_FALCON512_HASHED: 6,
    CRYPTO_FALCON512: 7,
    CRYPTO_FALCON1024_HASHED: 8,
    CRYPTO_FALCON1024: 9,
}


def pq_lengths(crypto_type: int) -> tuple[int, int]:
    """Canonical public-key and signature byte lengths; never infer from input."""
    if crypto_type in MLDSA_CRYPTO_TYPES:
        return 1952, 3309
    if crypto_type in FALCON512_CRYPTO_TYPES:
        return 897, 666
    if crypto_type in FALCON1024_CRYPTO_TYPES:
        return 1793, 1280
    raise ValueError("unsupported post-quantum signing scheme")


def is_stateless_pq_descriptor(descriptor: bytes) -> bool:
    return len(descriptor) == 66 and descriptor[0] == 2 and descriptor[1] in (5, 7, 9)


def is_stateless_pq(keypair) -> bool:
    return getattr(keypair, "crypto_type", None) in FIXED_PQ_CRYPTO_TYPES


def is_stateless_mldsa_descriptor(descriptor: bytes) -> bool:
    """Fixed MS authenticates its key without an authority registry."""
    return len(descriptor) == 66 and descriptor[:2] == b"\x02\x05"


def is_stateless_mldsa(keypair) -> bool:
    return getattr(keypair, "crypto_type", None) == CRYPTO_MLDSA65_FIXED


def requires_registration(keypair) -> bool:
    """Only rotating hashed accounts require authority registration."""
    return getattr(keypair, "crypto_type", None) in HASHED_CRYPTO_TYPES


def account_crypto_type(crypto_type: int, account_type: str | None = None) -> int:
    """Compose account mode and signing scheme; None preserves legacy key codes."""
    for fixed, hashed in (
        (CRYPTO_MLDSA65_FIXED, CRYPTO_MLDSA65),
        (CRYPTO_FALCON512, CRYPTO_FALCON512_HASHED),
        (CRYPTO_FALCON1024, CRYPTO_FALCON1024_HASHED),
    ):
        if crypto_type not in (fixed, hashed):
            continue
        if account_type is None:
            return crypto_type
        if account_type == "hashed":
            return hashed
        if account_type == "standard":
            return fixed
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


def verify_hashed_authorization(
    proof: bytes, payload: bytes, descriptor: bytes, generation: int, expected_commitment: bytes
) -> bool:
    """Verify an SR/ED rotating proof against independently trusted authority."""
    return bool(
        _backend.verify_hashed_authorization(
            proof, payload, descriptor, generation, expected_commitment
        )
    )


def verify_mldsa_authorization(
    proof: bytes, payload: bytes, descriptor: bytes, generation: int, expected_commitment: bytes
) -> bool:
    """Verify a complete transaction proof against an independently trusted record."""
    return bool(
        _backend.verify_mldsa_authorization(
            proof, payload, descriptor, generation, expected_commitment
        )
    )
