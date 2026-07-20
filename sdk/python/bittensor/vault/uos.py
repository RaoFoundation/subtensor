"""UOS (Universal Offline Signatures) framing for Polkadot Vault QR codes.

The wire format Polkadot Vault (ex Parity Signer) scans, matching the
reference hot-wallet implementation (polkadot-js ``@polkadot/react-qr``):

    0x53 | crypto | cmd | 32-byte public key | payload | 32-byte genesis hash

``crypto`` shares Substrate's numbering (0x00 ed25519, 0x01 sr25519). For a
transaction, ``payload`` is the Polkadot-JS ``ExtrinsicPayload.toU8a()``: the
signable payload with the call's compact length prefix restored, so Vault can
split call from extensions and decode both against its metadata. Oversized
calls fall back to signing the payload's blake2-256 hash (Vault then shows
only the hash — blind signing).

The framed bytes are always wrapped in the legacy multipart envelope
(``0x00 | u16-be frame count | u16-be frame index | chunk``), one QR per
frame — a single frame for every normal transaction.

Pure byte-shuffling: no I/O, no rendering.
"""

from __future__ import annotations

from .._transport.contract import UnsignedExtrinsic

SUBSTRATE_ID = 0x53

CMD_SIGN_TX_HASH = 0x01  # payload is the blake2-256 hash of the signable payload
CMD_SIGN_TX = 0x02  # payload is the full signable payload (call length-prefixed)
CMD_SIGN_MSG = 0x03  # payload is a raw message, displayed as text
# Payload carries an RFC-0078 metadata proof followed by the signable payload;
# Vault decodes and verifies against the proof, needing no pre-loaded metadata.
CMD_SIGN_TX_WITH_PROOF = 0x06

# One multipart chunk per QR code (polkadot-js FRAME_SIZE).
FRAME_SIZE = 1024

# Polkadot-JS hashes the QR payload when the call's hex string ("0x..."
# included) exceeds 5000 characters; expressed here over the raw call bytes.
_HASHED_METHOD_HEX_LIMIT = 5000


def _compact(value: int) -> bytes:
    """SCALE compact-u32 encoding (call lengths never exceed 2**30)."""
    if value < 1 << 6:
        return bytes([value << 2])
    if value < 1 << 14:
        return ((value << 2) | 0b01).to_bytes(2, "little")
    if value < 1 << 30:
        return ((value << 2) | 0b10).to_bytes(4, "little")
    raise ValueError(f"call too large for compact-u32 framing: {value} bytes")


def sign_tx_payload(unsigned: UnsignedExtrinsic) -> tuple[int, bytes]:
    """The ``(cmd, payload)`` pair for a transaction-signing frame.

    Normal transactions get the full signable payload (Vault decodes and
    displays the call). Calls too big for a scannable QR degrade to the
    hashed form — ``unsigned.payload`` already holds the blake2-256 hash by
    the Substrate oversize convention, and Vault shows only the hash.
    """
    if len(unsigned.call_data) * 2 + 2 > _HASHED_METHOD_HEX_LIMIT:
        if len(unsigned.payload) != 32:
            raise ValueError("oversized call without a hashed signing payload")
        return CMD_SIGN_TX_HASH, unsigned.payload
    full = (
        _compact(len(unsigned.call_data))
        + unsigned.call_data
        + unsigned.included_in_extrinsic
        + unsigned.included_in_signed_data
    )
    return CMD_SIGN_TX, full


def sign_frame(
    *,
    crypto_type: int,
    cmd: int,
    public_key: bytes,
    payload: bytes,
    genesis_hash: bytes,
) -> bytes:
    """One complete UOS signing frame (before the multipart envelope)."""
    if len(public_key) != 32:
        raise ValueError(f"expected a 32-byte public key, got {len(public_key)}")
    if len(genesis_hash) != 32:
        raise ValueError(f"expected a 32-byte genesis hash, got {len(genesis_hash)}")
    if crypto_type not in (0, 1):
        raise ValueError(f"unsupported crypto type for Vault signing: {crypto_type}")
    return bytes([SUBSTRATE_ID, crypto_type, cmd]) + public_key + payload + genesis_hash


def multipart_frames(data: bytes) -> list[bytes]:
    """Wrap a frame in the legacy multipart envelope, one entry per QR code.

    Polkadot-JS wraps even single-frame payloads, and Vault expects it.
    """
    chunks = [data[idx : idx + FRAME_SIZE] for idx in range(0, len(data), FRAME_SIZE)] or [b""]
    count = len(chunks).to_bytes(2, "big")
    return [
        b"\x00" + count + index.to_bytes(2, "big") + chunk for index, chunk in enumerate(chunks)
    ]


def transaction_frames(unsigned: UnsignedExtrinsic) -> tuple[list[bytes], bool]:
    """QR-ready multipart frames for signing ``unsigned``, plus whether the
    payload had to be hashed (Vault will blind-sign in that case).

    The legacy (portal-metadata) framing: Vault decodes against metadata it
    loaded earlier from a metadata portal QR.
    """
    cmd, payload = sign_tx_payload(unsigned)
    frame = sign_frame(
        crypto_type=unsigned.crypto_type,
        cmd=cmd,
        public_key=unsigned.public_key,
        payload=payload,
        genesis_hash=bytes.fromhex(unsigned.genesis_hash.removeprefix("0x")),
    )
    return multipart_frames(frame), cmd == CMD_SIGN_TX_HASH


def transaction_frames_with_proof(
    unsigned: UnsignedExtrinsic, metadata_proof: bytes
) -> list[bytes]:
    """QR-ready multipart frames carrying the RFC-0078 metadata proof.

    Vault (7.5+) parses this as ``MetadataProof ++ compact-length-prefixed
    call ++ extensions`` and decodes the transaction against the embedded
    proof — no portal metadata on the phone, no staleness after runtime
    upgrades. The proof is display/verification material only; the signable
    bytes (and thus the signature) are identical to the legacy framing.
    """
    body = (
        metadata_proof
        + _compact(len(unsigned.call_data))
        + unsigned.call_data
        + unsigned.included_in_extrinsic
        + unsigned.included_in_signed_data
    )
    frame = sign_frame(
        crypto_type=unsigned.crypto_type,
        cmd=CMD_SIGN_TX_WITH_PROOF,
        public_key=unsigned.public_key,
        payload=body,
        genesis_hash=bytes.fromhex(unsigned.genesis_hash.removeprefix("0x")),
    )
    return multipart_frames(frame)
