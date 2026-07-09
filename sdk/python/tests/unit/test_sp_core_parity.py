"""Cross-checks that bittensor_core key primitives stay internally consistent."""

from __future__ import annotations

import pytest

bittensor_core = pytest.importorskip("bittensor_core")

CRYPTO_ED25519 = bittensor_core.CRYPTO_ED25519
CRYPTO_SR25519 = bittensor_core.CRYPTO_SR25519
Keypair = bittensor_core.Keypair


def test_alice_address_stable() -> None:
    kp = Keypair.create_from_uri("//Alice")
    assert kp.ss58_address == "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"


def test_sr25519_signatures_cross_verify_between_instances() -> None:
    left = Keypair.create_from_uri("//Alice")
    right = Keypair.create_from_uri("//Alice")
    message = b"cross-verify"
    signature = bytes(left.sign(message))
    assert right.verify(message, signature)


def test_ed25519_signatures_are_byte_stable() -> None:
    mnemonic = (
        "abandon abandon abandon abandon abandon abandon abandon "
        "abandon abandon abandon abandon about"
    )
    left = Keypair.create_from_mnemonic(mnemonic, CRYPTO_ED25519)
    right = Keypair.create_from_mnemonic(mnemonic, CRYPTO_ED25519)
    message = b"deterministic-ed25519"
    assert bytes(left.sign(message)) == bytes(right.sign(message))


def test_keyfile_roundtrip_preserves_signing() -> None:
    original = Keypair.create_from_uri("//Alice")
    data = bytes(bittensor_core.serialized_keypair_to_keyfile_data(original))
    restored = bittensor_core.deserialize_keypair_from_keyfile_data(data)
    message = b"keyfile-roundtrip"
    signature = bytes(restored.sign(message))
    assert original.verify(message, signature)
