"""Key selection survives wallet creation, overrides, reload and recovery."""

from __future__ import annotations

import json

import pytest
from typer.testing import CliRunner

from bittensor import wallets
from bittensor.cli.main import app
from bittensor.sp_core import (
    CRYPTO_ED25519,
    CRYPTO_HASHED,
    CRYPTO_MLDSA,
    CRYPTO_SR25519,
    HASHED_CRYPTO_TYPES,
    Keypair,
)
from bittensor.wallet import Wallet


@pytest.fixture
def wallet_path(tmp_path, monkeypatch):
    path = tmp_path / "wallets"
    monkeypatch.setenv("BT_WALLET_PATH", str(path))
    monkeypatch.setenv("BT_WALLET", "schemes")
    monkeypatch.setenv("BT_HOTKEY", "default")
    monkeypatch.setenv("BT_NETWORK", "finney")
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    return path


def invoke(*args):
    return CliRunner().invoke(app, ["--yes", "--json", "wallet", *args])


@pytest.mark.parametrize(
    ("name", "code"),
    [
        ("ed", CRYPTO_ED25519),
        ("ed25519", CRYPTO_ED25519),
        ("sr", CRYPTO_SR25519),
        ("sr25519", CRYPTO_SR25519),
        ("hashed", CRYPTO_HASHED),
        ("mldsa", CRYPTO_MLDSA),
        ("ms", CRYPTO_MLDSA),
    ],
)
def test_cli_selection_applies_to_both_keys_and_mnemonic_recovery(wallet_path, name, code):
    result = invoke("create", "--crypto-type", name, "--no-password")
    assert result.exit_code == 0, result.output
    details = json.loads(result.output)
    wallet = Wallet("schemes", path=str(wallet_path))
    for role in ("coldkey", "hotkey"):
        key = getattr(wallet, role)
        public = getattr(wallet, role + "pub")
        assert len(details[role + "_mnemonic"].split()) == (24 if code == CRYPTO_MLDSA else 12)
        recovered = Keypair.create_from_mnemonic(details[role + "_mnemonic"], code)
        assert key.crypto_type == public.crypto_type == recovered.crypto_type == code
        assert key.ss58_address == public.ss58_address == recovered.ss58_address
        assert details[role + "_crypto_type"] == wallets.format_crypto_type(code)
        if code in HASHED_CRYPTO_TYPES:
            assert bytes(key.hashed_descriptor) == bytes(recovered.hashed_descriptor)
        else:
            payload = b"wallet selection survives signing and recovery"
            assert recovered.verify(payload, key.sign(payload))


@pytest.mark.parametrize(
    ("cold", "hot", "cold_code", "hot_code"),
    [("ed", "sr", CRYPTO_ED25519, CRYPTO_SR25519), ("hashed", "ed", CRYPTO_HASHED, CRYPTO_ED25519)],
)
def test_cli_explicit_hotkey_override(wallet_path, cold, hot, cold_code, hot_code):
    result = invoke("create", "--type", cold, "--hotkey-crypto-type", hot, "--no-password")
    assert result.exit_code == 0, result.output
    wallet = Wallet("schemes", path=str(wallet_path))
    assert wallet.coldkey.crypto_type == cold_code
    assert wallet.hotkey.crypto_type == hot_code


@pytest.mark.parametrize("code", [CRYPTO_ED25519, CRYPTO_SR25519, CRYPTO_HASHED, CRYPTO_MLDSA])
def test_sdk_create_uses_selected_scheme_for_both_keys(wallet_path, code):
    wallet = wallets.create(
        name="schemes",
        path=str(wallet_path),
        coldkey_crypto_type=code,
        use_password=False,
        on_mnemonic=lambda *_: None,
    )
    assert wallet.coldkey.crypto_type == wallet.hotkey.crypto_type == code


def test_sdk_explicit_ed25519_override_is_not_treated_as_missing(wallet_path):
    wallet = wallets.create(
        name="schemes",
        path=str(wallet_path),
        coldkey_crypto_type=CRYPTO_HASHED,
        hotkey_crypto_type=CRYPTO_ED25519,
        use_password=False,
        on_mnemonic=lambda *_: None,
    )
    assert wallet.coldkey.crypto_type == CRYPTO_HASHED
    assert wallet.hotkey.crypto_type == CRYPTO_ED25519


@pytest.mark.parametrize("name", ["unknown"])
def test_unsupported_scheme_fails_before_creating_keys(wallet_path, name):
    result = invoke("create", "--crypto-type", name, "--no-password")
    assert result.exit_code != 0
    assert "unknown crypto type" in result.output
    assert not wallet_path.exists()
