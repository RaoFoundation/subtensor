"""Adversarial wallet trust-boundary checks using disposable deterministic seeds."""

import hmac
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest

from bittensor.signing import WalletSigner, resolve_signer
from bittensor.sp_core import (
    CLASSICAL_HASHED_CRYPTO_TYPES,
    FIXED_PQ_CRYPTO_TYPES,
    PQ_CRYPTO_TYPES,
    KeyfileError,
    Keypair,
    deserialize_keypair_from_keyfile_data,
    serialized_keypair_to_keyfile_data,
    verify_hashed_authorization,
    verify_mldsa_authorization,
)
from bittensor.wallet import Wallet
from tests.unit.test_mldsa_accounts import connection as rotating_connection
from tests.unit.test_stateless_mldsa import connection

NEW_TYPES = (*CLASSICAL_HASHED_CRYPTO_TYPES, *PQ_CRYPTO_TYPES)
MASTER = bytes([93]) * 32


@pytest.fixture(params=NEW_TYPES)
def wallet_key(request):
    return Keypair.create_from_seed(MASTER, request.param)


def verify(key, proof, payload, *, descriptor=None, generation=None, commitment=None):
    verifier = (
        verify_hashed_authorization
        if key.crypto_type in CLASSICAL_HASHED_CRYPTO_TYPES
        else verify_mldsa_authorization
    )
    try:
        return verifier(
            bytes(proof),
            payload,
            bytes(key.hashed_descriptor) if descriptor is None else descriptor,
            key.hashed_generation if generation is None else generation,
            bytes(key.hashed_current_commitment) if commitment is None else commitment,
        )
    except ValueError:
        return False


def test_watch_only_exports_never_contain_or_reconstruct_secret(wallet_key):
    watch = wallet_key.public_only()
    encoded = serialized_keypair_to_keyfile_data(watch)
    fields = json.loads(encoded)
    assert not {"secretSeed", "secretPhrase", "privateKey"}.intersection(fields)
    assert MASTER.hex().encode() not in encoded
    restored = deserialize_keypair_from_keyfile_data(encoded)
    assert restored.public_key == wallet_key.public_key
    with pytest.raises(ValueError):
        restored.sign_hashed(b"attacker requests signature using only public wallet")
    with pytest.raises(ValueError):
        restored.at_generation(0)


@pytest.mark.parametrize(
    "field", ["secretSeed", "cryptoType", "accountId", "publicKey", "ss58Address", "descriptor"]
)
def test_keyfile_substitution_fails_closed(wallet_key, field):
    keyfile = json.loads(serialized_keypair_to_keyfile_data(wallet_key))
    other = Keypair.create_from_seed(bytes([94]) * 32, wallet_key.crypto_type)
    otherfile = json.loads(serialized_keypair_to_keyfile_data(other))
    if field == "descriptor":
        field = (
            "hashedDescriptor"
            if wallet_key.crypto_type in CLASSICAL_HASHED_CRYPTO_TYPES
            else "mldsaDescriptor"
        )
    keyfile[field] = (
        (0 if wallet_key.crypto_type != 0 else 1) if field == "cryptoType" else otherfile[field]
    )
    with pytest.raises((ValueError, KeyfileError)):
        deserialize_keypair_from_keyfile_data(json.dumps(keyfile).encode())


def test_transaction_proof_rejects_every_field_substitution(wallet_key):
    generation = 0 if wallet_key.crypto_type in FIXED_PQ_CRYPTO_TYPES else 19
    active = wallet_key.at_generation(generation)
    payload = b"call/nonce/era/spec/genesis/metadata" * 40
    proof = bytes(active.sign_hashed(payload))
    assert verify(active, proof, payload)
    if active.crypto_type in CLASSICAL_HASHED_CRYPTO_TYPES:
        starts = [0, 8, 40, 72, len(proof) - 1]
    else:
        public_len = len(active.hashed_public_key)
        starts = [0, 32, 33, 34, 42, 42 + public_len, 106 + public_len, len(proof) - 1]
    for offset in starts:
        changed = bytearray(proof)
        changed[offset] ^= 1
        assert not verify(active, changed, payload), (active.crypto_type, offset)
    assert not verify(active, proof + b"\0", payload)
    assert not verify(active, proof[:-1], payload)
    assert not verify(active, proof, payload + b"changed destination")
    assert not verify(active, proof, payload, generation=generation + 1)
    other = Keypair.create_from_seed(bytes([94]) * 32, active.crypto_type).at_generation(generation)
    assert not verify(active, proof, payload, descriptor=other.hashed_descriptor)
    assert not verify(active, proof, payload, commitment=other.hashed_current_commitment)


@pytest.mark.parametrize("crypto", CLASSICAL_HASHED_CRYPTO_TYPES)
@pytest.mark.parametrize("generation", [0, 1, 255, 2**32, 2**64 - 2])
def test_independent_hkdf_generation_vectors(crypto, generation):
    key = Keypair.create_from_seed(MASTER, crypto).at_generation(generation)
    scheme = 1 if crypto == 4 else 3
    prk = hmac.digest(b"bittensor/hashed/v1/derive", MASTER, "sha256")
    info = bytes([1, scheme]) + generation.to_bytes(8, "little")
    seed = hmac.digest(prk, info + b"\1", "sha256")
    signing_type = 1 if crypto == 4 else 0
    independent = Keypair.create_from_seed(seed, signing_type)
    assert key.hashed_public_key == independent.public_key
    assert seed != MASTER


def test_profile_and_generation_derivation_separation():
    ids = set()
    commitments = set()
    for crypto in NEW_TYPES:
        key = Keypair.create_from_seed(MASTER, crypto)
        assert bytes(key.public_key) not in ids
        ids.add(bytes(key.public_key))
        generations = [0] if crypto in FIXED_PQ_CRYPTO_TYPES else [0, 1, 2, 255]
        for generation in generations:
            active = key.at_generation(generation)
            commitment = bytes(active.hashed_current_commitment)
            assert commitment not in commitments
            commitments.add(commitment)
            assert key.public_key == active.public_key


@pytest.mark.parametrize("crypto", CLASSICAL_HASHED_CRYPTO_TYPES)
def test_offchain_message_cannot_be_reused_as_transaction_authority(crypto):
    key = Keypair.create_from_seed(MASTER, crypto)
    with pytest.raises(ValueError, match="generation zero"):
        key.sign(b"arbitrary message before retirement")
    active = key.at_generation(1)
    payload = b"hostile website asks you to sign these transaction-looking bytes"
    message = bytes(active.sign(payload))
    assert active.public_only().verify(payload, message)
    assert not verify(active, message, payload)
    transaction = bytes(active.sign_hashed(payload))
    assert not active.public_only().verify(payload, transaction)


@pytest.mark.parametrize("crypto", PQ_CRYPTO_TYPES)
def test_pq_generic_message_interface_refuses_spending_key_reuse(crypto):
    key = Keypair.create_from_seed(MASTER, crypto)
    with pytest.raises(ValueError, match="not supported"):
        key.sign(b"arbitrary website message")


@pytest.mark.parametrize("crypto", FIXED_PQ_CRYPTO_TYPES)
@pytest.mark.parametrize("encrypted", [False, True])
async def test_mismatched_public_companion_must_not_switch_spending_account(
    tmp_path, crypto, encrypted
):
    victim = Keypair.create_from_seed(bytes([94]) * 32, crypto)
    displayed = Keypair.create_from_seed(bytes([95]) * 32, crypto)
    wallet = Wallet("victim", path=str(tmp_path))
    wallet.coldkey_file.set_keypair(victim, encrypt=encrypted, password="test-only-password")
    wallet.coldkeypub_file.set_keypair(displayed.public_only(), encrypt=False)
    signer = WalletSigner(wallet, password="test-only-password")
    assert signer.ss58_address == displayed.ss58_address
    conn, codec = connection(displayed, pending=0)
    # A stale/replaced public companion must fail before producing a signature
    # for another account, whether or not that account happens to use nonce 0.
    with pytest.raises(ValueError, match=r"identity|wallet|account|descriptor"):
        await conn.create_signed_extrinsic(b"transfer reviewed as displayed account", signer)
    assert not hasattr(codec, "proof")
    assert signer._keypair is None
    assert signer.ss58_address == displayed.ss58_address


@pytest.mark.parametrize("crypto", [0, 1, *NEW_TYPES])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_wallet_unlock_rejects_mismatched_companion_for_every_scheme(tmp_path, crypto, role):
    private = Keypair.create_from_seed(MASTER, crypto)
    public = Keypair.create_from_seed(bytes([94]) * 32, crypto).public_only()
    wallet = Wallet("mismatch", path=str(tmp_path))
    getattr(wallet, role + "_file").set_keypair(private, encrypt=False)
    getattr(wallet, role + "pub_file").set_keypair(public, encrypt=False)
    signer = WalletSigner(wallet, role)
    with pytest.raises(ValueError, match="signing identity"):
        signer.sign(b"must never authorize another public companion")
    assert signer._keypair is None
    assert signer.ss58_address == public.ss58_address


@pytest.mark.parametrize("crypto", FIXED_PQ_CRYPTO_TYPES)
@pytest.mark.parametrize("descriptor", [False, True])
async def test_matching_encrypted_wallet_and_address_only_companion_sign(
    tmp_path, crypto, descriptor
):
    private = Keypair.create_from_seed(MASTER, crypto)
    wallet = Wallet("matching", path=str(tmp_path))
    wallet.coldkey_file.set_keypair(private, encrypt=True, password="test-only-password")
    public = (
        private.public_only()
        if descriptor
        else Keypair(ss58_address=private.ss58_address, crypto_type=crypto)
    )
    wallet.coldkeypub_file.set_keypair(public, encrypt=False)
    signer = WalletSigner(wallet, password="test-only-password")
    conn, codec = connection(public, pending=0)
    signed = await conn.create_signed_extrinsic(b"reviewed call", signer)
    assert signed.signer_address == private.ss58_address
    assert verify(private, codec.proof, b"reviewed call" + bytes(4) + b"runtime-genesis-era")


def signer_view(key):
    return SimpleNamespace(
        ss58_address=key.ss58_address,
        public_key=bytes(key.public_key),
        crypto_type=key.crypto_type,
        hashed_descriptor=bytes(key.hashed_descriptor),
        hashed_current_commitment=bytes(key.hashed_current_commitment),
        sign_hashed=Mock(side_effect=AssertionError("must reject before signing")),
    )


@pytest.mark.parametrize("crypto", FIXED_PQ_CRYPTO_TYPES)
@pytest.mark.parametrize("in_place", [False, True])
@pytest.mark.parametrize(
    "field", ["ss58_address", "public_key", "crypto_type", "hashed_descriptor"]
)
async def test_fixed_generation_hook_cannot_replace_any_identity_field(crypto, in_place, field):
    private = Keypair.create_from_seed(MASTER, crypto)
    other = Keypair.create_from_seed(bytes([94]) * 32, crypto)
    signer = signer_view(private)
    replacement = signer if in_place else signer_view(private)

    def derive(generation):
        assert generation == 0
        value = 7 if field == "crypto_type" else getattr(other, field)
        setattr(replacement, field, value)
        return replacement

    signer.at_generation = derive
    conn, codec = connection(private, pending=0)
    with pytest.raises(ValueError, match="signing identity"):
        await conn.create_signed_extrinsic(b"reviewed call", signer)
    signer.sign_hashed.assert_not_called()
    replacement.sign_hashed.assert_not_called()
    assert not hasattr(codec, "proof")


@pytest.mark.parametrize("crypto", [4, 6, 7, 10, 12])
@pytest.mark.parametrize("offset", [0, 1])
async def test_rotating_generation_hook_cannot_switch_account_even_for_shield_inner(crypto, offset):
    private = Keypair.create_from_seed(MASTER, crypto)
    other = Keypair.create_from_seed(bytes([94]) * 32, crypto)
    signer = signer_view(private)
    current = signer_view(private.at_generation(7))
    replacement = signer_view(other.at_generation(7 + offset))
    current.at_generation = Mock(return_value=replacement)
    signer.at_generation = Mock(return_value=current if offset else replacement)
    conn, codec, _ = rotating_connection(private)
    snapshot = await conn.prepare_hashed_snapshot(signer)
    with pytest.raises(ValueError, match="signing identity"):
        await conn.create_signed_extrinsic(
            b"reviewed call",
            signer,
            nonce=13 + offset,
            hashed_generation_offset=offset,
            hashed_signing_snapshot=snapshot,
        )
    signer.sign_hashed.assert_not_called()
    current.sign_hashed.assert_not_called()
    replacement.sign_hashed.assert_not_called()
    assert not hasattr(codec, "proof")


@pytest.mark.parametrize("crypto", [1, 8])
def test_legacy_hotkey_without_public_companion_still_unlocks(tmp_path, crypto):
    private = Keypair.create_from_seed(MASTER, crypto)
    wallet = Wallet("legacy", path=str(tmp_path))
    wallet.hotkey_file.set_keypair(private, encrypt=False)
    signer = WalletSigner(wallet, "hotkey")
    assert signer.ss58_address == private.ss58_address
    if crypto == 1:
        signature = signer.sign(b"legacy hotkey")
        assert private.public_only().verify(b"legacy hotkey", signature)
    else:
        assert signer.at_generation(0).public_key == private.public_key
    assert not wallet.hotkeypub_file.exists_on_device()


@pytest.mark.parametrize("crypto", FIXED_PQ_CRYPTO_TYPES)
async def test_fixed_fee_quote_does_not_unlock_or_select_generation(crypto):
    private = Keypair.create_from_seed(MASTER, crypto)
    public = signer_view(private)
    public.at_generation = Mock(side_effect=AssertionError("quote must not unlock"))
    conn, _ = connection(private, pending=0)
    await conn.create_signed_extrinsic(b"quote", public, signature=bytes(64))
    public.at_generation.assert_not_called()
    public.sign_hashed.assert_not_called()


@pytest.mark.parametrize("crypto", [0, 1, *NEW_TYPES])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("matching", [False, True])
def test_eagerly_unlocked_wallet_keeps_the_reviewed_public_identity(crypto, role, matching):
    private = Keypair.create_from_seed(MASTER, crypto)
    public = (
        private if matching else Keypair.create_from_seed(bytes([94]) * 32, crypto)
    ).public_only()
    # CLI encrypted-wallet paths unlock before constructing this wallet shape.
    # Checking only WalletSigner would not protect these submissions.
    wallet = SimpleNamespace(coldkey=private, coldkeypub=public, hotkey=private, hotkeypub=public)
    if matching:
        assert resolve_signer(wallet, role) is private
    else:
        with pytest.raises(ValueError, match="signing identity"):
            resolve_signer(wallet, role)


@pytest.mark.parametrize("crypto", FIXED_PQ_CRYPTO_TYPES)
async def test_plan_execute_rejects_eager_unlock_identity_change_before_submission(crypto):
    from bittensor.executor import Executor
    from bittensor.intents import Transfer
    from tests.unit.test_mldsa_accounts import chain

    private = Keypair.create_from_seed(MASTER, crypto)
    displayed = Keypair.create_from_seed(bytes([94]) * 32, crypto).public_only()
    wallet = SimpleNamespace(coldkey=private, coldkeypub=displayed, hotkey=private)
    substrate = chain()
    substrate.seed(
        "System", "Account", [displayed.ss58_address], {"nonce": 0, "data": {"free": 10**14}}
    )
    executor = Executor(substrate)
    intent = Transfer(Keypair.create_from_seed(bytes([95]) * 32, 1).ss58_address, 1)
    plan = await executor.plan(intent, wallet, shielded=False)
    assert plan.ok and plan.signer_address == displayed.ss58_address
    with pytest.raises(ValueError, match="signing identity"):
        await executor.execute(intent, wallet, shielded=False)
    assert substrate.submissions == []


@pytest.mark.parametrize("crypto", [8, 9, 11])
async def test_plan_cannot_change_signer_after_quote(crypto):
    from bittensor.executor import Executor
    from bittensor.intents import Transfer
    from tests.unit.test_mldsa_accounts import chain

    private = Keypair.create_from_seed(MASTER, crypto)
    displayed = Keypair.create_from_seed(bytes([96]) * 32, crypto).public_only()
    wallet = SimpleNamespace(coldkey=private, coldkeypub=displayed, hotkey=private)
    substrate = chain()
    substrate.seed(
        "System", "Account", [displayed.ss58_address], {"nonce": 0, "data": {"free": 10**14}}
    )
    executor = Executor(substrate)
    intent = Transfer(Keypair.create_from_seed(bytes([97]) * 32, 1).ss58_address, 1)
    original_plan = executor.plan

    async def change_public_after_plan(*args, **kwargs):
        plan = await original_plan(*args, **kwargs)
        assert plan.ok and plan.signer_address == displayed.ss58_address
        wallet.coldkeypub = private.public_only()
        return plan

    executor.plan = change_public_after_plan
    with pytest.raises(ValueError, match="signing identity does not match the planned account"):
        await executor.execute(intent, wallet, shielded=False)
    assert substrate.submissions == []


@pytest.mark.parametrize("crypto", [8, 9, 11])
def test_wallet_signer_pins_public_companion_during_unlock(tmp_path, crypto):
    private = Keypair.create_from_seed(MASTER, crypto)
    displayed = Keypair.create_from_seed(bytes([98]) * 32, crypto).public_only()
    wallet = Wallet("pinned", path=str(tmp_path))
    wallet.coldkey_file.set_keypair(private, encrypt=True, password="test-only-password")
    wallet.coldkeypub_file.set_keypair(displayed, encrypt=False)
    signer = WalletSigner(wallet, password="test-only-password")
    assert signer.ss58_address == displayed.ss58_address
    wallet.coldkeypub_file.set_keypair(private.public_only(), encrypt=False, overwrite=True)
    with pytest.raises(ValueError, match="unlocked signing identity does not match"):
        signer.at_generation(0)
    assert signer._keypair is None


@pytest.mark.parametrize("crypto", [8, 9, 11])
async def test_shield_quote_cannot_change_the_fee_payer_before_signing(crypto):
    from bittensor.executor import Executor
    from bittensor.intents import Transfer
    from tests.unit.test_mldsa_accounts import chain

    private = Keypair.create_from_seed(MASTER, crypto)
    displayed = Keypair.create_from_seed(bytes([99]) * 32, crypto).public_only()
    wallet = SimpleNamespace(coldkey=private, coldkeypub=displayed, hotkey=private)
    substrate = chain()
    substrate.mev_key = bytes(1184)
    substrate.seed(
        "System", "Account", [displayed.ss58_address], {"nonce": 0, "data": {"free": 10**14}}
    )
    executor = Executor(substrate)
    executor._submit_encrypted_call = AsyncMock()
    original_quote = executor._quote_submission

    async def change_public_after_quote(*args, **kwargs):
        quoted = await original_quote(*args, **kwargs)
        wallet.coldkeypub = private.public_only()
        return quoted

    executor._quote_submission = change_public_after_quote
    intent = Transfer(Keypair.create_from_seed(bytes([100]) * 32, 1).ss58_address, 1)
    with pytest.raises(ValueError, match="signing identity does not match the planned account"):
        await executor.submit_shielded(intent, wallet)
    executor._submit_encrypted_call.assert_not_awaited()
