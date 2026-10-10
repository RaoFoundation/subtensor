"""Key selection survives wallet creation, overrides, reload and recovery."""

from __future__ import annotations

import json
import sys

import pytest
from typer.testing import CliRunner

from bittensor import wallets
from bittensor.cli.main import app
from bittensor.cli.prompt import run_app
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
    ("command", "option"),
    [
        ("create", "--crypto-type"),
        ("create", "--type"),
        ("create", "--hotkey-crypto-type"),
        ("regen-coldkey", "--crypto-type"),
        ("regen-hotkey", "--crypto-type"),
        ("verify", "--crypto-type"),
    ],
)
def test_missing_crypto_type_lists_choices(wallet_path, monkeypatch, capsys, command, option):
    monkeypatch.setattr(sys, "argv", ["btcli", "wallet", command, option])
    with pytest.raises(SystemExit) as error:
        run_app(app)
    assert error.value.code == 2
    output = " ".join(capsys.readouterr().err.replace("│", " ").split())
    assert "requires an argument" in output
    for choice in ("sr (sr25519)", "ed (ed25519)", "hashed", "ms (mldsa / ml-dsa)"):
        assert choice in output
    assert not wallet_path.exists()


def test_missing_other_option_does_not_list_crypto_types(wallet_path, monkeypatch, capsys):
    monkeypatch.setattr(sys, "argv", ["btcli", "wallet", "create", "--n-words"])
    with pytest.raises(SystemExit) as error:
        run_app(app)
    assert error.value.code == 2
    output = capsys.readouterr().err
    assert "requires an argument" in output
    assert "Choose sr" not in output
    assert not wallet_path.exists()


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


@pytest.mark.parametrize("scheme,code", [("ed", 0), ("sr", 1), ("hashed", 4), ("ms", 5)])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_cli_recovers_exported_private_key(wallet_path, scheme, code, role):
    from bittensor.sp_core import serialized_keypair_to_keyfile_data

    original = Keypair.create_from_seed(bytes([31]) * 32, code)
    exported = json.loads(serialized_keypair_to_keyfile_data(original))["privateKey"]
    args = [f"regen-{role}", "--crypto-type", scheme, "--private-key", exported]
    if role == "coldkey":
        args.append("--no-password")
    result = invoke(*args)
    assert result.exit_code == 0, result.output
    restored = getattr(Wallet("schemes", path=str(wallet_path)), role)
    assert restored.crypto_type == code
    assert restored.ss58_address == original.ss58_address
    if code in HASHED_CRYPTO_TYPES:
        assert restored.hashed_descriptor == original.hashed_descriptor
        restored, original = restored.at_generation(1), original.at_generation(1)
    assert restored.verify(b"restored key", original.sign(b"restored key"))


@pytest.mark.parametrize("scheme", ["hashed", "ms"])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_cli_rejects_long_rotating_private_key_before_writing(wallet_path, scheme, role):
    secret = "ab" * 64
    result = invoke(f"regen-{role}", "--crypto-type", scheme, "--private-key", secret)
    assert result.exit_code == 2, result.output
    assert "32 bytes" in result.output
    assert secret not in result.output
    assert not wallet_path.exists()


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_cli_ed_recovery_preserves_legacy_64_byte_backups(wallet_path, role):
    original = Keypair.create_from_seed(bytes([32]) * 32, CRYPTO_ED25519)
    backup = (bytes([32]) * 32 + bytes(original.public_key)).hex()
    args = [f"regen-{role}", "--crypto-type", "ed", "--private-key", backup]
    if role == "coldkey":
        args.append("--no-password")
    result = invoke(*args)
    assert result.exit_code == 0, result.output
    assert (
        getattr(Wallet("schemes", path=str(wallet_path)), role).ss58_address
        == original.ss58_address
    )
