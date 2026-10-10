"""Hashed wallet recovery, setup and generation-aware transaction signing."""

from __future__ import annotations

import json
import shutil
from copy import deepcopy
from hashlib import blake2b
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from typer.testing import CliRunner

from bittensor._transport.codec import StorageEntry
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.extrinsics import NonceCache, create_hashed_extrinsic, prepare_extrinsic
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


class _LifecycleCodec(_Codec):
    """Storage framing is JSON in this harness; it does not claim SCALE coverage."""

    def storage_entry(self, pallet, item):
        assert (pallet, item) in (("HashedAccounts", "Accounts"), ("System", "Account"))
        return StorageEntry(pallet, item, pallet, item, ["AccountId"], [], "Optional", b"null")

    def storage_key(self, entry, params):
        return json.dumps([entry.pallet, entry.name, *params]).encode()

    def decode(self, _type, data):
        return json.loads(data)


class _LifecycleRpc:
    """Stateful RPC boundary for wallet tests, not a FRAME/consensus simulator.

    The real transport fetches snapshots and submits proof bytes here. Inclusion
    is controlled by the test, after independent native signature verification;
    dropping and rewinding state exercise wallet resynchronization only. Runtime
    tests separately establish authorization and dispatch semantics.
    """

    def __init__(self, public):
        self.address = public.ss58_address
        self.account = bytes(public.public_key)
        self.head = "0x" + "00" * 32
        self.snapshots = {
            self.head: {
                "descriptor": descriptor_value(bytes(public.hashed_descriptor)),
                "generation": 0,
                "commitment": "0x" + bytes(public.hashed_descriptor)[2:].hex(),
                "nonce": 1,
            }
        }
        self.pending = None
        self.storage_reads = []
        self._next_head = 1

    @property
    def state(self):
        return self.snapshots[self.head]

    def connection(self):
        codec = _LifecycleCodec()
        conn = SubstrateConnection("ws://wallet-lifecycle.invalid")
        conn._session = self
        conn._nonces = NonceCache(self)
        conn._runtimes = SimpleNamespace(
            codec_at=AsyncMock(return_value=codec), genesis_hash=AsyncMock(return_value=GENESIS)
        )
        return conn

    async def request(self, method, params):
        if method == "chain_getHead":
            return self.head
        if method == "account_nextIndex":
            assert params == [self.address]
            return self.state["nonce"] + int(self.pending is not None)
        if method == "state_getStorageAt":
            encoded_key, block_hash = params
            pallet, item, address = json.loads(bytes.fromhex(encoded_key.removeprefix("0x")))
            assert block_hash is not None
            self.storage_reads.append((pallet, item, address, block_hash))
            if address != self.address:
                return None
            state = self.snapshots[block_hash]
            value = (
                {name: value for name, value in state.items() if name != "nonce"}
                if pallet == "HashedAccounts"
                else {"nonce": state["nonce"]}
            )
            return "0x" + json.dumps(value).encode().hex()
        if method == "author_submitExtrinsic":
            assert self.pending is None
            body = bytes.fromhex(params[0].removeprefix("0x"))
            self._verify(body)
            self.pending = body
            return "0x" + blake2b(body, digest_size=32).hexdigest()
        raise AssertionError(f"unexpected RPC method: {method}")

    def _verify(self, body):
        assert body[:2] == b"\x45\x01"
        assert body[2:34] == self.account
        proof, encoded_nonce, call = body[34:170], body[170:174], body[174:]
        generation = int.from_bytes(proof[:8], "little")
        assert generation == self.state["generation"]
        assert int.from_bytes(encoded_nonce, "little") == self.state["nonce"]
        commitment = blake2b(
            b"bittensor/hashed/v1/key\x01\x01" + proof[8:40], digest_size=32
        ).hexdigest()
        assert "0x" + commitment == self.state["commitment"]
        implication = b"\x01" + call + encoded_nonce + b"runtime-genesis-era"
        transcript = (
            b"bittensor/hashed/v1/transaction"
            + self.account
            + b"\x01"
            + proof[:8]
            + proof[40:72]
            + blake2b(implication, digest_size=32).digest()
        )
        verifier = Keypair(public_key=proof[8:40], crypto_type=CRYPTO_SR25519)
        assert verifier.verify(blake2b(transcript, digest_size=32).digest(), proof[72:])

    def include(self):
        assert self.pending is not None
        self._verify(self.pending)
        state = deepcopy(self.state)
        state["commitment"] = "0x" + self.pending[74:106].hex()
        state["generation"] += 1
        state["nonce"] += 1
        self.head = "0x" + self._next_head.to_bytes(32, "big").hex()
        self._next_head += 1
        self.snapshots[self.head] = state
        self.pending = None

    def drop(self):
        assert self.pending is not None
        self.pending = None

    def reorg_to(self, head):
        assert head in self.snapshots
        self.head = head
        self.pending = None


def _wallet_files(wallet):
    root = Path(wallet.path) / wallet.name
    return {file.relative_to(root): file.read_bytes() for file in root.rglob("*") if file.is_file()}


def _restore_machine(root, mnemonic=MNEMONIC):
    wallet = Wallet("wallet", path=str(root))
    wallet.regenerate_coldkey(
        mnemonic=mnemonic, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
    )
    return wallet


async def _sign_submit(conn, signer, call):
    signed = await conn.create_signed_extrinsic(call, signer)
    await conn.submit_extrinsic(signed)
    return signed


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


async def test_lost_machine_restores_from_mnemonic_and_signs_current_generation(tmp_path):
    machine_one = tmp_path / "machine-one"
    first = Wallet("wallet", path=str(machine_one))
    recovery_words = []
    first.create_new_coldkey(
        crypto_type=CRYPTO_HASHED,
        use_password=False,
        suppress=True,
        on_mnemonic=recovery_words.append,
    )
    address = first.coldkeypub.ss58_address
    rpc = _LifecycleRpc(first.coldkeypub)
    conn = rpc.connection()
    signer = WalletSigner(first)
    original_files = _wallet_files(first)
    for call in (b"first transfer", b"second transfer"):
        await _sign_submit(conn, signer, call)
        rpc.include()
    assert rpc.state["generation"] == 2
    assert _wallet_files(first) == original_files

    # Simulate losing every local wallet file, not merely opening another
    # handle to the first machine's keyfile or its unlocked key cache.
    shutil.rmtree(machine_one)
    del first, signer, conn
    assert not machine_one.exists()
    restored = _restore_machine(tmp_path / "machine-two", recovery_words[0])
    restored_files = _wallet_files(restored)
    assert restored.coldkeypub.ss58_address == address
    conn = rpc.connection()
    signed = await _sign_submit(conn, WalletSigner(restored), b"recovered transfer")
    assert int.from_bytes(signed.data[34:42], "little") == 2
    assert int.from_bytes(signed.data[170:174], "little") == 3
    rpc.include()
    assert rpc.state["generation"] == 3
    assert _wallet_files(restored) == restored_files
    assert len(restored_files) == 2


async def test_old_keyfile_backup_and_stale_nonce_cache_use_current_chain_state(tmp_path):
    first = _restore_machine(tmp_path / "first")
    stale_backup = tmp_path / "stale-backup"
    shutil.copytree(Path(first.path), stale_backup)
    rpc = _LifecycleRpc(first.coldkeypub)
    conn = rpc.connection()
    for call in (b"advance once", b"advance twice"):
        await _sign_submit(conn, WalletSigner(first), call)
        rpc.include()

    old_copy = Wallet("wallet", path=str(stale_backup))
    original_backup = _wallet_files(old_copy)
    assert old_copy.coldkey.hashed_generation == 0
    restored_conn = rpc.connection()
    restored_conn._nonces.pin(old_copy.coldkeypub.ss58_address, 1000)
    signed = await _sign_submit(restored_conn, WalletSigner(old_copy), b"from stale backup")
    assert int.from_bytes(signed.data[34:42], "little") == 2
    assert int.from_bytes(signed.data[170:174], "little") == 3
    rpc.include()
    assert _wallet_files(old_copy) == original_backup
    assert _wallet_files(first) == original_backup


async def test_dropped_transaction_resynchronizes_without_consuming_a_generation(tmp_path):
    wallet = _restore_machine(tmp_path / "machine")
    files = _wallet_files(wallet)
    rpc = _LifecycleRpc(wallet.coldkeypub)
    conn = rpc.connection()
    signer = WalletSigner(wallet)
    dropped = await _sign_submit(conn, signer, b"transaction that is dropped")
    with pytest.raises(SubstrateRequestException, match="pending transaction"):
        await conn.create_signed_extrinsic(b"cannot pipeline", signer)
    assert rpc.state["generation"] == 0
    rpc.drop()
    replacement = await _sign_submit(conn, signer, b"replacement after dropping")
    assert dropped.data[34:106] == replacement.data[34:106]
    assert replacement.data.endswith(b"replacement after dropping")
    rpc.include()
    assert rpc.state["generation"] == 1
    assert rpc.state["nonce"] == 2
    assert _wallet_files(wallet) == files


async def test_reorg_reads_the_rewound_commitment_without_local_counter_rollback(tmp_path):
    wallet = _restore_machine(tmp_path / "machine")
    files = _wallet_files(wallet)
    rpc = _LifecycleRpc(wallet.coldkeypub)
    conn = rpc.connection()
    signer = WalletSigner(wallet)
    await _sign_submit(conn, signer, b"common ancestor")
    rpc.include()
    ancestor = rpc.head
    orphaned = await _sign_submit(conn, signer, b"orphaned transfer")
    rpc.include()
    orphaned_head = rpc.head
    assert rpc.state["generation"] == 2
    rpc.reorg_to(ancestor)
    rpc.storage_reads.clear()

    replacement = await _sign_submit(conn, signer, b"replacement branch")
    assert int.from_bytes(replacement.data[34:42], "little") == 1
    assert int.from_bytes(replacement.data[170:174], "little") == 2
    assert replacement.data[34:106] == orphaned.data[34:106]
    assert {row[3] for row in rpc.storage_reads} == {ancestor}
    assert orphaned_head != ancestor
    rpc.include()
    assert rpc.head != orphaned_head
    assert _wallet_files(wallet) == files


async def test_wrong_mnemonic_never_unlocks_or_signs_for_the_original_account(tmp_path):
    original = _restore_machine(tmp_path / "original")
    rpc = _LifecycleRpc(original.coldkeypub)
    wrong_words = "legal winner thank year wave sausage worth useful legal winner thank yellow"
    wrong = _restore_machine(tmp_path / "wrong-restore", wrong_words)
    assert wrong.coldkeypub.ss58_address != original.coldkeypub.ss58_address
    signer = WalletSigner(wrong)
    files = _wallet_files(wrong)
    conn = rpc.connection()
    with pytest.raises(SubstrateRequestException, match="not registered"):
        await conn.create_signed_extrinsic(b"cannot spend original funds", signer)
    assert signer._keypair is None
    assert not hasattr(conn._runtimes.codec_at.return_value, "proof")
    assert rpc.pending is None
    assert rpc.state["generation"] == 0
    assert _wallet_files(wrong) == files


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
    assert "bth1_" in result.output
    assert wallet.coldkeypub.ss58_address not in result.output


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
    assert plan.call.function == "batch_all"
    guard, payment = plan.call.params["calls"]
    assert guard.function == "check_registered"
    assert payment.function == "transfer_keep_alive"
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


async def test_signing_reads_live_switch_at_the_authority_block():
    key = _key()
    conn, codec, _ = _connection(key)
    codec.enabled = None
    query_authority = conn.query
    enabled = False

    async def query(pallet, item, params=None, *, block_hash):
        assert block_hash == "0xhead"
        if (pallet, item) == ("AdminUtils", "HashedAccountsEnabled"):
            return enabled
        return await query_authority(pallet, item, params, block_hash=block_hash)

    conn.query = AsyncMock(side_effect=query)
    for enabled in (False, True, False, True):
        if enabled:
            await conn.create_signed_extrinsic(b"call", key)
        else:
            with pytest.raises(SubstrateRequestException, match="not enabled"):
                await conn.create_signed_extrinsic(b"call", key)
