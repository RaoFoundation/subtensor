"""Golden tests for native keyfile encryption and wallet flows."""

from __future__ import annotations

import json
import os
import stat
from pathlib import Path

import pytest

from bittensor.keyfiles import Keyfile, Keypair
from bittensor.sp_core import (
    decrypt_keyfile_data,
    deserialize_keypair_from_keyfile_data,
    encrypt_keyfile_data,
    keyfile_data_is_encrypted_legacy,
    keyfile_data_is_encrypted_nacl,
    serialized_keypair_to_keyfile_data,
)
from bittensor.wallet import Wallet

pytest.importorskip("bittensor_core")

ALICE = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
TEST_MNEMONIC = (
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
)
TEST_PASSWORD = "golden-test-password"


def test_alice_uri_address() -> None:
    kp = Keypair.create_from_uri("//Alice")
    assert kp.ss58_address == ALICE


def test_alice_sign_and_verify() -> None:
    kp = Keypair.create_from_uri("//Alice")
    message = b"golden-signing-payload"
    signature = bytes(kp.sign(message))
    assert len(signature) == 64
    assert kp.verify(message, signature)


def test_keyfile_json_roundtrip() -> None:
    original = Keypair.create_from_mnemonic(TEST_MNEMONIC)
    data = bytes(serialized_keypair_to_keyfile_data(original))
    restored = deserialize_keypair_from_keyfile_data(data)
    assert restored.ss58_address == original.ss58_address
    assert restored.crypto_type == original.crypto_type
    assert bytes(restored.public_key) == bytes(original.public_key)


def test_nacl_encrypt_decrypt_roundtrip() -> None:
    original = Keypair.create_from_uri("//Alice")
    plaintext = bytes(serialized_keypair_to_keyfile_data(original))
    encrypted = bytes(encrypt_keyfile_data(plaintext, TEST_PASSWORD))
    assert keyfile_data_is_encrypted_nacl(encrypted)
    decrypted = bytes(decrypt_keyfile_data(encrypted, TEST_PASSWORD))
    restored = deserialize_keypair_from_keyfile_data(decrypted)
    assert restored.ss58_address == original.ss58_address
    signature = bytes(restored.sign(b"after-decrypt"))
    assert restored.verify(b"after-decrypt", signature)


def test_legacy_fernet_fixture_roundtrip() -> None:
    import base64
    import hashlib

    try:
        from cryptography.fernet import Fernet
    except ImportError:
        pytest.skip("cryptography not installed")

    legacy_salt = b"Iguesscyborgslikemyselfhaveatendencytobeparanoidaboutourorigins"
    key = hashlib.pbkdf2_hmac("sha256", TEST_PASSWORD.encode(), legacy_salt, 10_000_000)
    fernet_key = base64.urlsafe_b64encode(key)
    plaintext = bytes(serialized_keypair_to_keyfile_data(Keypair.create_from_uri("//Alice")))
    encrypted = Fernet(fernet_key).encrypt(plaintext)

    assert keyfile_data_is_encrypted_legacy(encrypted)
    decrypted = bytes(decrypt_keyfile_data(encrypted, TEST_PASSWORD))
    restored = deserialize_keypair_from_keyfile_data(decrypted)
    assert restored.ss58_address == ALICE


def test_wallet_create_sign_flow(tmp_path: Path) -> None:
    wallet = Wallet(name="golden", path=str(tmp_path))
    wallet.create_new_coldkey(
        use_password=True,
        overwrite=True,
        suppress=True,
        coldkey_password=TEST_PASSWORD,
    )
    wallet.create_new_hotkey(use_password=False, overwrite=True, suppress=True)

    assert wallet.coldkeypub_file.exists_on_device()
    assert wallet.hotkey_file.exists_on_device()
    assert wallet.coldkey_file.is_encrypted()
    assert not wallet.hotkey_file.is_encrypted()

    coldkey = wallet.get_coldkey(password=TEST_PASSWORD)
    hotkey = wallet.get_hotkey()
    message = b"wallet e2e signing"
    assert len(bytes(coldkey.sign(message))) == 64
    assert len(bytes(hotkey.sign(message))) == 64

    coldkey_pub = json.loads(wallet.coldkeypub_file._read_data())
    assert coldkey_pub["ss58Address"] == wallet.coldkeypub.ss58_address


def test_keyfile_class_roundtrip(tmp_path: Path) -> None:
    path = tmp_path / "coldkey"
    keyfile = Keyfile(path)
    keypair = Keypair.create_from_uri("//Alice")
    keyfile.set_keypair(keypair, encrypt=True, overwrite=True, password=TEST_PASSWORD)
    restored = keyfile.get_keypair(password=TEST_PASSWORD)
    assert restored.ss58_address == keypair.ss58_address


def test_keyfile_written_owner_only(tmp_path: Path) -> None:
    """Key material is written 0600 (and its dir 0700) even under a wide-open umask."""
    path = tmp_path / "wallets" / "golden" / "hotkeys" / "hotkey"
    keyfile = Keyfile(path)
    keypair = Keypair.create_from_uri("//Alice")

    previous_umask = os.umask(0o000)
    try:
        keyfile.set_keypair(keypair, encrypt=False, overwrite=True)
    finally:
        os.umask(previous_umask)

    assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert stat.S_IMODE(path.parent.stat().st_mode) == 0o700

    # Overwriting a file that already exists with loose permissions clamps it.
    os.chmod(path, 0o644)
    keyfile.set_keypair(keypair, encrypt=False, overwrite=True)
    assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert keyfile.get_keypair().ss58_address == keypair.ss58_address
