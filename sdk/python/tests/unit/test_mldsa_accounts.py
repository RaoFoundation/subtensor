"""ML-DSA uses normal wallet selection and the protected account transport."""

from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from typer.testing import CliRunner

from bittensor import wallets
from bittensor._transport.extrinsics import create_hashed_extrinsic
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.hashed import descriptor_bytes, descriptor_value
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.sp_core import CRYPTO_ED25519, CRYPTO_HASHED, CRYPTO_MLDSA, CRYPTO_SR25519, Keypair
from bittensor.wallet import Wallet
from tests.unit.test_hashed_accounts import GENESIS, MNEMONIC, _connection


class MlDsaCodec:
    def constant(self, pallet, name):
        assert (pallet, name) == ("HashedAccounts", "Enabled")
        return True

    def signature_payload_parts(self, call, *, nonce, **kwargs):
        return call, nonce.to_bytes(4, "little"), b"runtime-genesis-era"

    def encode_signed_extrinsic(self, call, *, signature_version, signature, public_key, **kwargs):
        assert signature_version == CRYPTO_MLDSA
        assert len(signature) == 5301
        return b"\x45\x02" + public_key + signature + call, "0xhash"


@pytest.mark.parametrize("selected", [None, CRYPTO_ED25519, CRYPTO_SR25519, CRYPTO_HASHED])
def test_receiving_address_never_falls_back_to_a_weaker_message_scheme(monkeypatch, selected):
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_MLDSA)
    address = receiving_address(key, GENESIS)

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


def test_mldsa_descriptors_receiving_addresses_and_public_recovery():
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_MLDSA)
    descriptor = bytes(key.hashed_descriptor)
    assert descriptor_bytes(descriptor_value(descriptor)) == descriptor
    assert descriptor_value(descriptor)["scheme"] == "MlDsa65"
    address = receiving_address(key, GENESIS)
    recipient = parse_recipient(address)
    assert recipient.account == key.ss58_address
    assert recipient.descriptor == descriptor
    assert Keypair.from_hashed_descriptor(descriptor).crypto_type == CRYPTO_MLDSA


async def test_mldsa_transport_refreshes_generation_and_prices_complete_proof():
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_MLDSA)
    conn, _, _ = _connection(key, generation=7, nonce=8)
    conn._runtimes.codec_at = AsyncMock(return_value=MlDsaCodec())
    signed = await conn.create_signed_extrinsic(b"transfer", key)
    assert signed.data[:2] == b"\x45\x02"
    assert signed.data[2:34] == bytes(key.public_key)
    proof = signed.data[34:5335]
    assert int.from_bytes(proof[:8], "little") == 7
    assert proof[8:1960] == bytes(key.at_generation(7).hashed_signing_public_key)
    assert proof[1960:1992] == bytes(key.at_generation(7).hashed_next_commitment)
    estimate = await conn.sign_without_nonce_tracking(
        b"transfer", key.public_only(), nonce=8, signature=bytes(64)
    )
    assert len(estimate.data) == len(signed.data)
    assert int.from_bytes(estimate.data[34:42], "little") == 7
    # Dropping before inclusion leaves the chain state and wallet backup unchanged.
    retry = await conn.create_signed_extrinsic(b"transfer", key)
    assert retry.data[34:2026] == signed.data[34:2026]
    assert key.hashed_generation == 0


@pytest.mark.parametrize("length", [0, 64, 136, 5300, 5302])
def test_mldsa_transport_rejects_incomplete_or_oversized_proofs(length):
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_MLDSA)
    with pytest.raises(ValueError, match="5301-byte"):
        create_hashed_extrinsic(
            MlDsaCodec(),
            b"transfer",
            key,
            era="00",
            nonce=1,
            tip=0,
            tip_asset_id=None,
            genesis_hash=GENESIS,
            era_block_hash=GENESIS,
            proof=bytes(length),
        )


@pytest.mark.parametrize(
    "crypto_type", [CRYPTO_ED25519, CRYPTO_SR25519, CRYPTO_HASHED, CRYPTO_MLDSA]
)
def test_message_signing_and_explicit_or_automatic_verification(tmp_path, crypto_type):
    wallet = Wallet("signer", path=str(tmp_path))
    wallet.regenerate_coldkey(
        mnemonic=MNEMONIC, crypto_type=crypto_type, use_password=False, suppress=True
    )
    if crypto_type == CRYPTO_HASHED:
        with pytest.raises(ValueError, match="generation zero"):
            wallets.sign_message("challenge", name="signer", path=str(tmp_path))
    signed = wallets.sign_message(
        "challenge",
        name="signer",
        path=str(tmp_path),
        hashed_generation=1 if crypto_type == CRYPTO_HASHED else None,
    )
    with pytest.raises(ValueError, match=r"trusted.*crypto_type"):
        wallets.verify_message("challenge", signed["signature"], signed["ss58"])
    if crypto_type in (CRYPTO_HASHED, CRYPTO_MLDSA):
        assert wallets.verify_message(
            "challenge", signed["signature"], receiving_address(wallet.coldkeypub, GENESIS)
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


@pytest.mark.parametrize("scheme", ["ed", "sr", "hashed", "mldsa"])
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
    if scheme == "hashed":
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
    for flags in ([], ["--crypto-type", scheme]):
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


@pytest.mark.parametrize("crypto_type", [CRYPTO_HASHED, CRYPTO_MLDSA])
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


def test_http_auth_rejects_classical_forgery_against_a_trusted_mldsa_identity(monkeypatch):
    from bittensor import http_auth

    sender = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_MLDSA)
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
        pytest.fail("an ML-DSA identity must never reach a classical signature verifier")

    monkeypatch.setattr(http_auth, "_sp_core_verify", classical_verifier)
    with pytest.raises(http_auth.BadSignature, match="trusted sender scheme"):
        http_auth.verify(
            headers,
            b"body",
            method="POST",
            path="/generate",
            self_hotkey_ss58=receiver.ss58_address,
            expected_crypto_type=CRYPTO_MLDSA,
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
