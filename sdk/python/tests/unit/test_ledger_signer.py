"""LedgerSigner glue, exercised offline against golden metadata.

A fake device stands in for the HID transport; the digest and proof math run
through the real ``bittensor_core`` binding, so these tests pin the exact
bytes a device would receive. The digest value itself is cross-checked
against polkadot-js ``merkleizeMetadata`` in the Rust core's test suite.
"""

from __future__ import annotations

import asyncio

import pytest

from bittensor import ledger as ledger_mod
from bittensor._transport import extrinsics
from bittensor._transport.contract import SigningContext
from bittensor._transport.protocols import (
    Keypair,
    MetadataVerifyingSigner,
    UnsignedExtrinsicSigner,
)
from tests.conftest import GOLDEN_FIXTURE, golden, golden_codec

pytest.importorskip("bittensor_core")

pytestmark = pytest.mark.skipif(not GOLDEN_FIXTURE.exists(), reason="golden fixture not recorded")

# The digest for the golden metadata with node-subtensor/439/42/9/TAO — the
# same vector pinned against polkadot-js in bittensor-core/src/digest.
EXPECTED_DIGEST = "b5c88dea6d1920f1b4ced91e632b1b1db2db5c79a01e51ea383848b61f37d8a8"


class FakeDevice:
    """Stands in for bittensor_core.LedgerDevice; records what it signs."""

    def __init__(self):
        self.sign_calls: list[tuple[bytes, bytes, int, int]] = []

    def address(self, account, index, ss58_prefix, confirm):
        return (b"\x01" * 32, "5FakeLedgerAddress")

    def app_version(self):
        return (100, 0, 5)

    def sign(self, payload, proof, account, index):
        self.sign_calls.append((bytes(payload), bytes(proof), account, index))
        return b"\x00" + b"\x11" * 64  # MultiSignature: ed25519 prefix + sig


@pytest.fixture()
def signer(monkeypatch):
    monkeypatch.setattr(ledger_mod, "_device_cls", lambda: FakeDevice)
    return ledger_mod.LedgerSigner()


def _context() -> SigningContext:
    codec = golden_codec()
    return SigningContext(
        metadata_bytes=codec.metadata_bytes,
        spec_version=codec.spec_version,
        spec_name=codec.spec_name,
        transaction_version=codec.transaction_version,
        ss58_format=codec.ss58_format,
        genesis_hash=golden()["network"]["genesis_hash"],
    )


def test_satisfies_the_signing_protocols(signer):
    assert isinstance(signer, Keypair)
    assert isinstance(signer, MetadataVerifyingSigner)
    assert isinstance(signer, UnsignedExtrinsicSigner)
    assert signer.crypto_type == 0  # ed25519
    assert signer.ss58_address == "5FakeLedgerAddress"
    assert signer.public_key == b"\x01" * 32


def test_metadata_digest_matches_pinned_vector(signer):
    digest = signer.metadata_digest(_context())
    assert digest.hex() == EXPECTED_DIGEST


def test_raw_sign_is_refused(signer):
    with pytest.raises(ledger_mod.LedgerError, match="blind-sign"):
        signer.sign(b"\x01\x02")


def test_sign_unsigned_extrinsic_ships_payload_and_proof(signer):
    codec = golden_codec()
    g = golden()
    digest = signer.metadata_digest(_context())
    call = codec.compose_call(
        "Balances", "transfer_keep_alive", {"dest": g["ss58"][1]["address"], "value": 12345}
    )
    unsigned = extrinsics.prepare_extrinsic(
        codec,
        call,
        address=signer.ss58_address,
        public_key=signer.public_key,
        crypto_type=signer.crypto_type,
        era="00",
        nonce=0,
        tip=0,
        tip_asset_id=None,
        genesis_hash=g["network"]["genesis_hash"],
        era_block_hash=g["network"]["genesis_hash"],
        metadata_hash=digest,
    )
    signature = asyncio.run(signer.sign_unsigned_extrinsic(unsigned))
    assert signature == b"\x00" + b"\x11" * 64

    (payload, proof, account, index) = signer._device.sign_calls[0]
    # The device receives the exact unhashed payload (the parts, reassembled)
    # followed by the RFC-0078 proof for exactly this extrinsic's types.
    assert payload == (
        unsigned.call_data + unsigned.included_in_extrinsic + unsigned.included_in_signed_data
    )
    assert (account, index) == (0, 0)
    # The proof ends with the SCALE-encoded ExtraInfo tail: spec_version LE,
    # then spec_name / prefix / decimals / symbol.
    assert proof.endswith(
        (439).to_bytes(4, "little")
        + bytes([0x38])  # compact len("node-subtensor")
        + b"node-subtensor"
        + (42).to_bytes(2, "little")
        + bytes([9])
        + bytes([0x0C])  # compact len("TAO")
        + b"TAO"
    )


def test_sign_without_context_is_refused(signer):
    codec = golden_codec()
    g = golden()
    call = codec.compose_call(
        "Balances", "transfer_keep_alive", {"dest": g["ss58"][1]["address"], "value": 1}
    )
    unsigned = extrinsics.prepare_extrinsic(
        codec,
        call,
        address=signer.ss58_address,
        public_key=signer.public_key,
        crypto_type=signer.crypto_type,
        era="00",
        nonce=0,
        tip=0,
        tip_asset_id=None,
        genesis_hash=g["network"]["genesis_hash"],
        era_block_hash=g["network"]["genesis_hash"],
    )
    with pytest.raises(ledger_mod.LedgerError, match="signing context"):
        asyncio.run(signer.sign_unsigned_extrinsic(unsigned))
