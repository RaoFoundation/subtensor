"""Ledger signing sessions use fresh keys and survive cancellation without overlap.

Only deterministic public test seeds are used; no device or network is accessed.
"""

from __future__ import annotations

import asyncio
from hashlib import blake2b
from threading import Event, Lock

import bittensor_core as backend
import pytest

from bittensor import ledger as ledger_mod

_ORIGINAL_SEED = b"public test seed: Ledger signing session A"
_OTHER_SEED = b"public test seed: Ledger signing session B"
_ACCOUNT = 3


def _key(seed, index):
    material = seed + _ACCOUNT.to_bytes(4, "little") + index.to_bytes(4, "little")
    return backend.Keypair.from_seed(blake2b(material, digest_size=32).digest(), crypto_type=0)


def _commitment(seed, index):
    return blake2b(
        b"bittensor/hashed/v1/key\x01\x03" + _key(seed, index).public_key, digest_size=32
    ).digest()


class SessionSoftwareDevice:
    def __init__(self):
        self.other_successor_once = None
        self.message_calls = []

    def address(self, account, index, ss58_format, confirm):
        assert account == _ACCOUNT
        seed = _ORIGINAL_SEED
        if index == self.other_successor_once:
            seed = _OTHER_SEED
            self.other_successor_once = None
        key = _key(seed, index)
        return key.public_key, backend.ss58_encode(key.public_key, ss58_format)

    def sign_message(self, message, account, index):
        assert account == _ACCOUNT
        signature = b"\x00" + _key(_ORIGINAL_SEED, index).sign(message)
        self.message_calls.append((bytes(message), index, signature))
        return signature


# A valid current-key signature must not rotate funds to a transient wrong-wallet successor.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("generation", [0, 7])
async def test_transient_wrong_wallet_successor_is_rejected_without_poisoning_recovery(
    monkeypatch, generation
):
    device = SessionSoftwareDevice()
    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: lambda: device)
    signer = ledger_mod.LedgerHashedSigner(account=_ACCOUNT)
    selected = signer.at_generation(generation)
    payload = b"spend test funds without losing the next signing authority"
    current_commitment = _commitment(_ORIGINAL_SEED, generation)

    # The initial/current key and actual signature come from A; exactly one
    # successor response comes from B. The current-key signature is valid, so
    # re-reading the successor is necessary to prevent accepting this rotation.
    device.other_successor_once = generation + 1
    with pytest.raises(ledger_mod.LedgerError, match="keys changed during signing"):
        await selected.sign_hashed(payload)
    assert len(device.message_calls) == 1
    mixed_proof = (
        b"\x81"
        + generation.to_bytes(8, "little")
        + _key(_ORIGINAL_SEED, generation).public_key
        + _commitment(_OTHER_SEED, generation + 1)
        + device.message_calls[0][2][1:]
    )
    assert backend.verify_hashed_authorization(
        mixed_proof, payload, signer.hashed_descriptor, generation, current_commitment
    )

    # A retry must commit to A's successor, and restoring A into a new signer
    # must still authorize a subsequent spend under that accepted commitment.
    proof = await selected.sign_hashed(payload)
    assert backend.verify_hashed_authorization(
        proof, payload, signer.hashed_descriptor, generation, current_commitment
    )
    assert proof[41:73] == _commitment(_ORIGINAL_SEED, generation + 1)
    recovered = ledger_mod.LedgerHashedSigner(account=_ACCOUNT).at_generation(generation + 1)
    assert recovered.public_key == signer.public_key
    next_payload = b"spend remaining test funds after recovery"
    next_proof = await recovered.sign_hashed(next_payload)
    assert backend.verify_hashed_authorization(
        next_proof, next_payload, recovered.hashed_descriptor, generation + 1, proof[41:73]
    )


class BlockingSessionDevice(SessionSoftwareDevice):
    def __init__(self):
        super().__init__()
        self.signing_started = Event()
        self.release_signing = Event()
        self.signing_in_flight = Event()
        self.overlap = Event()

    def address(self, account, index, ss58_format, confirm):
        if self.signing_in_flight.is_set():
            self.overlap.set()
        return super().address(account, index, ss58_format, confirm)

    def sign_message(self, message, account, index):
        if self.signing_in_flight.is_set():
            self.overlap.set()
        if not self.signing_started.is_set():
            self.signing_in_flight.set()
            self.signing_started.set()
            try:
                if not self.release_signing.wait(10):
                    raise RuntimeError("test did not release the blocked signing operation")
                return super().sign_message(message, account, index)
            finally:
                self.signing_in_flight.clear()
        return super().sign_message(message, account, index)


class ObservedLock:
    """Expose the second acquisition attempt without releasing the real lock."""

    def __init__(self, lock):
        self.lock = lock
        self.counter_lock = Lock()
        self.entries = 0
        self.second_attempted = Event()

    def __enter__(self):
        with self.counter_lock:
            self.entries += 1
            if self.entries == 2:
                self.second_attempted.set()
        self.lock.acquire()
        return self

    def __exit__(self, *_):
        self.lock.release()


async def test_cancelled_signing_worker_keeps_generation_views_serialized(monkeypatch):
    device = BlockingSessionDevice()
    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: lambda: device)
    signer = ledger_mod.LedgerHashedSigner(account=_ACCOUNT)
    observed_lock = ObservedLock(signer._device_lock)
    signer._device_lock = observed_lock
    first = signer.at_generation(0)
    second = signer.at_generation(1)
    assert first._device_lock is second._device_lock
    cancelled = asyncio.create_task(first.sign_hashed(b"cancelled caller payload"))
    retry = None
    try:
        assert await asyncio.to_thread(device.signing_started.wait, 2)
        cancelled.cancel()
        with pytest.raises(asyncio.CancelledError):
            await cancelled

        retry_payload = b"sign safely from another generation view"
        retry = asyncio.create_task(second.sign_hashed(retry_payload))
        assert await asyncio.to_thread(observed_lock.second_attempted.wait, 2)
        # Cancellation ended the caller but the first worker still owns the
        # shared lock. The second worker has reached its acquisition attempt.
        assert observed_lock.lock.locked()
        assert device.signing_in_flight.is_set()
        assert not retry.done()
        assert not device.overlap.is_set()
        device.release_signing.set()
        proof = await asyncio.wait_for(retry, 5)
        assert not device.overlap.is_set()
        assert [index for _, index, _ in device.message_calls] == [0, 1]
        assert backend.verify_hashed_authorization(
            proof,
            retry_payload,
            signer.hashed_descriptor,
            1,
            _commitment(_ORIGINAL_SEED, 1),
        )
    finally:
        device.release_signing.set()
        if not cancelled.done():
            cancelled.cancel()
        if retry is not None and not retry.done():
            await asyncio.wait_for(retry, 5)
