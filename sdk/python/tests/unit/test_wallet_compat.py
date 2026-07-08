"""Backwards-compatibility tests for wallet and sp_core API gaps."""

from __future__ import annotations

import pytest

py_sp_core = pytest.importorskip("py_sp_core")
from bittensor import sp_core
from bittensor.wallet import Wallet

Keypair = py_sp_core.Keypair


def test_sp_core_backwards_compat_aliases() -> None:
    assert sp_core.verify_signature is sp_core.verify
    assert sp_core.encode_ss58 is sp_core.ss58_encode
    assert sp_core.decode_ss58 is sp_core.ss58_decode
    assert py_sp_core.verify_signature is py_sp_core.verify
    assert py_sp_core.encode_ss58 is py_sp_core.ss58_encode
    assert py_sp_core.decode_ss58 is py_sp_core.ss58_decode


def test_keypair_sign_accepts_str_and_hex() -> None:
    kp = Keypair.create_from_uri("//Alice")
    message = b"hello"
    sig_bytes = bytes(kp.sign(message))
    sig_str = bytes(kp.sign("hello"))
    sig_hex = bytes(kp.sign("0x68656c6c6f"))
    assert kp.verify(message, sig_bytes)
    assert kp.verify(message, sig_str)
    assert kp.verify(message, sig_hex)


def test_regenerate_coldkey_accepts_bytes_seed(tmp_path) -> None:
    seed = bytes(range(32))
    wallet = Wallet(name="bytes-seed", path=str(tmp_path))
    wallet.regenerate_coldkey(seed=seed, use_password=False, overwrite=True, suppress=True)
    assert wallet.coldkey.ss58_address.startswith("5")


def test_wallet_coldkey_and_hotkeypub_properties(tmp_path) -> None:
    wallet = Wallet(name="props", path=str(tmp_path))
    wallet.create_new_coldkey(use_password=False, overwrite=True, suppress=True)
    wallet.create_new_hotkey(use_password=False, overwrite=True, suppress=True)
    assert wallet.coldkey.ss58_address == wallet.coldkeypub.ss58_address
    assert wallet.hotkey.ss58_address == wallet.hotkeypub.ss58_address
