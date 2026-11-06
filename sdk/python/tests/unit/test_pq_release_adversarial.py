"""Adversarial release coverage for PQ proofs and approved-order clients."""

from copy import deepcopy
from dataclasses import replace
from unittest.mock import AsyncMock

import pytest

from bittensor.client import Client
from bittensor.limit_orders import authorized_order, prepare_order
from bittensor.sp_core import (
    FIXED_PQ_CRYPTO_TYPES,
    Keypair,
    pq_lengths,
    verify_mldsa_authorization,
)
from bittensor.wallet import Wallet
from tests.harness.samples import ALICE_HOT, BOB
from tests.unit.test_mldsa_orders import OrderSubstrate, order
from tests.unit.test_wallet_crypto_types import invoke
from tests.unit.test_wallet_crypto_types import wallet_path as wallet_path


@pytest.fixture(scope="module", params=[7, 8, 9, 10, 11, 12])
def pq_proof(request):
    wallet = Keypair.create_from_seed(bytes([91]) * 32, request.param)
    generation = 0 if request.param in FIXED_PQ_CRYPTO_TYPES else 2
    signer = wallet.at_generation(generation)
    payload = bytes(range(256)) * 5
    proof = bytes(signer.sign_mldsa(payload))
    authority = (
        bytes(wallet.hashed_descriptor),
        generation,
        bytes(signer.hashed_current_commitment),
    )
    assert verify_mldsa_authorization(proof, payload, *authority)
    return wallet, payload, proof, authority


@pytest.mark.parametrize(
    "field",
    ["account", "version", "profile", "generation", "public_key", "commitment", "signature"],
)
def test_each_proof_component_is_authenticated(pq_proof, field):
    wallet, payload, proof, authority = pq_proof
    public_length, _ = pq_lengths(wallet.crypto_type)
    offset = {
        "account": 0,
        "version": 32,
        "profile": 33,
        "generation": 34,
        "public_key": 42,
        "commitment": 42 + public_length,
        "signature": len(proof) - 1,
    }[field]
    changed = bytearray(proof)
    changed[offset] ^= 1
    assert not verify_mldsa_authorization(bytes(changed), payload, *authority)


@pytest.mark.parametrize("boundary", [0, 32, 34, 42, "public", "commitment", "last", "extra"])
def test_incomplete_or_extended_proofs_never_verify(pq_proof, boundary):
    wallet, payload, proof, authority = pq_proof
    public_length, _ = pq_lengths(wallet.crypto_type)
    if boundary == "extra":
        changed = proof + b"\0"
    else:
        length = {
            "public": 42 + public_length,
            "commitment": 42 + public_length + 64,
            "last": len(proof) - 1,
        }.get(boundary, boundary)
        changed = proof[:length]
    assert not verify_mldsa_authorization(changed, payload, *authority)


@pytest.mark.parametrize("offset", [0, 255, 256, 511, 1279])
def test_full_native_payload_is_bound_across_legacy_hashing_boundary(pq_proof, offset):
    _, payload, proof, authority = pq_proof
    changed = bytearray(payload)
    changed[offset] ^= 1
    assert not verify_mldsa_authorization(proof, bytes(changed), *authority)


@pytest.mark.parametrize("route", ["authorize", "cancel", "execute", "execute_batched"])
@pytest.mark.parametrize(
    "field,value",
    [
        ("signer", ALICE_HOT),
        ("hotkey", BOB),
        ("netuid", 2),
        ("order_type", "LimitSell"),
        ("amount", 101),
        ("limit_price", 1_000_000_001),
        ("expiry", 2_000_000_000_001),
        ("fee_rate", 1),
        ("fee_recipient", ALICE_HOT),
        ("relayer", [ALICE_HOT] * 10),
        ("max_slippage", 1001),
        ("chain_id", 43),
        ("partial_fills_enabled", False),
    ],
)
async def test_every_changed_order_term_is_rejected_before_submission(route, field, value):
    substrate = OrderSubstrate()
    client = Client(substrate=substrate)
    client.execute = AsyncMock()
    prepared = await prepare_order(substrate, 17, order(BOB))
    changed = deepcopy(prepared.order)
    changed["V1"][field] = value
    forged = replace(prepared, order=changed)
    with pytest.raises(ValueError, match="prepared order bytes or chain changed"):
        if route in ("authorize", "cancel"):
            await getattr(client.limit_orders, route)(forged, object())
        elif route == "execute":
            await client.limit_orders.execute([forged.account_approved()], object())
        else:
            await client.limit_orders.execute_batched(1, [forged.account_approved()], object())
    client.execute.assert_not_awaited()


@pytest.mark.parametrize("route", ["authorize", "cancel", "execute", "execute_batched"])
async def test_cross_chain_prepared_orders_are_rejected_before_submission(route):
    substrate = OrderSubstrate()
    client = Client(substrate=substrate)
    client.execute = AsyncMock()
    prepared = await prepare_order(substrate, 17, order(BOB))
    substrate.block_hash = AsyncMock(return_value="0x" + "ab" * 32)
    with pytest.raises(ValueError, match="prepared order bytes or chain changed"):
        if route in ("authorize", "cancel"):
            await getattr(client.limit_orders, route)(prepared, object())
        elif route == "execute":
            await client.limit_orders.execute([prepared.account_approved()], object())
        else:
            await client.limit_orders.execute_batched(1, [prepared.account_approved()], object())
    client.execute.assert_not_awaited()


@pytest.mark.parametrize("nonce", [-1, 2**64, True, False, "1", 1.5])
def test_approval_nonce_cannot_alias_or_wrap_a_u64(nonce):
    with pytest.raises(ValueError, match="order nonce must be a u64 integer"):
        authorized_order(nonce, order(BOB))


@pytest.mark.parametrize("nonce", [0, 2**64 - 1])
async def test_approval_nonce_endpoints_survive_preparation_and_relay(nonce):
    source = order(BOB)
    prepared = await prepare_order(OrderSubstrate(), nonce, source)
    original = deepcopy(prepared.order)
    source["V1"]["relayer"][0] = ALICE_HOT
    approved = prepared.account_approved().value()
    assert approved["order"]["Authorized"] == {"nonce": nonce, "terms": original}
    approved["order"]["Authorized"]["terms"]["V1"]["amount"] += 1
    assert prepared.order == original


@pytest.mark.parametrize(
    "scheme,mode,crypto_type",
    [
        ("falcon512", "standard", 9),
        ("falcon512", "hashed", 10),
        ("falcon1024", "standard", 11),
        ("falcon1024", "hashed", 12),
    ],
)
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("source", ["mnemonic", "seed", "private-key"])
def test_falcon_cli_recovery_restores_authority_in_every_profile(
    wallet_path, scheme, mode, crypto_type, role, source
):
    secret = Keypair.generate_mnemonic(24) if source == "mnemonic" else "5b" * 32
    original = (
        Keypair.create_from_mnemonic(secret, crypto_type)
        if source == "mnemonic"
        else Keypair.create_from_seed(bytes.fromhex(secret), crypto_type)
    )
    result = invoke(
        f"regen-{role}",
        "--crypto-type",
        scheme,
        "--type",
        mode,
        f"--{source}",
        secret,
        *(["--no-password"] if role == "coldkey" else []),
    )
    assert result.exit_code == 0, result.output
    restored = getattr(Wallet("schemes", path=str(wallet_path)), role)
    assert restored.crypto_type == crypto_type
    assert restored.ss58_address == original.ss58_address
    assert restored.hashed_descriptor == original.hashed_descriptor
    generation = 0 if mode == "standard" else 3
    signer = restored.at_generation(generation)
    payload = b"restored Falcon authority after three rotations"
    assert verify_mldsa_authorization(
        bytes(signer.sign_mldsa(payload)),
        payload,
        bytes(original.hashed_descriptor),
        generation,
        bytes(original.at_generation(generation).hashed_current_commitment),
    )
