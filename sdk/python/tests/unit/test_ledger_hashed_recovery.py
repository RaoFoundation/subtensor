"""Seed-restoration recovery through transport and the real native proof verifier.

The software device uses public test seeds; no hardware or network is accessed.
"""

from __future__ import annotations

from hashlib import blake2b
from types import SimpleNamespace
from unittest.mock import AsyncMock

import bittensor_core as backend
import pytest

from bittensor import LedgerHashedSigner
from bittensor import ledger as ledger_mod
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.interface import SubstrateConnection
from bittensor.hashed import descriptor_value

_ORIGINAL_SEED = b"public test seed: original Ledger recovery phrase"
_OTHER_SEED = b"public test seed: accidentally restored different phrase"
_GENESIS = "0x" + "11" * 32
_FINALIZED = "0x" + "22" * 32


class RestorableSoftwareDevice:
    """Emulate restoring another seed on the same connected Ledger device."""

    def __init__(self):
        self.seed = _ORIGINAL_SEED

    def key(self, account, index):
        seed = blake2b(
            self.seed + account.to_bytes(4, "little") + index.to_bytes(4, "little"),
            digest_size=32,
        ).digest()
        return backend.Keypair.from_seed(seed, crypto_type=0)

    def address(self, account, index, ss58_format, confirm):
        key = self.key(account, index)
        return key.public_key, backend.ss58_encode(key.public_key, ss58_format)

    def sign_message(self, message, account, index):
        return b"\x00" + self.key(account, index).sign(message)


class RecoveryCodec:
    """Capture transport proof bytes while preserving real signing and verification."""

    def __init__(self):
        self.proofs = []

    @staticmethod
    def constant(pallet, name):
        assert pallet == "HashedAccounts"
        return {"RegistrationDeposit": 200_000_000, "ClassicalSignerVersions": b"\x01\x81"}[name]

    @staticmethod
    def call_data(call):
        return bytes(call)

    @staticmethod
    def signature_payload_parts(call, *, nonce, **kwargs):
        return bytes(call), nonce.to_bytes(4, "little"), b"runtime-genesis-era"

    def encode_signed_extrinsic(
        self, call, *, public_key, signature, signature_version, nonce, **kwargs
    ):
        assert signature_version == 6
        self.proofs.append(signature)
        body = public_key + signature + nonce.to_bytes(4, "little") + bytes(call)
        return body, "0x" + blake2b(body, digest_size=32).hexdigest()


# A rejected wrong-seed attempt must not poison the successor and lock the original seed out.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("recreate_signer", [False, True], ids=["same-session", "restored-session"])
async def test_restored_ledger_seed_can_spend_after_failed_wrong_seed_attempt(
    monkeypatch, recreate_signer
):
    device = RestorableSoftwareDevice()
    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: lambda: device)
    signer = LedgerHashedSigner(account=3)
    codec = RecoveryCodec()
    record = {
        "descriptor": descriptor_value(signer.hashed_descriptor),
        "generation": 0,
        "commitment": bytes(signer.hashed_current_commitment),
    }
    nonce = 13
    conn = SubstrateConnection("ws://unused.invalid")
    conn._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    conn.get_chain_finalised_head = AsyncMock(return_value=_FINALIZED)
    conn.genesis_hash = AsyncMock(return_value=_GENESIS)
    conn._normalize_era = AsyncMock(return_value=("00", _GENESIS))
    conn._nonces = SimpleNamespace(next_for=AsyncMock(side_effect=lambda *args, **kwargs: nonce))

    async def query(pallet, item, params=None, *, block_hash):
        assert block_hash == _FINALIZED
        if (pallet, item) == ("AdminUtils", "HashedAccountsEnabled"):
            return True
        assert params == [signer.ss58_address]
        if (pallet, item) == ("HashedAccounts", "Accounts"):
            return record
        assert (pallet, item) == ("System", "Account")
        return {"nonce": nonce}

    conn.query = AsyncMock(side_effect=query)
    call = b"transfer test funds after restoring the original seed"
    device.seed = _OTHER_SEED
    with pytest.raises((ledger_mod.LedgerError, SubstrateRequestException)):
        await conn.create_signed_extrinsic(call, signer)
    assert codec.proofs == []

    # Retrying on the restored original seed must authorize safely. Verify the
    # actual proof before advancing authority exactly to the successor it commits.
    device.seed = _ORIGINAL_SEED
    signed = await conn.create_signed_extrinsic(call, signer)
    proof = codec.proofs[-1]
    payload = call + nonce.to_bytes(4, "little") + b"runtime-genesis-era"
    assert backend.verify_hashed_authorization(
        proof, payload, signer.hashed_descriptor, record["generation"], record["commitment"]
    )
    assert signed.signer_address == signer.ss58_address
    assert proof[0] == 0x81
    record["generation"] += 1
    record["commitment"] = proof[41:73]
    nonce += 1

    # A real accepted rotation must leave the original seed able to authorize
    # the next spend, both in the same session and after recreating the signer.
    recovered = LedgerHashedSigner(account=3) if recreate_signer else signer
    assert recovered.public_key == signer.public_key
    next_signed = await conn.create_signed_extrinsic(call, recovered)
    payload = call + nonce.to_bytes(4, "little") + b"runtime-genesis-era"
    assert backend.verify_hashed_authorization(
        codec.proofs[-1],
        payload,
        recovered.hashed_descriptor,
        record["generation"],
        record["commitment"],
    )
    assert next_signed.signer_address == signed.signer_address
    assert next_signed.nonce == nonce
