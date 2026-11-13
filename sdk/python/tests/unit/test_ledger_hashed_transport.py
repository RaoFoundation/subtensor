"""Wrapped hardware authorization through the real SDK signing lifecycle.

RPC and codec framing are isolated here; the native proof verifier is real.
The core and runtime suites separately test the exact SCALE envelope.
"""

from __future__ import annotations

import threading
from hashlib import blake2b
from types import SimpleNamespace
from unittest.mock import AsyncMock

import bittensor_core as backend
import pytest

from bittensor import LedgerHashedSigner
from bittensor import ledger as ledger_mod
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.extrinsics import create_hashed_extrinsic
from bittensor._transport.interface import SubstrateConnection
from bittensor.hashed import descriptor_value
from bittensor.signing import public_view, resolve_signer
from tests.unit.test_ledger_hashed_signer import SoftwareDevice

GENESIS = "0x" + "11" * 32
FINALIZED = "0x" + "22" * 32


class Codec:
    """Capture assembled proof bytes without replacing cryptographic checks."""

    enabled = True
    versions = b"\x01\x81"

    def constant(self, pallet, name):
        assert pallet == "HashedAccounts"
        return {"RegistrationDeposit": 200_000_000, "ClassicalSignerVersions": self.versions}[name]

    @staticmethod
    def call_data(call):
        return bytes(call)

    @staticmethod
    def signature_payload_parts(call, *, nonce, **kwargs):
        return bytes(call), nonce.to_bytes(4, "little"), b"runtime-genesis-era"

    def encode_signed_extrinsic(
        self, call, *, public_key, signature, signature_version, nonce, **kw
    ):
        assert signature_version == 6
        self.proof = signature
        self.nonce = nonce
        body = public_key + signature + nonce.to_bytes(4, "little") + call
        return body, "0x" + blake2b(body, digest_size=32).hexdigest()


@pytest.fixture()
def setup(monkeypatch):
    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: SoftwareDevice)
    signer = LedgerHashedSigner(account=3)
    codec = Codec()
    conn = SubstrateConnection("ws://unused.invalid")
    conn._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    conn.get_chain_head = AsyncMock(side_effect=AssertionError("must use finalized authority"))
    conn.get_chain_finalised_head = AsyncMock(return_value=FINALIZED)
    conn.genesis_hash = AsyncMock(return_value=GENESIS)
    conn._nonces = SimpleNamespace(next_for=AsyncMock(return_value=13))
    conn._normalize_era = AsyncMock(return_value=("00", GENESIS))
    generation = 7
    record = {
        "descriptor": descriptor_value(signer.hashed_descriptor),
        "generation": generation,
        "commitment": blake2b(
            b"bittensor/hashed/v1/key\x01\x03" + SoftwareDevice.key(3, generation).public_key,
            digest_size=32,
        ).digest(),
    }

    async def query(pallet, item, params=None, *, block_hash):
        assert block_hash == FINALIZED
        if (pallet, item) == ("AdminUtils", "HashedAccountsEnabled"):
            return codec.enabled
        assert params == [signer.ss58_address]
        return record if pallet == "HashedAccounts" else {"nonce": 13}

    conn.query = AsyncMock(side_effect=query)
    return signer, conn, codec, record


async def test_wrapped_ledger_uses_finalized_generation_full_payload_and_async_device(
    setup, monkeypatch
):
    signer, conn, codec, record = setup
    # An uncached generation requires HID address derivation. It must not run
    # on the event-loop thread, including the pre-sign commitment check.
    loop_thread = threading.get_ident()
    original_address = signer._device.address

    def address(*args):
        assert threading.get_ident() != loop_thread
        return original_address(*args)

    monkeypatch.setattr(signer._device, "address", address)
    assert public_view(signer) is signer
    assert resolve_signer(signer) is signer
    call = b"large-call" * 50
    signed = await conn.create_signed_extrinsic(call, signer)
    proof = codec.proof
    payload = call + (13).to_bytes(4, "little") + b"runtime-genesis-era"
    assert proof[0] == 0x81
    assert len(proof) == 137
    assert int.from_bytes(proof[1:9], "little") == 7
    assert backend.verify_hashed_authorization(
        proof, payload, signer.hashed_descriptor, 7, record["commitment"]
    )
    assert signed.signer_address == signer.ss58_address
    assert signed.nonce == 13
    assert signer.hashed_generation == 0
    assert signer._device.message_calls[0][1:] == (3, 7)
    assert len(signer._device.message_calls) == 1
    conn.get_chain_finalised_head.assert_awaited_once()
    conn._nonces.next_for.assert_awaited_once_with(signer.ss58_address, use_cache=False)


@pytest.mark.parametrize("versions", [None, b"\x01", "0x01", [1], "invalid", [1, 256]])
async def test_unsupported_runtime_rejects_before_device_or_authority_reads(setup, versions):
    signer, conn, codec, _ = setup
    codec.versions = versions
    with pytest.raises(SubstrateRequestException, match="signature mode"):
        await conn.create_signed_extrinsic(b"call", signer)
    assert len(signer._device.address_calls) == 1  # constructor only
    assert signer._device.message_calls == []
    conn.query.assert_not_awaited()


@pytest.mark.parametrize("versions", [b"\x01\x81", "0x0181", [1, 129], (1, 129)])
async def test_runtime_capability_decodings_accept_explicit_wrapped_mode(setup, versions):
    signer, conn, codec, _ = setup
    codec.versions = versions
    await conn.create_signed_extrinsic(b"call", signer)
    assert len(signer._device.message_calls) == 1


@pytest.mark.parametrize("failure", ["disabled", "commitment", "descriptor", "pending", "nonce"])
async def test_unsynchronized_authority_never_requests_a_signature(setup, failure):
    signer, conn, codec, record = setup
    if failure == "disabled":
        codec.enabled = False
    elif failure == "commitment":
        record["commitment"] = bytes(32)
    elif failure == "descriptor":
        record["descriptor"]["initial_commitment"] = "0x" + "00" * 32
    elif failure == "pending":
        conn._nonces.next_for.return_value = 14
    with pytest.raises(SubstrateRequestException, match="hashed"):
        await conn.create_signed_extrinsic(
            b"call", signer, nonce=12 if failure == "nonce" else None
        )
    assert signer._device.message_calls == []
    assert not hasattr(codec, "proof")


async def test_fee_quote_uses_wrapped_envelope_without_device_access(setup):
    signer, conn, codec, _ = setup
    await conn.sign_without_nonce_tracking(b"call", signer, nonce=13, signature=bytes(64))
    assert len(codec.proof) == 137
    assert codec.proof[0] == 0x81
    assert int.from_bytes(codec.proof[1:9], "little") == 7
    assert signer._device.message_calls == []
    assert len(signer._device.address_calls) == 1


async def test_shield_pair_uses_consecutive_keys_from_one_pinned_authority(setup):
    signer, conn, codec, record = setup
    snapshot = await conn.prepare_hashed_snapshot(signer)
    for offset, call in [(1, b"inner call"), (0, b"carrier call")]:
        await conn.create_signed_extrinsic(
            call,
            signer,
            nonce=13 + offset,
            hashed_generation_offset=offset,
            hashed_signing_snapshot=snapshot,
        )
        payload = call + (13 + offset).to_bytes(4, "little") + b"runtime-genesis-era"
        commitment = signer.hashed_commitment(7 + offset)
        assert backend.verify_hashed_authorization(
            codec.proof, payload, signer.hashed_descriptor, 7 + offset, commitment
        )
    assert [index for _, _, index in signer._device.message_calls] == [8, 7]
    assert record["generation"] == 7
    assert signer.hashed_generation == 0


def test_sync_assembly_requires_async_entry_point_for_hardware_signing(setup):
    signer, _, codec, _ = setup
    with pytest.raises(TypeError, match="asynchronous hashed signers"):
        create_hashed_extrinsic(
            codec,
            b"call",
            signer,
            era="00",
            nonce=13,
            tip=0,
            tip_asset_id=None,
            genesis_hash=GENESIS,
            era_block_hash=GENESIS,
        )
    assert signer._device.message_calls == []
