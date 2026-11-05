"""Parity tests for the in-repo ``bittensor_core.Keypair`` API."""

from __future__ import annotations

import pytest

bittensor_core = pytest.importorskip("bittensor_core")

CRYPTO_ED25519 = bittensor_core.CRYPTO_ED25519
CRYPTO_SR25519 = bittensor_core.CRYPTO_SR25519
Keypair = bittensor_core.Keypair


def test_create_from_uri_alice_address() -> None:
    kp = Keypair.create_from_uri("//Alice")
    assert kp.ss58_address == "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
    assert kp.crypto_type == CRYPTO_SR25519


def test_public_only_from_ss58_address() -> None:
    full = Keypair.create_from_uri("//Bob")
    public = Keypair(ss58_address=full.ss58_address)
    assert public.ss58_address == full.ss58_address
    assert bytes(public.public_key) == bytes(full.public_key)
    assert public.crypto_type == CRYPTO_SR25519


def test_sign_and_verify_roundtrip() -> None:
    kp = Keypair.create_from_uri("//Alice")
    message = b"hello substrate"
    signature = bytes(kp.sign(message))
    assert len(signature) == 64
    assert kp.verify(message, signature)


def test_public_only_verify() -> None:
    full = Keypair.create_from_uri("//Alice")
    message = b"verify-only"
    signature = bytes(full.sign(message))
    public = Keypair(ss58_address=full.ss58_address)
    assert public.verify(message, signature)


def test_generate_and_create_from_mnemonic() -> None:
    mnemonic = Keypair.generate_mnemonic()
    kp = Keypair.create_from_mnemonic(mnemonic)
    assert kp.ss58_address.startswith("5")
    assert len(bytes(kp.public_key)) == 32


def test_ed25519_encrypt_decrypt_roundtrip() -> None:
    mnemonic = (
        "abandon abandon abandon abandon abandon abandon abandon "
        "abandon abandon abandon abandon about"
    )
    kp = Keypair.create_from_mnemonic(mnemonic, CRYPTO_ED25519)
    message = b"secret message for testing"
    ciphertext = bytes(kp.encrypt(message))
    assert len(ciphertext) == len(message) + 48
    assert bytes(kp.decrypt(ciphertext)) == message


def test_encrypt_for_recipient() -> None:
    mnemonic = (
        "abandon abandon abandon abandon abandon abandon abandon "
        "abandon abandon abandon abandon about"
    )
    recipient = Keypair.create_from_mnemonic(mnemonic, CRYPTO_ED25519)
    sender = Keypair.create_from_mnemonic(Keypair.generate_mnemonic(), CRYPTO_ED25519)
    message = b"hello recipient"
    ciphertext = bytes(Keypair.encrypt_for(recipient.ss58_address, message, CRYPTO_ED25519))
    assert bytes(recipient.decrypt(ciphertext)) == message
    with pytest.raises(ValueError, match="decryption failed"):
        sender.decrypt(ciphertext)


def test_encrypt_for_rejects_sr25519_recipient() -> None:
    sr25519_recipient = Keypair.create_from_uri("//Alice")
    with pytest.raises(ValueError, match="ed25519"):
        Keypair.encrypt_for(sr25519_recipient.ss58_address, b"hello", CRYPTO_SR25519)


# secp256k1 generator point G, compressed.
_ECDSA_PUBKEY = bytes.fromhex("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")


def test_ecdsa_crypto_type_is_multisignature_index() -> None:
    from bittensor import sp_core

    assert sp_core.CRYPTO_ECDSA == bittensor_core.CRYPTO_ECDSA == 2


def test_key_and_signature_lengths() -> None:
    from bittensor import sp_core

    assert sp_core.ECDSA_PUBLIC_KEY_LEN == 33
    assert sp_core.ED25519_SIGNATURE_LEN == 64
    assert sp_core.SR25519_SIGNATURE_LEN == 64
    assert sp_core.ECDSA_SIGNATURE_LEN == 65


def test_ecdsa_ss58_is_blake2_256_of_compressed_pubkey() -> None:
    import hashlib

    from bittensor import sp_core

    account = hashlib.blake2b(_ECDSA_PUBKEY, digest_size=32).digest()
    assert sp_core.ecdsa_account_id(_ECDSA_PUBKEY) == account
    assert sp_core.ecdsa_ss58_encode(_ECDSA_PUBKEY) == sp_core.ss58_encode(account, 42)
    assert sp_core.ecdsa_ss58_encode(_ECDSA_PUBKEY, 0) == sp_core.ss58_encode(account, 0)
    assert sp_core.ss58_decode(sp_core.ecdsa_ss58_encode(_ECDSA_PUBKEY)) == account


@pytest.mark.parametrize(
    "bad", [_ECDSA_PUBKEY[1:], b"\x04" + _ECDSA_PUBKEY[1:], b"\x04" + bytes(64)]
)
def test_ecdsa_ss58_rejects_non_compressed_keys(bad: bytes) -> None:
    from bittensor import sp_core

    with pytest.raises(ValueError):
        sp_core.ecdsa_ss58_encode(bad)
