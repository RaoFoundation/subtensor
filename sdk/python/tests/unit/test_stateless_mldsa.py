"""Standard MS signs direct calls without recipient or signer registration."""

import asyncio
from dataclasses import replace
from hashlib import blake2b
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import bittensor_core
import pytest

from bittensor._substrate import RpcSubstrate
from bittensor._transport.contract import HashedSigningSnapshot, SignedExtrinsic
from bittensor._transport.errors import ExtrinsicSubmissionRejected, SubstrateRequestException
from bittensor._transport.extrinsics import (
    SHIELD_ENCRYPTION_OVERHEAD,
    NonceCache,
    hashed_proof_length,
)
from bittensor._transport.interface import SubstrateConnection
from bittensor.cli.wallet_addresses import address_fields, check_registration
from bittensor.executor import Executor, _is_transient
from bittensor.hashed import with_recipient_registration
from bittensor.intents import Transfer
from bittensor.mldsa_signing import prepare_mldsa_extrinsic, sign_prepared_mldsa
from bittensor.receiving import receiving_address
from bittensor.result import ChainError, ExtrinsicResult
from bittensor.sp_core import (
    PQ_PROFILES,
    Keypair,
    is_stateless_pq,
    pq_lengths,
    requires_registration,
)
from bittensor.wallet import Wallet
from tests.conftest import golden_codec
from tests.unit.test_mldsa_accounts import GENESIS, Codec
from tests.unit.test_mldsa_signing import OfflineCodec


def mock_key(crypto_type=8):
    key = SimpleNamespace(
        crypto_type=crypto_type,
        public_key=bytes([9]) * 32,
        ss58_address=Keypair.create_from_uri("//Alice").ss58_address,
        hashed_descriptor=bytes((2, PQ_PROFILES[crypto_type])) + bytes([7]) * 64,
    )
    public_length, signature_length = pq_lengths(crypto_type)
    proof = (
        key.public_key
        + key.hashed_descriptor[:2]
        + bytes(8 + public_length)
        + bytes([7]) * 64
        + bytes(signature_length)
    )
    key.sign_hashed = Mock(return_value=proof)
    key.at_generation = Mock(return_value=key)
    return key


def connection(key, *, enabled=True, profiles=bytes([4, 5, 6, 7, 8, 9]), pending=13):
    codec = Codec()
    codec.constant = Mock(return_value=profiles)
    conn = SubstrateConnection("ws://unused.invalid")
    conn._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    conn.get_chain_finalised_head = AsyncMock(return_value="0xfinalized")
    conn.genesis_hash = AsyncMock(return_value=GENESIS)
    conn._nonces = NonceCache(SimpleNamespace(request=AsyncMock(return_value=pending)))

    async def query(pallet, item, params=None, *, block_hash):
        assert block_hash == "0xfinalized"
        if (pallet, item) == ("AdminUtils", "HashedAccountsEnabled"):
            return enabled
        assert (pallet, item) == ("System", "Account"), "registration must never be queried"
        assert params == [key.ss58_address]
        return {"nonce": 3}

    conn.query = AsyncMock(side_effect=query)
    return conn, codec


@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_stateless_signer_queues_native_nonces_without_authority_reads(
    crypto_type,
):
    key = mock_key(crypto_type)
    conn, codec = connection(key)
    await conn.create_signed_extrinsic(b"first", key)
    assert codec.nonce == 13
    await conn.create_signed_extrinsic(b"second", key)
    assert codec.nonce == 14
    assert all(call.args[0] == 0 for call in key.at_generation.call_args_list)
    assert all(
        call.args[:2] == ("AdminUtils", "HashedAccountsEnabled")
        for call in conn.query.call_args_list
    )
    assert len(codec.proof) == hashed_proof_length(crypto_type)


@pytest.mark.parametrize("enabled,profiles", [(False, bytes([4, 5])), (True, bytes([4]))])
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_disabled_or_old_runtime_rejects_before_private_signing(
    crypto_type, enabled, profiles
):
    key = mock_key(crypto_type)
    conn, _ = connection(key, enabled=enabled, profiles=profiles)
    with pytest.raises(SubstrateRequestException):
        await conn.create_signed_extrinsic(b"call", key)
    key.at_generation.assert_not_called()
    key.sign_hashed.assert_not_called()
    assert conn._nonces._nonces == {}


@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_shield_pair_reserves_nonces_and_keeps_generation_zero(
    crypto_type,
):
    key = mock_key(crypto_type)
    conn, codec = connection(key, pending=63)
    snapshot = await conn.prepare_hashed_snapshot(key, reserve_nonces=True)
    assert (snapshot.nonce, snapshot.generation) == (63, 0)
    await conn.create_signed_extrinsic(
        b"inner", key, nonce=64, hashed_generation_offset=1, hashed_signing_snapshot=snapshot
    )
    assert codec.nonce == 64
    await conn.create_signed_extrinsic(b"outer", key, nonce=63, hashed_signing_snapshot=snapshot)
    assert codec.nonce == 63
    await conn.create_signed_extrinsic(b"following", key)
    assert codec.nonce == 65
    assert all(call.args[0] == 0 for call in key.at_generation.call_args_list)


async def test_concurrent_nonce_reservations_cannot_split_shield_pair():
    cache = NonceCache(SimpleNamespace(request=AsyncMock(return_value=63)))
    first, second = await asyncio.gather(cache.reserve("account", 2), cache.reserve("account"))
    assert (first, second) == (63, 65)


async def test_explicit_lower_nonce_cannot_reclaim_pending_shield_inner():
    cache = NonceCache(SimpleNamespace(request=AsyncMock(return_value=63)))
    assert await cache.reserve("account", 2) == 63
    cache.pin("account", 63)
    assert await cache.reserve("account") == 65


@pytest.mark.parametrize("failure", ["sign", "pool", "disconnect"])
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_rpc_shield_failure_preserves_other_local_reservations(crypto_type, failure):
    key = mock_key(crypto_type)
    conn, _ = connection(key, pending=63)
    snapshot = await conn.prepare_hashed_snapshot(key, reserve_nonces=True)
    assert await conn._nonces.next_for(key.ss58_address) == 65
    conn.create_signed_extrinsic = AsyncMock(return_value=object())
    conn.submit_extrinsic = AsyncMock()
    if failure == "sign":
        conn.create_signed_extrinsic.side_effect = ValueError("signing refused")
    elif failure == "pool":
        conn.submit_extrinsic.side_effect = ExtrinsicSubmissionRejected("Invalid Transaction")
    else:
        conn.submit_extrinsic.side_effect = OSError("connection lost")
    substrate = RpcSubstrate("ws://unused.invalid")
    substrate._substrate = conn
    if failure == "pool":
        result = await substrate.submit(b"call", key, hashed_signing_snapshot=snapshot)
        assert not result.success
    else:
        with pytest.raises(ChainError):
            await substrate.submit(b"call", key, hashed_signing_snapshot=snapshot)
    # A connection loss cannot establish that the hidden inner was abandoned.
    assert await conn._nonces.next_for(key.ss58_address) == (66 if failure == "disconnect" else 63)


@pytest.mark.parametrize("concurrent", [False, True])
async def test_shield_retry_reuses_expired_inner_nonce_without_skipping_gap(concurrent):
    session = SimpleNamespace(request=AsyncMock(return_value=63))
    cache = NonceCache(session)
    assert await cache.reserve("account", 2) == 63
    if concurrent:
        assert await cache.reserve("account") == 65
    cache.release("account", 63, 2)
    session.request.return_value = 64
    if concurrent:
        with pytest.raises(ValueError, match="nonce gap"):
            await cache.reserve("account", 2)
        assert await cache.reserve("account") == 64
        assert await cache.reserve("account", 2) == 66
    else:
        assert await cache.reserve("account", 2) == 64
        assert await cache.reserve("account") == 66


@pytest.mark.parametrize(
    "failure",
    [
        "RPC session closed",
        "connection lost while waiting for a subscription",
        {"error": {"code": 1013, "message": "Transaction Already Imported"}},
    ],
)
@pytest.mark.parametrize("accepted_watch", [False, True])
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_shield_unknown_rpc_result_keeps_hidden_inner_nonce(
    crypto_type, monkeypatch, failure, accepted_watch
):
    key = mock_key(crypto_type)
    conn, _ = connection(key, pending=63)
    conn.create_signed_extrinsic = AsyncMock(return_value=SignedExtrinsic(b"carrier", "0xcarrier"))
    error = SubstrateRequestException(failure)
    conn._session = SimpleNamespace(subscribe=AsyncMock())
    if accepted_watch:
        from tests.unit.test_submission_admission import Watch

        conn._session.subscribe.return_value = Watch(error=error)
    else:
        conn._session.subscribe.side_effect = error
    substrate = RpcSubstrate("ws://unused.invalid")
    substrate._substrate = conn
    substrate.mev_next_key = AsyncMock(return_value=bytes(1184))
    substrate.sign_extrinsic = AsyncMock(return_value=(b"inner", "0xinner"))
    substrate.compose = AsyncMock(return_value=b"outer")
    monkeypatch.setattr(
        "bittensor.executor._core.encrypt_mlkem768", lambda *_args, **_kwargs: b"ciphertext"
    )
    result = await Executor(substrate)._submit_encrypted_call(
        b"call", key, {}, period=8, wait_for_inclusion=True, wait_for_finalization=True
    )
    assert not result.success
    # The node knows the carrier but cannot see the encrypted inner yet.
    conn._nonces._session.request.return_value = 64
    assert await conn._nonces.next_for(key.ss58_address) == 65
    assert result.data["submission_outcome_unknown"] is True
    assert not _is_transient(replace(result, message="Priority is too low"))


@pytest.mark.parametrize("failure", ["sign", "pool", "disconnect"])
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_ordinary_fixed_failure_preserves_concurrent_shield_pair(crypto_type, failure):
    key = mock_key(crypto_type)
    conn, _ = connection(key, pending=63)
    await conn.prepare_hashed_snapshot(key, reserve_nonces=True)
    conn.create_signed_extrinsic = AsyncMock(return_value=object())
    conn.submit_extrinsic = AsyncMock()
    if failure == "sign":
        conn.create_signed_extrinsic.side_effect = ValueError("signing refused")
    elif failure == "pool":
        conn.submit_extrinsic.side_effect = ExtrinsicSubmissionRejected("Invalid Transaction")
    else:
        conn.submit_extrinsic.side_effect = OSError("connection lost")
    substrate = RpcSubstrate("ws://unused.invalid")
    substrate._substrate = conn
    if failure == "pool":
        result = await substrate.submit(b"call", key)
        assert not result.success
    else:
        with pytest.raises(ChainError):
            await substrate.submit(b"call", key)
    assert conn.create_signed_extrinsic.call_args.kwargs["nonce"] == 65
    assert await conn._nonces.next_for(key.ss58_address) == (66 if failure == "disconnect" else 65)


@pytest.mark.parametrize("failure", ["rpc", "disconnect"])
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_nonce_lookup_failure_preserves_existing_shield_reservations(crypto_type, failure):
    key = mock_key(crypto_type)
    conn, _ = connection(key, pending=63)
    await conn.prepare_hashed_snapshot(key, reserve_nonces=True)
    conn.create_signed_extrinsic = AsyncMock()
    conn._nonces._session.request.side_effect = (
        SubstrateRequestException("nonce lookup refused")
        if failure == "rpc"
        else OSError("connection lost")
    )
    substrate = RpcSubstrate("ws://unused.invalid")
    substrate._substrate = conn
    if failure == "rpc":
        assert not (await substrate.submit(b"call", key)).success
    else:
        with pytest.raises(ChainError):
            await substrate.submit(b"call", key)
    conn.create_signed_extrinsic.assert_not_called()
    conn._nonces._session.request.side_effect = None
    assert await asyncio.gather(
        conn._nonces.next_for(key.ss58_address), conn._nonces.next_for(key.ss58_address)
    ) == [65, 66]


@pytest.mark.parametrize("method", ["submit_signed", "submit_signature"])
@pytest.mark.parametrize("failure", ["pool", "disconnect"])
async def test_external_fixed_submit_failure_preserves_unrelated_reservations(
    native_key, method, failure
):
    conn, _ = connection(native_key, pending=63)
    snapshot = await conn.prepare_hashed_snapshot(native_key, reserve_nonces=True)
    assert await conn._nonces.next_for(native_key.ss58_address) == 65
    conn.submit_extrinsic = AsyncMock(
        side_effect=(
            ExtrinsicSubmissionRejected("Invalid Transaction")
            if failure == "pool"
            else OSError("connection lost")
        )
    )
    substrate = RpcSubstrate("ws://unused.invalid")
    substrate._substrate = conn
    if method == "submit_signed":
        args = (object(), native_key)
    else:
        codec = OfflineCodec()
        codec.constant = lambda *_: bytes([4, 5])
        prepared = prepare_mldsa_extrinsic(
            codec,
            b"external call",
            replace(snapshot, nonce=66),
            era="00",
            genesis_hash=GENESIS,
            era_block_hash=GENESIS,
        )
        conn.attach_signature = AsyncMock(return_value=object())
        args = (prepared, bytes(5367))
    if failure == "pool":
        assert not (await getattr(substrate, method)(*args)).success
    else:
        with pytest.raises(ChainError):
            await getattr(substrate, method)(*args)
    # Neither the hidden inner nonce nor another locally reserved transaction
    # belongs to this externally supplied extrinsic's failure cleanup.
    assert await conn._nonces.next_for(native_key.ss58_address) == 66


@pytest.mark.parametrize(
    "failure", ["sign", "oversize", "compose", "carrier", "expired", "inner", "pending"]
)
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_shield_abandoned_nonces_recover_without_reusing_concurrent_reservation(
    crypto_type, monkeypatch, failure
):
    key = mock_key(crypto_type)
    conn, _ = connection(key, pending=63)

    async def sign(*_args, **_kwargs):
        # A second caller reserves its own transaction while Shield is preparing.
        assert await conn._nonces.next_for(key.ss58_address) == 65
        if failure == "sign":
            raise ValueError("signing aborted")
        return b"inner", "0xinner"

    substrate = SimpleNamespace(
        mev_next_key=AsyncMock(return_value=bytes(1184)),
        prepare_hashed_snapshot=conn.prepare_hashed_snapshot,
        release_nonce_reservation=conn.release_nonce_reservation,
        sign_extrinsic=sign,
        compose=AsyncMock(return_value=b"outer"),
        submit=AsyncMock(
            return_value=ExtrinsicResult(success=failure != "carrier", message="carrier")
        ),
    )
    if failure == "compose":
        substrate.compose.side_effect = ValueError("composition aborted")
    monkeypatch.setattr(
        "bittensor.executor._core.encrypt_mlkem768",
        lambda *_args, **_kwargs: bytes(32769) if failure == "oversize" else b"ciphertext",
    )
    executor = Executor(substrate)
    executor._resolve_shielded_inner = AsyncMock(
        return_value=ExtrinsicResult(
            success=False,
            message="inner failed or expired",
            error=ChainError(
                "inner failed or expired",
                "ShieldInnerExpired" if failure == "expired" else "CallFailed",
            ),
        )
    )
    options = dict(period=8, wait_for_inclusion=failure != "pending", wait_for_finalization=False)
    if failure in ("sign", "oversize", "compose"):
        with pytest.raises(ValueError):
            await executor._submit_encrypted_call(b"call", key, {}, **options)
        substrate.submit.assert_not_called()
        expected = 63
    else:
        result = await executor._submit_encrypted_call(b"call", key, {}, **options)
        assert result.success == (failure == "pending")
        # A failed inner still consumed n+1; an expired/missing inner did not.
        conn._nonces._session.request.return_value = 65 if failure == "inner" else 64
        expected = 66 if failure in ("inner", "pending") else 64
    assert await conn._nonces.next_for(key.ss58_address) == expected
    # Neither release nor RPC reconciliation can reclaim the other caller's 65.
    assert await conn._nonces.next_for(key.ss58_address) != 65


@pytest.mark.parametrize("outcome", ["expired", "unfinalized", "unavailable"])
async def test_shield_expiration_requires_observed_finalized_era_end(outcome):
    from tests.harness.fake_substrate import success_result

    substrate = SimpleNamespace(
        block_hash=AsyncMock(
            side_effect=lambda block: None if outcome == "unavailable" else f"0x{block:064x}"
        ),
        block_time=AsyncMock(return_value=0),
        find_extrinsic=AsyncMock(return_value=None),
        finalized_block_number=AsyncMock(return_value=11 if outcome == "expired" else 10),
    )
    executor = Executor(substrate)
    if outcome == "expired":
        result = await executor._resolve_shielded_inner(success_result(10), "0xinner", 1)
        assert not result.success and result.error.name == "ShieldInnerExpired"
    else:
        with pytest.raises(ChainError, match=r"expired|finalized"):
            await executor._resolve_shielded_inner(success_result(10), "0xinner", 1)


@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_fee_dummy_has_full_proof_and_never_unlocks_or_reads_registry(
    crypto_type,
):
    key = mock_key(crypto_type)
    conn, codec = connection(key)
    await conn.sign_without_nonce_tracking(b"call", key, nonce=64, signature=bytes(64))
    assert codec.nonce == 64
    assert len(codec.proof) == hashed_proof_length(crypto_type)
    assert codec.proof[32:42] == bytes((2, PQ_PROFILES[crypto_type])) + bytes(8)
    public_length, signature_length = pq_lengths(crypto_type)
    assert codec.proof[42 : 42 + public_length] == bytes(public_length)
    assert codec.proof[106 + public_length :] == bytes(signature_length)
    key.at_generation.assert_not_called()
    key.sign_hashed.assert_not_called()


@pytest.mark.parametrize("crypto_type", [8, 9, 11])
def test_standard_ms_has_one_address_and_no_registration_lookup(
    crypto_type,
):
    key = mock_key(crypto_type)
    assert is_stateless_pq(key) and not requires_registration(key)
    assert receiving_address(key) == key.ss58_address
    fields = address_fields(key, network="finney")
    assert fields == {"ss58": key.ss58_address}
    check_registration(SimpleNamespace(), [fields])
    rotating = SimpleNamespace(crypto_type=7)
    assert requires_registration(rotating)


@pytest.fixture
def native_key():
    key = Keypair.create_from_seed(bytes([23]) * 32, 8)
    assert bytes(key.hashed_descriptor)[:2] == b"\x02\x05", "rebuild bittensor-core for fixed MS"
    return key


def test_native_seed_restore_and_watch_only_keep_standard_identity(native_key, tmp_path):
    wallet = Wallet("standard", path=str(tmp_path))
    wallet.regenerate_coldkey(
        seed=bytes([23]) * 32, crypto_type=8, use_password=False, suppress=True
    )
    assert wallet.coldkeypub.ss58_address == native_key.ss58_address
    assert receiving_address(wallet.coldkeypub) == native_key.ss58_address
    watch = Wallet("watch", path=str(tmp_path))
    watch.regenerate_coldkeypub(ss58_address=native_key.ss58_address, crypto_type=8)
    assert watch.coldkeypub.ss58_address == native_key.ss58_address
    assert not requires_registration(watch.coldkeypub)


async def test_native_local_recipient_does_not_add_setup_or_deposit(native_key, tmp_path):
    recipient = Wallet("recipient", path=str(tmp_path))
    recipient.regenerate_coldkey(
        seed=bytes([23]) * 32, crypto_type=8, use_password=False, suppress=True
    )
    chain = SimpleNamespace(query=AsyncMock(side_effect=AssertionError("no registry lookup")))
    intent = Transfer(dest_ss58=native_key.ss58_address, amount_tao=1)
    call = object()
    result, extras = await with_recipient_registration(chain, recipient, intent, call)
    assert result is call and extras == {}
    chain.query.assert_not_called()


def test_native_offline_proof_uses_nonce_but_no_generation(native_key):
    descriptor = bytes(native_key.hashed_descriptor)
    authority = HashedSigningSnapshot(
        "0xfinalized", native_key.ss58_address, 8, descriptor, 0, descriptor[2:], 64
    )
    codec = OfflineCodec()
    codec.constant = lambda *_: bytes([4, 5])
    prepared = prepare_mldsa_extrinsic(
        codec, b"call", authority, era="00", genesis_hash=GENESIS, era_block_hash=GENESIS
    )
    proof = sign_prepared_mldsa(prepared, native_key, codec=codec, expected_genesis_hash=GENESIS)
    assert len(proof) == 5367 and proof[34:42] == bytes(8)
    with pytest.raises(ValueError, match="invalid versioned"):
        replace(prepared, authority=replace(authority, generation=1)).validate()
    with pytest.raises(ValueError, match="invalid versioned"):
        replace(
            prepared,
            unsigned=replace(prepared.unsigned, nonce=2**32 - 1),
            authority=replace(authority, nonce=2**32 - 1),
        ).validate()


@pytest.mark.parametrize("nonce", [63, 16383, 2**30 - 1])
@pytest.mark.parametrize("carrier_size", [8192, 8193, 32768, 32769])
async def test_stateless_shield_quote_matches_native_proof_and_ciphertext_at_nonce_boundaries(
    native_key, nonce, carrier_size
):
    class CompactNonceCodec(Codec):
        def encode_signed_extrinsic(self, call, **kwargs):
            body, _ = super().encode_signed_extrinsic(call, **kwargs)
            body += golden_codec().encode_compact(kwargs["nonce"])
            body = golden_codec().encode_compact(len(body)) + body
            return body, "0x" + blake2b(body, digest_size=32).hexdigest()

    conn, _ = connection(native_key, pending=nonce)
    codec = CompactNonceCodec()
    conn._runtimes.codec_at.return_value = codec
    era = {"period": 8, "current": 100}
    conn._normalize_era_at = AsyncMock(return_value=(era, GENESIS))
    conn.compose_call = AsyncMock(return_value=b"carrier")
    conn.get_payment_info = AsyncMock(return_value={"partial_fee": 17})
    size = carrier_size - SHIELD_ENCRYPTION_OVERHEAD - 5367 - 8
    for _ in range(4):
        call = bytes([42]) * size
        actual = await conn.sign_without_nonce_tracking(call, native_key, nonce=nonce + 1, era=era)
        delta = carrier_size - SHIELD_ENCRYPTION_OVERHEAD - len(actual.data)
        if not delta:
            break
        size += delta
    encrypted = bittensor_core.encrypt_mlkem768(bytes(1184), actual.data, include_key_hash=True)
    assert len(encrypted) == carrier_size
    public = native_key.public_only()
    if carrier_size > 32768:
        with pytest.raises(ValueError, match="32768-byte carrier limit"):
            await conn.get_shielded_payment_info(call, public, period=8)
        conn.compose_call.assert_not_called()
    else:
        await conn.get_shielded_payment_info(call, public, period=8)
        assert codec.nonce == nonce + 1
        assert len(conn.compose_call.call_args.args[2]["ciphertext"]) == len(encrypted)
        assert conn.compose_call.call_args.kwargs == {"block_hash": "0xfinalized"}
        assert conn.get_payment_info.call_args.kwargs == {
            "nonce": nonce,
            "era": era,
            "_block_hash": "0xfinalized",
        }
    assert conn._nonces._nonces == {}


async def test_fixed_client_plan_preserves_signing_scheme_and_enforces_large_carrier_fee(
    native_key, monkeypatch
):
    from bittensor import Balance, Client, Policy
    from bittensor.intents import Batch
    from bittensor.result import PolicyError
    from tests.harness.samples import BOB
    from tests.unit.test_submission_costs import chain

    substrate = chain()
    wire = golden_codec()

    async def compose(call):
        return call if hasattr(call, "data") else wire.compose_call(*call)

    substrate.compose = compose
    substrate.decode_scale = AsyncMock(side_effect=wire.decode)
    child = wire.compose_call("Balances", "transfer_keep_alive", {"dest": BOB, "value": 10**9})
    for count in range(1, 100):
        call = wire.compose_call("Utility", "batch_all", {"calls": [child] * count})
        if len(wire.call_data(call)) + 5367 + SHIELD_ENCRYPTION_OVERHEAD > 8192:
            break
    carrier_sizes = []

    async def carrier_fee(call, public, *, period):
        assert public.crypto_type == 8
        conn, _ = connection(public)
        conn._normalize_era_at = AsyncMock(
            return_value=({"period": period, "current": 100}, GENESIS)
        )
        conn.compose_call = AsyncMock(
            side_effect=lambda _module, _function, params, **_kwargs: params["ciphertext"]
        )

        async def quote(outer, *_args, **_kwargs):
            carrier_sizes.append(len(outer))
            return {"partial_fee": 700 if len(outer) > 8192 else 70}

        conn.get_payment_info = quote
        info = await conn.get_shielded_payment_info(wire.call_data(call), public, period=period)
        return Balance.from_rao(info["partial_fee"])

    substrate.estimate_shielded_carrier_fee = carrier_fee
    client = Client("local", substrate=substrate)
    intent = Batch([Transfer(BOB, 1) for _ in range(count)])
    policy = Policy(max_fee_tao=Balance.from_rao(170))
    plan = await client.plan(intent, native_key, shielded=True, policy=policy)
    assert carrier_sizes and all(8192 < size < 8250 for size in carrier_sizes)
    assert plan.fee.rao == 800 and not plan.ok
    unlock = Mock(side_effect=AssertionError("fee cap must reject before private signing"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    with pytest.raises(PolicyError, match="max_fee_tao"):
        await client.execute(intent, native_key, shielded=True, policy=policy)
    unlock.assert_not_called()


@pytest.mark.parametrize("route", ["client", "multisig", "cli"])
async def test_carrier_quote_entry_points_retain_fixed_public_key(native_key, route):
    from bittensor import Client
    from bittensor.cli.context import AppContext
    from bittensor.cli.output import Output
    from bittensor.multisig import check_multisig_funds
    from tests.harness.samples import BOB
    from tests.unit.test_submission_costs import chain

    substrate = chain()
    substrate.query = AsyncMock(wraps=substrate.query)
    client = Client("local", substrate=substrate)
    public = native_key.public_only()
    call = ("System", "remark", {"remark": b"fee quote"})
    if route == "client":
        await client.estimate_shielded_carrier_fee(public, call)
    elif route == "multisig":
        await check_multisig_funds(
            substrate,
            signer_ss58=public.ss58_address,
            threshold=2,
            opening=False,
            outer_call=call,
            fee_keypair=public,
            shielded=True,
        )
    else:
        context = AppContext(
            "local", "unused", "default", "/tmp", True, False, Output(json_mode=True)
        )
        assert (
            await context._shield_outer_fee_shortfall(
                client, public.ss58_address, call=call, intent=Transfer(BOB, 1), wallet=native_key
            )
            is None
        )
    assert substrate.estimate_shielded_carrier_fee.await_args.args[1].crypto_type == 8
    assert not any(
        call.args[:2] == ("HashedAccounts", "MlDsaAccounts")
        for call in substrate.query.call_args_list
    )
