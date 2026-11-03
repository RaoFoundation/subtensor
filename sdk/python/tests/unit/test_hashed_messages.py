"""Hashed Ed25519 uses normal wallet selection and the protected account transport."""

from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from typer.testing import CliRunner

from bittensor import wallets
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.hashed import descriptor_value
from bittensor.receiving import receiving_address
from bittensor.sp_core import (
    CLASSICAL_HASHED_CRYPTO_TYPES,
    CRYPTO_ED25519,
    CRYPTO_HASHED,
    CRYPTO_HASHED_ED25519,
    CRYPTO_SR25519,
    HASHED_CRYPTO_TYPES,
    Keypair,
)
from bittensor.wallet import Wallet
from tests.unit.test_hashed_accounts import MNEMONIC


@pytest.mark.parametrize("selected", [None, CRYPTO_ED25519, CRYPTO_SR25519, CRYPTO_HASHED])
def test_receiving_address_never_falls_back_to_a_weaker_message_scheme(monkeypatch, selected):
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_HASHED_ED25519)
    address = receiving_address(key)

    class ClassicalForgery:
        """Model a broken classical verifier; no quantum hardware is needed."""

        from_hashed_descriptor = staticmethod(Keypair.from_hashed_descriptor)

        def __init__(self, *, ss58_address, crypto_type):
            self.crypto_type = crypto_type

        def verify(self, message, signature):
            return self.crypto_type in (CRYPTO_ED25519, CRYPTO_SR25519)

    monkeypatch.setattr(wallets, "Keypair", ClassicalForgery)
    assert not wallets.verify_message("challenge", "00" * 64, address, selected)


def test_bare_account_message_verification_requires_a_trusted_scheme():
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_ED25519)
    signature = bytes(key.sign(b"challenge")).hex()
    with pytest.raises(ValueError, match=r"trusted.*crypto_type"):
        wallets.verify_message("challenge", signature, key.ss58_address)
    assert wallets.verify_message("challenge", signature, key.ss58_address, CRYPTO_ED25519)


@pytest.mark.parametrize(
    "crypto_type",
    [
        CRYPTO_ED25519,
        CRYPTO_SR25519,
        CRYPTO_HASHED,
        CRYPTO_HASHED_ED25519,
    ],
)
def test_message_signing_and_explicit_or_automatic_verification(tmp_path, crypto_type):
    wallet = Wallet("signer", path=str(tmp_path))
    wallet.regenerate_coldkey(
        mnemonic=MNEMONIC, crypto_type=crypto_type, use_password=False, suppress=True
    )
    if crypto_type in CLASSICAL_HASHED_CRYPTO_TYPES:
        with pytest.raises(ValueError, match="generation zero"):
            wallets.sign_message("challenge", name="signer", path=str(tmp_path))
    signed = wallets.sign_message(
        "challenge",
        name="signer",
        path=str(tmp_path),
        hashed_generation=1 if crypto_type in CLASSICAL_HASHED_CRYPTO_TYPES else None,
    )
    with pytest.raises(ValueError, match=r"trusted.*crypto_type"):
        wallets.verify_message("challenge", signed["signature"], signed["ss58"])
    if crypto_type in HASHED_CRYPTO_TYPES:
        assert wallets.verify_message(
            "challenge", signed["signature"], receiving_address(wallet.coldkeypub)
        )
    assert wallets.verify_message("challenge", signed["signature"], signed["ss58"], crypto_type)
    assert not wallets.verify_message("changed", signed["signature"], signed["ss58"], crypto_type)
    assert not wallets.verify_message(
        "challenge", signed["signature"] + "00", signed["ss58"], crypto_type
    )
    other = Keypair.create_from_seed(bytes([99]) * 32, crypto_type)
    assert not wallets.verify_message(
        "challenge", signed["signature"], other.ss58_address, crypto_type
    )


@pytest.mark.parametrize("scheme", ["ed", "sr", "hashed", "hashed-ed25519"])
def test_cli_message_round_trip_uses_finalized_state_only_for_classical_hashed(
    tmp_path, monkeypatch, scheme
):
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path / "wallets"))
    monkeypatch.setenv("BT_WALLET", "signer")
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    wallet = Wallet("signer", path=str(tmp_path / "wallets"))
    wallet.regenerate_coldkey(
        mnemonic=MNEMONIC,
        crypto_type=wallets.parse_crypto_type(scheme),
        use_password=False,
        suppress=True,
    )
    if scheme in ("hashed", "hashed-ed25519"):
        import asyncio

        async def query(item, params, *, block):
            assert item == ("HashedAccounts", "Accounts")
            assert params == [wallet.coldkeypub.ss58_address]
            assert block == 77
            return {
                "descriptor": descriptor_value(bytes(wallet.coldkeypub.hashed_descriptor)),
                "generation": 1,
            }

        client = SimpleNamespace(finalized_block=AsyncMock(return_value=77), query=query)
        monkeypatch.setattr(AppContext, "run", lambda self, work: asyncio.run(work(client)))
    else:

        def offline_only(*args):
            pytest.fail("message signing should not access the chain for this key type")

        monkeypatch.setattr(AppContext, "run", offline_only)
    import json

    runner = CliRunner()
    result = runner.invoke(app, ["--yes", "--json", "wallet", "sign", "--message", "challenge"])
    assert result.exit_code == 0, result.output
    signed = json.loads(result.output)
    for flags in (
        [],
        ["--crypto-type", scheme],
    ):
        result = runner.invoke(
            app,
            [
                "--json",
                "wallet",
                "verify",
                "--message",
                "challenge",
                "--signature",
                signed["signed_message"],
                "--ss58",
                signed["signer_address"],
                *flags,
            ],
        )
        assert result.exit_code == (0 if flags else 1), result.output
        if not flags:
            assert "trusted crypto_type" in result.output


@pytest.mark.parametrize("crypto_type", [CRYPTO_HASHED, CRYPTO_HASHED_ED25519])
def test_protected_hotkey_http_authentication(crypto_type):
    from bittensor import http_auth

    sender = Keypair.create_from_mnemonic(MNEMONIC, crypto_type).at_generation(1)
    receiver = Keypair.create_from_uri("//Bob")
    nonce = 1_752_076_800_000_000_000
    headers = http_auth.sign(
        sender,
        method="POST",
        path="/generate",
        body=b"body",
        receiver_ss58=receiver.ss58_address,
        nonce_ns=nonce,
    )
    caller = http_auth.verify(
        headers,
        b"body",
        method="POST",
        path="/generate",
        self_hotkey_ss58=receiver.ss58_address,
        expected_crypto_type=crypto_type,
        now_ns=nonce,
    )
    assert caller.hotkey_ss58 == sender.ss58_address
    assert caller.crypto_type == crypto_type
    with pytest.raises(http_auth.BadSignature):
        http_auth.verify(
            headers,
            b"changed body",
            method="POST",
            path="/generate",
            self_hotkey_ss58=receiver.ss58_address,
            expected_crypto_type=crypto_type,
            now_ns=nonce,
        )


def test_http_auth_rejects_classical_forgery_against_a_trusted_hashed_ed25519_identity(monkeypatch):
    from bittensor import http_auth

    sender = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_HASHED_ED25519).at_generation(1)
    receiver = Keypair.create_from_uri("//Bob")
    nonce = 1_800_000_000_000_000_000
    headers = http_auth.sign(
        sender,
        method="POST",
        path="/generate",
        body=b"body",
        receiver_ss58=receiver.ss58_address,
        nonce_ns=nonce,
    )
    headers[http_auth.HEADER_CRYPTO] = "sr25519"
    headers[http_auth.HEADER_SIGNATURE] = "00" * 64

    def classical_verifier(*args):
        pytest.fail("an Hashed Ed25519 identity must never reach a classical signature verifier")

    monkeypatch.setattr(http_auth, "_sp_core_verify", classical_verifier)
    with pytest.raises(http_auth.BadSignature, match="trusted sender scheme"):
        http_auth.verify(
            headers,
            b"body",
            method="POST",
            path="/generate",
            self_hotkey_ss58=receiver.ss58_address,
            expected_crypto_type=CRYPTO_HASHED_ED25519,
            now_ns=nonce,
        )


def test_cli_never_uses_a_companion_accounts_retired_generation(tmp_path, monkeypatch):
    import asyncio

    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path))
    monkeypatch.setenv("BT_WALLET", "signer")
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    wallet = Wallet("signer", path=str(tmp_path))
    wallet.regenerate_coldkey(
        mnemonic=MNEMONIC, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
    )
    actual = wallet.coldkey
    unrelated = Keypair.create_from_seed(bytes([99]) * 32, CRYPTO_HASHED)
    wallet.coldkeypub_file.set_keypair(unrelated.public_only(), encrypt=False, overwrite=True)

    async def query(item, params, *, block):
        assert params == [actual.ss58_address]
        return {"descriptor": descriptor_value(bytes(actual.hashed_descriptor)), "generation": 0}

    client = SimpleNamespace(finalized_block=AsyncMock(return_value=77), query=query)
    monkeypatch.setattr(AppContext, "run", lambda self, work: asyncio.run(work(client)))
    result = CliRunner().invoke(
        app, ["--yes", "--json", "wallet", "sign", "--message", "challenge"]
    )
    assert result.exit_code != 0
    assert "generation zero" in result.output


@pytest.mark.parametrize("crypto_type", [CRYPTO_HASHED, CRYPTO_HASHED_ED25519])
async def test_disk_hotkey_http_signer_preparation_uses_finalized_retirement(tmp_path, crypto_type):
    from bittensor import Client, http_auth
    from tests.harness.fake_substrate import FakeSubstrate

    wallet = Wallet("sender", path=str(tmp_path))
    wallet.regenerate_hotkey(mnemonic=MNEMONIC, crypto_type=crypto_type, suppress=True)
    # Reload from disk, as documented applications do; no manual generation.
    wallet = Wallet("sender", path=str(tmp_path))
    with pytest.raises(ValueError, match="generation zero"):
        http_auth.sign(wallet, method="POST", path="/generate")
    key = wallet.hotkey
    chain = FakeSubstrate()
    chain.finalized = 77
    finalized = await chain.block_hash(77)
    chain.query = AsyncMock(
        return_value={"descriptor": descriptor_value(bytes(key.hashed_descriptor)), "generation": 9}
    )
    client = Client(substrate=chain)
    prepared = await http_auth.prepare_signer(wallet, client=client)
    chain.query.assert_awaited_once_with(
        "HashedAccounts", "Accounts", [key.ss58_address], block_hash=finalized
    )
    receiver = Keypair.create_from_uri("//Bob")
    nonce = 1_752_076_800_000_000_000
    headers = http_auth.sign(
        prepared,
        method="POST",
        path="/generate",
        body=b"body",
        receiver_ss58=receiver.ss58_address,
        nonce_ns=nonce,
    )
    caller = http_auth.verify(
        headers,
        b"body",
        method="POST",
        path="/generate",
        self_hotkey_ss58=receiver.ss58_address,
        expected_crypto_type=crypto_type,
        now_ns=nonce,
        nonce_store=http_auth.InMemoryNonceStore(),
    )
    assert caller.hotkey_ss58 == key.ss58_address
    assert caller.crypto_type == crypto_type
    # Preparation does not mutate a wallet's untrusted generation on disk.
    with pytest.raises(ValueError, match="generation zero"):
        http_auth.sign(Wallet("sender", path=str(tmp_path)), method="POST", path="/generate")


@pytest.mark.parametrize("crypto_type", [CRYPTO_HASHED, CRYPTO_HASHED_ED25519])
@pytest.mark.parametrize("state", ["missing", "active", "mismatch", "no_finality", "unavailable"])
async def test_hashed_http_preparation_fails_closed(tmp_path, crypto_type, state):
    from bittensor import Client, http_auth
    from tests.harness.fake_substrate import FakeSubstrate

    wallet = Wallet("sender", path=str(tmp_path))
    wallet.regenerate_hotkey(mnemonic=MNEMONIC, crypto_type=crypto_type, suppress=True)
    key = wallet.hotkey
    chain = FakeSubstrate()
    record = {"descriptor": descriptor_value(bytes(key.hashed_descriptor)), "generation": 2}
    if state == "missing":
        record = None
    elif state == "active":
        record["generation"] = 0
    elif state == "mismatch":
        other = Keypair.create_from_seed(bytes([95]) * 32, crypto_type)
        record["descriptor"] = descriptor_value(bytes(other.hashed_descriptor))
    chain.query = AsyncMock(return_value=record)
    if state == "no_finality":
        chain.block_hash = AsyncMock(return_value=None)
    elif state == "unavailable":
        chain.query.side_effect = RuntimeError("finalized state unavailable")
    with pytest.raises((ValueError, RuntimeError)):
        await http_auth.prepare_signer(wallet, client=Client(substrate=chain))
    if state == "no_finality":
        chain.query.assert_not_awaited()


@pytest.mark.parametrize("crypto_type", [CRYPTO_HASHED, CRYPTO_HASHED_ED25519])
async def test_http_preparation_uses_actual_hotkey_not_stale_companion(tmp_path, crypto_type):
    from bittensor import Client, http_auth
    from tests.harness.fake_substrate import FakeSubstrate

    wallet = Wallet("sender", path=str(tmp_path))
    wallet.regenerate_hotkey(mnemonic=MNEMONIC, crypto_type=crypto_type, suppress=True)
    actual = wallet.hotkey
    unrelated = Keypair.create_from_seed(bytes([95]) * 32, crypto_type)
    wallet.hotkeypub_file.set_keypair(unrelated.public_only(), encrypt=False, overwrite=True)
    wallet = Wallet("sender", path=str(tmp_path))
    chain = FakeSubstrate()

    async def query(module, storage, params, *, block_hash):
        assert params == [actual.ss58_address]
        return {"descriptor": descriptor_value(bytes(actual.hashed_descriptor)), "generation": 0}

    chain.query = query
    with pytest.raises(ValueError, match="generation zero"):
        await http_auth.prepare_signer(wallet, client=Client(substrate=chain))


@pytest.mark.parametrize("crypto_type", [CRYPTO_SR25519, CRYPTO_ED25519])
async def test_classical_http_preparation_does_not_access_chain(crypto_type):
    from bittensor import http_auth

    key = Keypair.create_from_mnemonic(MNEMONIC, crypto_type)
    client = AsyncMock()
    assert await http_auth.prepare_signer(key, client=client) is key
    assert not client.mock_calls
