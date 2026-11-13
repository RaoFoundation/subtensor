"""Hashed Ledger signing with a deterministic software device; no hardware or RPC."""

from __future__ import annotations

import asyncio
from hashlib import blake2b

import bittensor_core as backend
import pytest

from bittensor import ledger as ledger_mod
from bittensor._transport.protocols import Keypair, MetadataVerifyingSigner


class SoftwareDevice:
    """Test-only Ed25519 keys stand in for the device's hardened path keys."""

    def __init__(self):
        self.address_calls = []
        self.message_calls = []
        self.response_mode = "wrapped"

    @staticmethod
    def key(account, index):
        seed = blake2b(
            b"ledger-test-only" + account.to_bytes(4, "little") + index.to_bytes(4, "little"),
            digest_size=32,
        ).digest()
        return backend.Keypair.from_seed(seed, crypto_type=0)

    def address(self, account, index, ss58_prefix, confirm):
        self.address_calls.append((account, index, ss58_prefix, confirm))
        key = self.key(account, index)
        return key.public_key, backend.ss58_encode(key.public_key, ss58_prefix)

    def sign_message(self, message, account, index):
        self.message_calls.append((bytes(message), account, index))
        if self.response_mode == "reject":
            raise ledger_mod.LedgerError("the request was rejected on the device")
        key = self.key(account, index + (self.response_mode == "wrong_key"))
        signed_message = message[7:-8] if self.response_mode == "raw" else message
        signature = b"\x00" + key.sign(signed_message)
        if self.response_mode == "wrong_scheme":
            return b"\x01" + signature[1:]
        if self.response_mode == "truncated":
            return signature[:-1]
        return signature


@pytest.fixture()
def signer(monkeypatch):
    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: SoftwareDevice)
    return ledger_mod.LedgerHashedSigner(account=3)


def test_hashed_ledger_keeps_permanent_identity_and_distinct_clear_signing_mode(signer):
    initial_key = SoftwareDevice.key(3, 0)
    initial_commitment = blake2b(
        b"bittensor/hashed/v1/key\x01\x03" + initial_key.public_key, digest_size=32
    ).digest()
    descriptor = b"\x01\x03" + initial_commitment
    public = backend.Keypair.from_hashed_descriptor(descriptor)
    assert isinstance(signer, Keypair)
    assert not isinstance(signer, MetadataVerifyingSigner)
    assert signer.crypto_type == 6
    assert signer.hashed_signature_version == 0x81
    assert signer.hashed_descriptor == descriptor
    assert signer.public_key == public.public_key
    assert signer.ss58_address == public.ss58_address
    assert signer.ss58_address != initial_key.ss58_address
    assert signer.hashed_receiving_address() == public.hashed_receiving_address()
    assert signer.public_only().hashed_descriptor == descriptor

    selected = signer.at_generation(7)
    assert signer.hashed_generation == 0
    assert selected.hashed_generation == 7
    assert selected.hashed_descriptor == descriptor
    assert selected.public_key == signer.public_key
    assert selected.ss58_address == signer.ss58_address
    assert selected.hashed_signing_public_key == SoftwareDevice.key(3, 7).public_key
    assert selected.hashed_next_commitment == signer.hashed_commitment(8)
    assert selected.hashed_current_commitment != initial_commitment
    assert selected._device is signer._device
    with pytest.raises(ledger_mod.LedgerError, match="synchronized hashed"):
        selected.sign(b"payload")


def test_hashed_ledger_signs_exact_wrapped_domain_hash_and_core_accepts_proof(signer):
    selected = signer.at_generation(4)
    # The full Substrate payload must be bound even when larger than 256 bytes.
    payload = b"call-and-signed-extensions" * 20 + b"genesis-and-era"
    proof = asyncio.run(selected.sign_hashed(payload))
    current = SoftwareDevice.key(3, 4).public_key
    next_commitment = selected.hashed_next_commitment
    address = (
        selected.public_key + b"\x81\x03" + (4).to_bytes(8, "little") + current + next_commitment
    )
    digest = blake2b(
        b"bittensor/hashed/v1/transaction" + address + payload, digest_size=32
    ).digest()
    message = b"<Bytes>" + digest + b"</Bytes>"
    assert signer._device.message_calls == [(message, 3, 4)]
    assert len(message) == 47
    assert len(proof) == 137
    assert proof[:73] == b"\x81" + (4).to_bytes(8, "little") + current + next_commitment
    assert backend.verify_strict(message, proof[73:], backend.ss58_encode(current), 0)
    assert backend.verify_hashed_authorization(
        proof, payload, selected.hashed_descriptor, 4, selected.hashed_current_commitment
    )
    assert not backend.verify_hashed_authorization(
        proof,
        payload + b"changed",
        selected.hashed_descriptor,
        4,
        selected.hashed_current_commitment,
    )
    assert not backend.verify_hashed_authorization(
        proof[1:], payload, selected.hashed_descriptor, 4, selected.hashed_current_commitment
    )


@pytest.mark.parametrize("response_mode", ["raw", "wrong_key", "wrong_scheme", "truncated"])
def test_hashed_ledger_rejects_device_signature_for_wrong_bytes_or_key(signer, response_mode):
    signer._device.response_mode = response_mode
    with pytest.raises(ledger_mod.LedgerError, match=r"signature|MultiSignature"):
        asyncio.run(signer.sign_hashed(b"complete payload"))


def test_hashed_ledger_propagates_device_refusal_and_missing_capability(signer, monkeypatch):
    signer._device.response_mode = "reject"
    with pytest.raises(ledger_mod.LedgerError, match="rejected"):
        asyncio.run(signer.sign_hashed(b"complete payload"))
    monkeypatch.setattr(signer._device, "sign_message", None)
    with pytest.raises(ledger_mod.LedgerError, match="no Ledger message-signing support"):
        asyncio.run(signer.sign_hashed(b"complete payload"))
    assert len(signer._device.message_calls) == 1


@pytest.mark.parametrize("account", [-1, 2**31, True, 1.5])
def test_hashed_ledger_rejects_path_aliases_before_device_access(monkeypatch, account):
    def unexpected_device():
        pytest.fail("invalid path must fail before device discovery")

    monkeypatch.setattr(ledger_mod, "_device_cls", unexpected_device)
    with pytest.raises(ValueError, match="Ledger"):
        ledger_mod.LedgerHashedSigner(account=account)


def test_hashed_ledger_reserves_the_full_account_instead_of_accepting_a_base_index(monkeypatch):
    def unexpected_device():
        pytest.fail("custom starting index must fail before device discovery")

    monkeypatch.setattr(ledger_mod, "_device_cls", unexpected_device)
    with pytest.raises(TypeError, match="index"):
        ledger_mod.LedgerHashedSigner(account=3, index=2**31 - 2)


@pytest.mark.parametrize("generation", [-1, True, 1.5, 2**31, 2**31 - 1])
def test_hashed_ledger_rejects_generation_and_successor_overflow(signer, generation):
    with pytest.raises(ValueError, match="Ledger"):
        signer.at_generation(generation)
    assert len(signer._device.address_calls) == 1
    assert signer._device.message_calls == []


def test_hashed_ledger_accepts_last_generation_with_a_valid_successor(signer):
    selected = signer.at_generation(2**31 - 2)
    assert selected.hashed_generation == 2**31 - 2
    assert selected._path_index(selected.hashed_generation + 1) == 2**31 - 1


def test_hashed_ledger_rejects_malformed_device_public_key(monkeypatch):
    class MalformedDevice(SoftwareDevice):
        def address(self, *args):
            return bytes(31), "unused"

    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: MalformedDevice)
    with pytest.raises(ledger_mod.LedgerError, match="invalid Ed25519 public key"):
        ledger_mod.LedgerHashedSigner(account=3)
