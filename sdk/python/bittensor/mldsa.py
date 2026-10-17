"""Owner-approved ML-DSA message keys, resolved from finalized chain state.

A proof never grants its own authority. Verifiers supply a freshly resolved
capability for the claimed account and intended audience.
"""

from __future__ import annotations

from dataclasses import dataclass

from ._generated.calls import Call
from .sp_core import (
    MLDSA_CRYPTO_TYPES,
    ROLE_MESSAGE,
    ss58_decode,
    verify_mldsa_capability,
)

MESSAGE_DOMAIN = b"bittensor/ms/v2/user-message\x00"
PROOF_LENGTH = 5336


def supports_fixed_profile(profiles) -> bool:
    """Accept metadata byte arrays as hex, bytes, or decoded integer sequences.

    The native metadata adapter renders [u8; N] constants as hex. Malformed or
    absent advertisements fail closed before receiving setup or private-key use.
    """
    try:
        if isinstance(profiles, str):
            if not profiles.startswith("0x") or len(profiles) > 514:
                return False
            profiles = bytes.fromhex(profiles[2:])
        elif isinstance(profiles, (bytes, bytearray, list, tuple)) and len(profiles) <= 256:
            profiles = bytes(profiles)
        else:
            return False
    except (ValueError, TypeError):
        return False
    return 5 in profiles


def _bytes(value) -> bytes:
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


@dataclass(frozen=True)
class MessageCapability:
    owner: str
    genesis_hash: bytes
    epoch: int
    commitment: bytes
    audience: bytes
    expires_at: int
    finalized_block: int

    def __post_init__(self):
        if (
            len(bytes(ss58_decode(self.owner))) != 32
            or len(self.genesis_hash) != 32
            or len(self.commitment) != 64
            or self.commitment == bytes(64)
            or not 0 < self.epoch < 2**64
            or not 0 < len(self.audience) <= 128
            or not 0 <= self.finalized_block <= self.expires_at < 2**64
        ):
            raise ValueError("invalid or expired ML-DSA message capability")


async def message_capability(substrate, owner: str, audience: bytes) -> MessageCapability:
    """Read one consistent finalized snapshot; do not indefinitely cache it."""
    block = await substrate.finalized_block_number()
    block_hash = await substrate.block_hash(block)
    if not block_hash:
        raise ValueError("finalized block hash is unavailable")
    if (
        await substrate.query("AdminUtils", "HashedAccountsEnabled", block_hash=block_hash)
        is not True
    ):
        raise ValueError("ML-DSA authorization is disabled")
    record = await substrate.query(
        "HashedAccounts", "ActiveCapabilities", [owner, "Message"], block_hash=block_hash
    )
    epoch = await substrate.query(
        "HashedAccounts", "CapabilityEpochs", [owner, "Message"], block_hash=block_hash
    )
    if not record or int(record["epoch"]) != int(epoch):
        raise ValueError(
            "no active ML-DSA message capability; install an owner-approved message key first"
        )
    scope = record["scope"]
    if not isinstance(scope, dict) or "Message" not in scope:
        raise ValueError("capability does not authorize messages")
    authorized_audience = _bytes(scope["Message"]["audience"])
    if authorized_audience != audience:
        raise ValueError("message capability does not authorize this audience")
    return MessageCapability(
        owner,
        _bytes(await substrate.block_hash(0)),
        int(epoch),
        _bytes(record["key_commitment"]),
        authorized_audience,
        int(record["expires_at"]),
        block,
    )


def install_message_capability(keypair, *, epoch: int, expires_at: int, audience: bytes) -> Call:
    """Build an explicit grant. Epoch must be the finalized tombstone plus one."""
    if keypair.crypto_type not in MLDSA_CRYPTO_TYPES or not 0 < len(audience) <= 128:
        raise ValueError("ML-DSA wallet and 1..128 audience bytes required")
    if not 0 < epoch < 2**64 or not 0 < expires_at < 2**64:
        raise ValueError("epoch and expiry must be positive u64 values")
    return Call(
        "HashedAccounts",
        "install_capability",
        {
            "expected_epoch": epoch,
            "key_commitment": bytes(keypair.mldsa_capability_commitment(ROLE_MESSAGE, epoch)),
            "expires_at": expires_at,
            "scope": {"Message": {"audience": audience}},
        },
    )


def sign_payload(keypair, payload: bytes, capability: MessageCapability) -> bytes:
    if keypair.crypto_type not in MLDSA_CRYPTO_TYPES:
        raise ValueError("message capability requires an ML-DSA signing key")
    if (
        bytes(keypair.mldsa_capability_commitment(ROLE_MESSAGE, capability.epoch))
        != capability.commitment
    ):
        raise ValueError("wallet message key does not match the finalized capability")
    return bytes(
        keypair.sign_mldsa_capability(
            ROLE_MESSAGE,
            capability.epoch,
            capability.genesis_hash,
            payload,
            owner=bytes(ss58_decode(capability.owner)),
        )
    )


def verify_payload(proof: bytes, payload: bytes, capability: MessageCapability) -> bool:
    if len(proof) != PROOF_LENGTH:
        return False
    return bool(
        verify_mldsa_capability(
            proof,
            payload,
            bytes(ss58_decode(capability.owner)),
            capability.genesis_hash,
            ROLE_MESSAGE,
            capability.epoch,
            capability.commitment,
        )
    )


def user_message_payload(message: bytes, capability: MessageCapability) -> bytes:
    # Fixed little-endian lengths are deliberately independent of metadata.
    return (
        MESSAGE_DOMAIN
        + len(capability.audience).to_bytes(2, "little")
        + capability.audience
        + len(message).to_bytes(8, "little")
        + message
    )
