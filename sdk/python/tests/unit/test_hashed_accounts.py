"""Hashed wallet recovery, setup and generation-aware transaction signing."""

from __future__ import annotations

from hashlib import blake2b
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from typer.testing import CliRunner

from bittensor._transport.extrinsics import create_hashed_extrinsic, prepare_extrinsic
from bittensor._transport.interface import SubstrateConnection
from bittensor.cli.main import app
from bittensor.executor import Executor, estimate_shielded_carrier_fee
from bittensor.hashed import descriptor_value
from bittensor.intents import AnnounceColdkeySwap, Transfer
from bittensor.signing import WalletSigner
from bittensor.sp_core import CRYPTO_HASHED, CRYPTO_SR25519, Keypair
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate

MNEMONIC = "bottom drive obey lake curtain smoke basket hold race lonely fit walk"
GENESIS = "0x" + "11" * 32


def _key():
    return Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_HASHED)


def _record(key, generation=0):
    return {
        "descriptor": descriptor_value(bytes(key.hashed_descriptor)),
        "generation": generation,
        "commitment": key.at_generation(generation).hashed_current_commitment,
    }


class _Codec:
    """Only the codec seam is mocked; native keys and proof verification are real."""

    enabled = True

    def constant(self, pallet, name):
        assert (pallet, name) == ("HashedAccounts", "Enabled")
        return self.enabled

    def signature_payload_parts(self, call, *, nonce, **kwargs):
        return call, nonce.to_bytes(4, "little"), b"runtime-genesis-era"

    def encode_signed_extrinsic(
        self, call, *, public_key, signature, signature_version, nonce, **kw
    ):
        assert signature_version == CRYPTO_HASHED
        self.call = call
        self.proof = signature
        self.account = public_key
        self.nonce = nonce
        body = b"\x45\x01" + public_key + signature + nonce.to_bytes(4, "little") + call
        return body, "0x" + blake2b(body, digest_size=32).hexdigest()


def _connection(key, *, generation=7, nonce=13):
    codec = _Codec()
    conn = SubstrateConnection("ws://unused.invalid")
    conn._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    conn.get_chain_head = AsyncMock(return_value="0xhead")
    conn.genesis_hash = AsyncMock(return_value=GENESIS)
    conn._nonces = SimpleNamespace(next_for=AsyncMock(return_value=nonce))
    conn._normalize_era = AsyncMock(return_value=("00", GENESIS))
    record = _record(key, generation)

    async def query(pallet, item, params, *, block_hash):
        assert block_hash == "0xhead"
        assert params == [key.ss58_address]
        return record if pallet == "HashedAccounts" else {"nonce": nonce}

    conn.query = AsyncMock(side_effect=query)
    return conn, codec, record


def _assert_proof(codec, key, generation, nonce, call):
    proof = codec.proof
    assert len(proof) == 136
    assert int.from_bytes(proof[:8], "little") == generation
    assert codec.nonce == nonce
    assert codec.account == bytes(key.public_key)
    signer = key.at_generation(generation)
    assert proof[8:40] == bytes(signer.hashed_public_key)
    assert proof[40:72] == bytes(signer.hashed_next_commitment)
    implication = b"\x01" + call + nonce.to_bytes(4, "little") + b"runtime-genesis-era"
    transcript = (
        b"bittensor/hashed/v1/transaction"
        + bytes(key.public_key)
        + b"\x01"
        + generation.to_bytes(8, "little")
        + proof[40:72]
        + blake2b(implication, digest_size=32).digest()
    )
    payload = blake2b(transcript, digest_size=32).digest()
    verifier = Keypair(public_key=proof[8:40], crypto_type=CRYPTO_SR25519)
    assert verifier.verify(payload, proof[72:])
    assert not verifier.verify(bytes(32), proof[72:])


def test_restore_on_another_machine_preserves_identity_and_all_generations(tmp_path):
    first = Wallet("first", path=str(tmp_path / "machine-one"))
    restored = Wallet("restored", path=str(tmp_path / "machine-two"))
    for wallet in (first, restored):
        wallet.regenerate_coldkey(
            mnemonic=MNEMONIC, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
        )
    assert first.coldkey.ss58_address == restored.coldkey.ss58_address
    assert first.coldkeypub.hashed_descriptor == restored.coldkeypub.hashed_descriptor
    for generation in (0, 1, 37, 1_000_000):
        assert (
            first.coldkey.at_generation(generation).hashed_public_key
            == restored.coldkey.at_generation(generation).hashed_public_key
        )
    assert len(list((tmp_path / "machine-two" / "restored").iterdir())) == 2
    with pytest.raises(ValueError):
        restored.coldkeypub.at_generation(1)
    with pytest.raises(ValueError):
        restored.coldkey.sign(b"off-chain challenge")


@pytest.mark.parametrize(
    "hotkey_option,expected_hotkey",
    [([], CRYPTO_HASHED), (["--hotkey-crypto-type", "sr25519"], CRYPTO_SR25519)],
)
def test_cli_type_creates_wallet_and_public_descriptors(
    tmp_path, monkeypatch, hotkey_option, expected_hotkey
):
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path))
    monkeypatch.setenv("BT_WALLET", "new")
    result = CliRunner().invoke(
        app, ["--yes", "wallet", "create", "--type", "hashed", "--no-password", *hotkey_option]
    )
    assert result.exit_code == 0, result.output
    wallet = Wallet("new", path=str(tmp_path))
    assert wallet.coldkeypub.crypto_type == CRYPTO_HASHED
    assert wallet.hotkeypub.crypto_type == expected_hotkey
    assert bytes(wallet.coldkeypub.hashed_descriptor).hex() in result.output


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_cli_mnemonic_recovery_accepts_hashed_type(tmp_path, monkeypatch, role):
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path))
    monkeypatch.setenv("BT_WALLET", "recovered")
    args = ["--yes", "wallet", f"regen-{role}", "--type", "hashed", "--mnemonic", MNEMONIC]
    if role == "coldkey":
        args.append("--no-password")
    result = CliRunner().invoke(app, args)
    assert result.exit_code == 0, result.output
    restored = getattr(Wallet("recovered", path=str(tmp_path)), role)
    assert restored.ss58_address == _key().ss58_address
    assert restored.at_generation(103).hashed_current_commitment == _key().hashed_commitment(103)


async def test_signing_recovers_current_generation_and_verifies_chain_commitment():
    key = _key()
    conn, codec, _ = _connection(key)
    call = b"large-call" * 50
    await conn.create_signed_extrinsic(call, key)
    _assert_proof(codec, key, 7, 13, call)
    conn._nonces.next_for.assert_awaited_once_with(key.ss58_address, use_cache=False)
    assert key.hashed_generation == 0  # signing never advances a local backup


async def test_shield_inner_uses_next_generation_but_checks_current_commitment():
    key = _key()
    conn, codec, _ = _connection(key)
    await conn.create_signed_extrinsic(b"inner", key, nonce=14, hashed_generation_offset=1)
    _assert_proof(codec, key, 8, 14, b"inner")
    await conn.create_signed_extrinsic(b"carrier", key, nonce=13)
    _assert_proof(codec, key, 7, 13, b"carrier")


@pytest.mark.parametrize(
    "failure", ["missing", "disabled", "commitment", "descriptor", "pending", "nonce", "exhausted"]
)
async def test_signing_fails_closed_for_unsynchronized_state(failure):
    key = _key()
    conn, codec, record = _connection(key)
    if failure == "missing":
        conn.query = AsyncMock(return_value=None)
    elif failure == "disabled":
        codec.enabled = False
    elif failure == "commitment":
        record["commitment"] = bytes(32)
    elif failure == "descriptor":
        record["descriptor"]["initial_commitment"] = "0x" + "00" * 32
    elif failure == "pending":
        conn._nonces.next_for.return_value = 14
    elif failure == "exhausted":
        record["generation"] = 2**64 - 1
    with pytest.raises(Exception, match="hashed"):
        await conn.create_signed_extrinsic(b"call", key, nonce=12 if failure == "nonce" else None)
    assert not hasattr(codec, "proof")


async def test_fee_estimation_does_not_unlock_or_rotate(tmp_path):
    wallet = Wallet("locked", path=str(tmp_path))
    wallet.regenerate_coldkey(
        mnemonic=MNEMONIC,
        crypto_type=CRYPTO_HASHED,
        coldkey_password="test password",
        suppress=True,
    )
    signer = WalletSigner(wallet)
    conn, codec, _ = _connection(_key())
    await conn.sign_without_nonce_tracking(b"call", signer, nonce=13, signature=bytes(64))
    assert signer._keypair is None
    assert len(codec.proof) == 136


def test_offline_legacy_signing_does_not_accept_hashed_accounts():
    with pytest.raises(ValueError, match="synchronization"):
        prepare_extrinsic(
            _Codec(),
            b"call",
            address=_key().ss58_address,
            public_key=bytes(_key().public_key),
            crypto_type=CRYPTO_HASHED,
            era="00",
            nonce=0,
            genesis_hash=GENESIS,
            era_block_hash=GENESIS,
        )
    with pytest.raises(ValueError, match="136-byte"):
        create_hashed_extrinsic(
            _Codec(),
            b"call",
            _key(),
            era="00",
            nonce=0,
            tip=0,
            tip_asset_id=None,
            genesis_hash=GENESIS,
            era_block_hash=GENESIS,
            proof=bytes(64),
        )


@pytest.fixture
def setup_wallets(tmp_path):
    sponsor = Wallet("sponsor", path=str(tmp_path))
    sponsor.regenerate_coldkey(seed=bytes([1]) * 32, use_password=False, suppress=True)
    recipient = Wallet("recipient", path=str(tmp_path))
    recipient.regenerate_coldkey(
        mnemonic=MNEMONIC,
        crypto_type=CRYPTO_HASHED,
        coldkey_password="never unlock recipient",
        suppress=True,
    )
    substrate = FakeSubstrate()
    substrate.seed_constant("HashedAccounts", "Enabled", True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    return sponsor, recipient, substrate


@pytest.mark.parametrize("swap", [False, True])
async def test_local_funding_and_migration_register_atomically_without_unlocking(
    setup_wallets, swap
):
    sponsor, recipient, substrate = setup_wallets
    address = recipient.coldkeypub.ss58_address
    intent = AnnounceColdkeySwap(address) if swap else Transfer(address, 1)
    plan = await Executor(substrate).plan(intent, sponsor)
    assert plan.call.module == "Utility" and plan.call.function == "batch_all"
    setup, operation = plan.call.params["calls"]
    assert (setup.module, setup.function) == ("HashedAccounts", "register")
    assert setup.params["descriptor"] == descriptor_value(
        bytes(recipient.coldkeypub.hashed_descriptor)
    )
    assert operation.function == ("announce_coldkey_swap" if swap else "transfer_keep_alive")
    assert plan.extras["hashed_registration_deposit_rao"] == 200_000_000
    assert any("reserve" in effect for effect in plan.effects)
    assert substrate.submissions == []


async def test_registered_destination_is_not_reset_and_remote_descriptor_is_checked(setup_wallets):
    sponsor, recipient, substrate = setup_wallets
    address = recipient.coldkeypub.ss58_address
    substrate.seed("HashedAccounts", "Accounts", [address], _record(_key(), 28))
    plan = await Executor(substrate).plan(Transfer(address, 1), sponsor)
    assert plan.call.function == "transfer_keep_alive"
    assert "hashed_registration" not in plan.extras
    with pytest.raises(ValueError, match="does not match"):
        await Executor(substrate).plan(
            Transfer(
                sponsor.coldkeypub.ss58_address,
                1,
                hashed_descriptor=bytes(recipient.coldkeypub.hashed_descriptor).hex(),
            ),
            sponsor,
        )


async def test_initial_registration_rejects_proxy_wrapping(setup_wallets):
    sponsor, recipient, substrate = setup_wallets
    with pytest.raises(ValueError, match="direct sponsor"):
        await Executor(substrate).plan(
            Transfer(recipient.coldkeypub.ss58_address, 1),
            sponsor,
            proxy_for=sponsor.coldkeypub.ss58_address,
        )


async def test_remote_recipient_can_supply_descriptor_without_local_wallet(setup_wallets):
    sponsor, _, substrate = setup_wallets
    remote = Keypair.create_from_seed(bytes([2]) * 32, CRYPTO_HASHED)
    plan = await Executor(substrate).plan(
        Transfer(remote.ss58_address, 1, hashed_descriptor=bytes(remote.hashed_descriptor).hex()),
        sponsor,
    )
    assert plan.extras["hashed_registration"] == remote.ss58_address
    assert plan.call.function == "batch_all"


async def test_executor_reserves_next_generation_only_for_shield_inner(monkeypatch):
    substrate = FakeSubstrate()
    substrate.mev_key = bytes([1]) * 32
    substrate.sign_extrinsic = AsyncMock(return_value=(b"inner", "0x" + "42" * 32))
    monkeypatch.setattr("bittensor.executor._core.encrypt_mlkem768", lambda *a, **kw: b"encrypted")
    key = _key()
    result = await Executor(substrate)._submit_encrypted_call(
        b"call", key, {}, period=8, wait_for_inclusion=False, wait_for_finalization=False
    )
    assert result.success
    substrate.sign_extrinsic.assert_awaited_once_with(
        b"call", key, nonce=1, period=8, hashed_generation_offset=1
    )
    assert substrate.submissions[0][2]["nonce"] == 0


async def test_shield_fee_estimate_uses_complete_hashed_authorization():
    substrate = FakeSubstrate()
    key = _key()
    substrate.seed_constant("HashedAccounts", "Enabled", True)
    substrate.seed("HashedAccounts", "Accounts", [key.ss58_address], _record(key, 7))
    substrate.estimate_fee = AsyncMock(return_value=substrate.fee)
    await estimate_shielded_carrier_fee(substrate, key.ss58_address)
    signer = substrate.estimate_fee.call_args.args[1]
    assert signer.crypto_type == CRYPTO_HASHED
    assert signer.ss58_address == key.ss58_address
