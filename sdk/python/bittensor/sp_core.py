"""Chain-matched key primitives backed by the in-repo ``bittensor_core`` binding."""

from __future__ import annotations

import bittensor_core as _backend

from bittensor._policy import evaluate_policy
from bittensor._policy_catalog import POLICY_CATALOG

BACKEND = "bittensor_core"

CRYPTO_ED25519 = _backend.CRYPTO_ED25519
CRYPTO_SR25519 = _backend.CRYPTO_SR25519
CRYPTO_ECDSA = _backend.CRYPTO_ECDSA
ECDSA_PUBLIC_KEY_LEN = _backend.ECDSA_PUBLIC_KEY_LEN
ED25519_SIGNATURE_LEN = _backend.ED25519_SIGNATURE_LEN
SR25519_SIGNATURE_LEN = _backend.SR25519_SIGNATURE_LEN
ECDSA_SIGNATURE_LEN = _backend.ECDSA_SIGNATURE_LEN
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
_PROFILES = {
    profile["crypto_type"]: profile for profile in POLICY_CATALOG["transaction"]["profiles"]
}
CLASSICAL_HASHED_CRYPTO_TYPES = tuple(
    code for code, profile in _PROFILES.items() if profile["hashed"]
)
MLDSA_CRYPTO_TYPES = tuple(code for code, profile in _PROFILES.items() if profile["mldsa"])
FALCON512_CRYPTO_TYPES = tuple(
    code for code, profile in _PROFILES.items() if profile["scheme"] == "Falcon512"
)
FALCON1024_CRYPTO_TYPES = tuple(
    code for code, profile in _PROFILES.items() if profile["scheme"] == "Falcon1024"
)
FALCON_CRYPTO_TYPES = tuple(code for code, profile in _PROFILES.items() if profile["falcon"])
PQ_CRYPTO_TYPES = tuple(code for code, profile in _PROFILES.items() if profile["pq"])
FIXED_PQ_CRYPTO_TYPES = tuple(code for code, profile in _PROFILES.items() if profile["fixed_pq"])
HASHED_CRYPTO_TYPES = tuple(code for code, profile in _PROFILES.items() if profile["rotating"])
REGISTERED_CRYPTO_TYPES = tuple(code for code, profile in _PROFILES.items() if profile["protected"])
PQ_PROFILES = {code: profile["profile"] for code, profile in _PROFILES.items() if profile["pq"]}


def pq_lengths(crypto_type: int) -> tuple[int, int]:
    profile = _PROFILES.get(crypto_type)
    if not profile or profile["lengths"] is None:
        raise ValueError("unsupported post-quantum signing scheme")
    return tuple(profile["lengths"])


def is_stateless_pq_descriptor(descriptor: bytes) -> bool:
    return evaluate_policy("transaction.fixed_descriptor", {"descriptor": bytes(descriptor)})


def is_stateless_pq(keypair) -> bool:
    return _PROFILES.get(getattr(keypair, "crypto_type", None), {}).get("fixed_pq", False)


def is_stateless_mldsa_descriptor(descriptor: bytes) -> bool:
    return evaluate_policy(
        "transaction.fixed_descriptor", {"descriptor": bytes(descriptor), "mldsa_only": True}
    )


def is_stateless_mldsa(keypair) -> bool:
    profile = _PROFILES.get(getattr(keypair, "crypto_type", None), {})
    return profile.get("fixed_pq", False) and profile.get("mldsa", False)


def requires_registration(keypair) -> bool:
    return _PROFILES.get(getattr(keypair, "crypto_type", None), {}).get("rotating", False)


def account_crypto_type(crypto_type: int, account_type: str | None = None) -> int:
    return evaluate_policy(
        "transaction.account_type", {"crypto_type": crypto_type, "mode": account_type}
    )


Keypair = _backend.Keypair
KeyfileError = _backend.KeyfileError
WrongPasswordError = _backend.WrongPasswordError

# Native bittensor_core names
verify = _backend.verify
ss58_decode = _backend.ss58_decode
ss58_encode = _backend.ss58_encode
ecdsa_account_id = _backend.ecdsa_account_id
ecdsa_ss58_encode = _backend.ecdsa_ss58_encode
decode_hashed_receiving_address = _backend.decode_hashed_receiving_address
decode_mldsa_receiving_address = _backend.decode_mldsa_receiving_address
encode_mldsa_receiving_address = _backend.encode_mldsa_receiving_address
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

# Native names newer than some installable cores, mapped to the first
# bittensor-core that exports them. A mismatched older core still imports;
# only calling the missing helper fails.
_CORE_HELPERS_SINCE = {
    "verify_strict": "0.1.10",
    "encode_password_for_environment": "0.1.10",
}


def _core_helper(name: str):
    native = getattr(_backend, name, None)
    if native is not None:
        return native
    version = _CORE_HELPERS_SINCE[name]

    def unavailable(*args, **kwargs):
        raise RuntimeError(
            f"bittensor.sp_core.{name} requires bittensor-core>={version}; "
            "rebuild or upgrade bittensor-core together with bittensor."
        )

    unavailable.__name__ = unavailable.__qualname__ = name
    return unavailable


verify_strict = _core_helper("verify_strict")
encode_password_for_environment = _core_helper("encode_password_for_environment")

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
