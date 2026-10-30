"""Versioned offline/device requests retain native bytes and current authority."""

import json
from dataclasses import replace
from unittest.mock import Mock

import pytest

from bittensor._transport.contract import HashedSigningSnapshot
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.extrinsics import hashed_proof_length
from bittensor.mldsa_signing import (
    FORMAT,
    PQ_FORMAT,
    PreparedMldsaExtrinsic,
    attach_mldsa_proof,
    prepare_mldsa_extrinsic,
    sign_prepared_mldsa,
)
from bittensor.sp_core import CRYPTO_MLDSA65, FALCON_CRYPTO_TYPES, FIXED_PQ_CRYPTO_TYPES, Keypair
from tests.unit.test_mldsa_accounts import GENESIS, Codec, connection, key


class OfflineCodec(Codec):
    spec_version = 417
    transaction_version = 2

    @staticmethod
    def signature_payload_parts(call, **kwargs):
        # Observe all extension inputs; native Rust tests prove SCALE framing.
        extra = json.dumps(kwargs, sort_keys=True, default=lambda b: bytes(b).hex()).encode()
        return bytes(call), extra, b"implicit-runtime-values"


@pytest.fixture(params=[7, 8, 9, 10, 11, 12])
def signing_wallet(request):
    return key(request.param)


def prepared(wallet=None):
    wallet = wallet or key()
    generation = 0 if wallet.crypto_type in FIXED_PQ_CRYPTO_TYPES else 7
    authority = HashedSigningSnapshot(
        "0x" + "22" * 32,
        wallet.ss58_address,
        wallet.crypto_type,
        bytes(wallet.hashed_descriptor),
        generation,
        bytes(wallet.at_generation(generation).hashed_current_commitment),
        13,
    )
    codec = OfflineCodec()
    request = prepare_mldsa_extrinsic(
        codec,
        b"canonical call" * 50,
        authority,
        era="00",
        genesis_hash=GENESIS,
        era_block_hash=GENESIS,
        tip=11,
        metadata_hash=bytes([9]) * 32,
    )
    return wallet, codec, request


def test_offline_json_roundtrip_and_full_payload_proof_attachment(signing_wallet):
    wallet, codec, request = prepared(signing_wallet)
    exported = json.loads(json.dumps(request.to_dict()))
    assert exported["format"] == (
        PQ_FORMAT if wallet.crypto_type in FALCON_CRYPTO_TYPES else FORMAT
    )
    restored = PreparedMldsaExtrinsic.from_dict(exported)
    assert restored == request
    swapped = dict(exported, format=FORMAT if exported["format"] == PQ_FORMAT else PQ_FORMAT)
    with pytest.raises(ValueError, match="cryptographic profile"):
        PreparedMldsaExtrinsic.from_dict(swapped)
    assert len(restored.payload) > 256
    assert restored.payload.startswith(b"canonical call" * 50)
    proof = sign_prepared_mldsa(restored, wallet, codec=codec, expected_genesis_hash=GENESIS)
    assert len(proof) == hashed_proof_length(wallet.crypto_type)
    signed = attach_mldsa_proof(codec, restored, proof)
    assert signed.data.endswith(proof)
    assert codec.account == bytes(wallet.public_key)
    assert codec.nonce == 13
    assert codec.call == restored.unsigned.call_data


@pytest.mark.parametrize("field", ["call", "tip", "metadata", "proof", "runtime"])
def test_attachment_rejects_changed_transaction_or_runtime(field, signing_wallet):
    wallet, codec, request = prepared(signing_wallet)
    proof = sign_prepared_mldsa(request, wallet, codec=codec, expected_genesis_hash=GENESIS)
    if field == "call":
        call = b"different call"
        unsigned = replace(request.unsigned, call_data=call)
        unsigned.payload = call + unsigned.included_in_extrinsic + unsigned.included_in_signed_data
        request = replace(request, unsigned=unsigned)
    elif field == "tip":
        request = replace(request, unsigned=replace(request.unsigned, tip=12))
    elif field == "metadata":
        request = replace(request, unsigned=replace(request.unsigned, metadata_hash=None))
    elif field == "proof":
        proof = proof[:-1] + bytes([proof[-1] ^ 1])
    else:
        codec.spec_version += 1
    with pytest.raises(ValueError, match=r"invalid|changed"):
        attach_mldsa_proof(codec, request, proof)


def test_offline_signer_rejects_wrong_wallet_or_authority(signing_wallet):
    wallet, codec, request = prepared(signing_wallet)
    other = Keypair.create_from_seed(bytes([8]) * 32, CRYPTO_MLDSA65)
    with pytest.raises(ValueError, match="another wallet"):
        sign_prepared_mldsa(request, other, codec=codec, expected_genesis_hash=GENESIS)
    with pytest.raises(ValueError, match=r"commitment|invalid versioned"):
        sign_prepared_mldsa(
            replace(request, authority=replace(request.authority, commitment=bytes([1]) * 64)),
            wallet,
            codec=codec,
            expected_genesis_hash=GENESIS,
        )
    exported = request.to_dict()
    exported["format"] = "bittensor/ms/v2/extrinsic/0"
    with pytest.raises(ValueError, match="format"):
        PreparedMldsaExtrinsic.from_dict(exported)


@pytest.mark.parametrize(
    "field,value",
    [
        ("nonce", 14),
        ("tip", 12),
        ("tip_asset_id", 2),
        ("era", {"period": 64, "current": 10}),
        ("genesis_hash", "0x" + "33" * 32),
        ("era_block_hash", "0x" + "44" * 32),
        ("spec_version", 418),
        ("transaction_version", 3),
        ("metadata_hash", None),
        ("included_in_signed_data", b"hidden other chain"),
        ("included_in_extrinsic", b"hidden different fee"),
    ],
)
def test_offline_signing_reconstructs_reviewed_fields_before_private_key_use(
    field, value, signing_wallet
):
    _wallet, codec, request = prepared(signing_wallet)
    unsigned = replace(request.unsigned, **{field: value})
    # Make the superficial concatenation check pass even for hidden bytes.
    unsigned.payload = (
        unsigned.call_data + unsigned.included_in_extrinsic + unsigned.included_in_signed_data
    )
    request = replace(request, unsigned=unsigned)
    private_key = Mock()
    with pytest.raises(ValueError):
        sign_prepared_mldsa(request, private_key, codec=codec, expected_genesis_hash=GENESIS)
    private_key.at_generation.assert_not_called()


async def test_connected_preparation_uses_public_descriptor_and_revalidates_finalized_state(
    signing_wallet,
):
    wallet = signing_wallet
    conn, _codec, state = connection(wallet)
    codec = OfflineCodec()
    conn._runtimes.codec_at.return_value = codec
    request = await conn.prepare_mldsa_unsigned(
        b"full call" * 64, descriptor=wallet.hashed_descriptor, tip=11
    )
    assert (
        request.authority.generation
        == state["generation"]
        == (0 if wallet.crypto_type in FIXED_PQ_CRYPTO_TYPES else 7)
    )
    assert request.authority.block_hash == "0xfinalized"
    proof = sign_prepared_mldsa(request, wallet, codec=codec, expected_genesis_hash=GENESIS)
    assert (await conn.attach_signature(request, proof)).data.endswith(proof)
    if wallet.crypto_type in FIXED_PQ_CRYPTO_TYPES:
        state["generation"] = 99  # Registry contents cannot change fixed-key authority.
        assert (await conn.attach_signature(request, proof)).data.endswith(proof)
        return
    state["generation"] = 8
    state["commitment"] = bytes(wallet.at_generation(8).hashed_current_commitment)
    with pytest.raises(SubstrateRequestException, match="authority or chain changed"):
        await conn.attach_signature(request, proof)


async def test_connected_attachment_rejects_pending_nonce_and_missing_finalized_head(
    signing_wallet,
):
    wallet = signing_wallet
    conn, _codec, _state = connection(wallet)
    codec = OfflineCodec()
    conn._runtimes.codec_at.return_value = codec
    request = await conn.prepare_mldsa_unsigned(b"call", descriptor=wallet.hashed_descriptor)
    proof = sign_prepared_mldsa(request, wallet, codec=codec, expected_genesis_hash=GENESIS)
    conn._nonces.next_for.return_value = 14
    if wallet.crypto_type in FIXED_PQ_CRYPTO_TYPES:
        assert (await conn.attach_signature(request, proof)).data.endswith(proof)
    else:
        with pytest.raises(SubstrateRequestException, match="pending transaction"):
            await conn.attach_signature(request, proof)
    conn.get_chain_finalised_head.return_value = None
    with pytest.raises(SubstrateRequestException, match="finalized"):
        await conn.prepare_mldsa_unsigned(b"call", descriptor=wallet.hashed_descriptor)


def test_attachment_rejects_correctly_signed_zero_next_commitment():
    from pathlib import Path

    # Native Rust regenerates this deterministic fixture and checks its ML-DSA
    # signature independently of authorization policy. It models a third-party
    # signer producing a valid signature over an unusable rotation commitment.
    proof = bytes.fromhex(
        (Path(__file__).parents[1] / "fixtures/mldsa-zero-next-proof.hex").read_text()
    )
    wallet, _codec, initial = prepared()

    class ExternalCodec(OfflineCodec):
        @staticmethod
        def signature_payload_parts(call, **_kwargs):
            return bytes(call), b"", b""

    codec = ExternalCodec()
    request = prepare_mldsa_extrinsic(
        codec,
        bytes([19]) * 500,
        initial.authority,
        era="00",
        genesis_hash=GENESIS,
        era_block_hash=GENESIS,
    )
    valid = wallet.at_generation(7).sign_mldsa(request.payload)
    assert attach_mldsa_proof(codec, request, valid).data.endswith(valid)
    codec.encode_signed_extrinsic = Mock(side_effect=AssertionError("must reject before assembly"))
    with pytest.raises(ValueError, match="invalid post-quantum transaction proof"):
        attach_mldsa_proof(codec, request, proof)
    codec.encode_signed_extrinsic.assert_not_called()


def test_offline_fixed_signer_requires_runtime_profile_support_before_private_key_use():
    wallet, codec, request = prepared(key(8))
    codec.constant = lambda *_: None
    private = Mock(wraps=wallet)
    with pytest.raises(ValueError, match="account profile"):
        sign_prepared_mldsa(request, private, codec=codec, expected_genesis_hash=GENESIS)
    private.at_generation.assert_not_called()
