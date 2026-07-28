"""Extrinsic construction and outcome resolution.

Signing side: every flow is the same two steps — :func:`prepare_extrinsic`
builds an :class:`UnsignedExtrinsic` (the exact payload bytes plus every field
a signer could need to re-frame them), and :func:`attach_signature` reunites
it with a signature. :func:`create_signed_extrinsic` runs both steps in-process
for the signer shapes the SDK supports: plain sync ``sign``, coroutine ``sign``
(hardware and remote signers), and browser-extension signers exposing
``sign_extrinsic_payload`` (which take a Polkadot-JS ``SignerPayloadJSON``
instead of raw bytes). When signing happens out-of-process (QR / air-gapped
devices), the caller holds the ``UnsignedExtrinsic`` and runs the second step
whenever the signature comes back.

Signers that verify the runtime before signing (Ledger's generic app) expose
``metadata_digest(SigningContext)``; the digest they return is signed into the
payload via the ``CheckMetadataHash`` extension.

Outcome side: the event-walk that turns a block's ``System.Events`` entries
into success/fee/weight/error for one extrinsic, including Bittensor's
MevShield failure events. This is pure logic over decoded events; fetching
lives in the facade.
"""

from __future__ import annotations

import asyncio
import inspect
from hashlib import blake2b
from typing import Any, Optional

from .codec import RuntimeCodec
from .contract import SignedExtrinsic, SigningContext, UnsignedExtrinsic
from .errors import SubstrateRequestException
from .protocols import (
    ExtensionPayloadSigner,
    Keypair,
    MetadataVerifyingSigner,
    UnsignedExtrinsicSigner,
)
from .rpc import RpcSession
from .utils.receipt import (
    dispatch_error_message,
    extract_failure_details,
    extract_fallback_deposit_fee_amount,
    extract_success_weight,
    extract_total_fee_amount,
    is_extrinsic_failure_event,
    is_extrinsic_success_event,
    nested_dispatch_error,
)

IMMORTAL = "00"


class NonceCache:
    """Per-account next-nonce cache for pipelined submissions.

    The first request for an account asks the node (``account_nextIndex``);
    subsequent requests increment locally so concurrent submissions get
    distinct consecutive nonces. A failed submission must clear the account:
    the chain never consumed that nonce.
    """

    def __init__(self, session: RpcSession):
        self._session = session
        self._nonces: dict[str, int] = {}
        self._lock = asyncio.Lock()

    async def next_for(self, address: str, *, use_cache: bool = True) -> int:
        if not use_cache:
            return await self._session.request("account_nextIndex", [address])
        async with self._lock:
            if address not in self._nonces:
                self._nonces[address] = await self._session.request("account_nextIndex", [address])
            else:
                self._nonces[address] += 1
            return self._nonces[address]

    def pin(self, address: str, nonce: int) -> None:
        """Record an explicitly-chosen nonce as the account's latest."""
        self._nonces[address] = nonce

    def clear(self, address: str) -> None:
        self._nonces.pop(address, None)


def signer_payload_json(
    codec: RuntimeCodec,
    *,
    call: Any,
    address: str,
    era: dict | str,
    nonce: int,
    tip: int,
    tip_asset_id: Optional[int],
    genesis_hash: str,
    era_block_hash: str,
    metadata_hash: Optional[bytes] = None,
) -> dict:
    """The Polkadot-JS ``SignerPayloadJSON`` for browser-extension signers."""
    era_hex = "0x00" if era == IMMORTAL else "0x" + codec.encode_era(era).hex()
    # Numeric fields are big-endian value hex at the field's SCALE width —
    # Polkadot-JS parses hex-string ints as BE numbers (AbstractInt), matching
    # what GenericSignerPayload.toPayload emits. SCALE-compact or LE bytes here
    # make the extension sign different bytes than the SDK assembles (BadProof).
    # ``method`` and ``era`` are raw SCALE hex by the same convention.
    payload: dict[str, Any] = {
        "address": address,
        "blockHash": era_block_hash,
        "genesisHash": genesis_hash,
        "method": "0x" + codec.call_data(call).hex(),
        "nonce": "0x" + nonce.to_bytes(4, "big").hex(),
        "specVersion": "0x" + codec.spec_version.to_bytes(4, "big").hex(),
        "tip": "0x" + tip.to_bytes(16, "big").hex(),
        "transactionVersion": "0x" + codec.transaction_version.to_bytes(4, "big").hex(),
        "era": era_hex,
        "version": codec.extrinsic_version,
    }
    signed_extensions = codec.signed_extension_identifiers()
    if signed_extensions:
        payload["signedExtensions"] = signed_extensions
    if tip_asset_id is not None:
        payload["assetId"] = "0x" + codec.encode_compact(tip_asset_id).hex()
    # Polkadot-JS wire shape: ``mode`` is a plain number and a disabled
    # ``metadataHash`` is null (GenericSignerPayload.toPayload).
    if metadata_hash is not None:
        payload["mode"] = 1
        payload["metadataHash"] = "0x" + metadata_hash.hex()
    elif "CheckMetadataHash" in signed_extensions:
        payload["mode"] = 0
        payload["metadataHash"] = None
    return payload


def prepare_extrinsic(
    codec: RuntimeCodec,
    call: Any,
    *,
    address: str,
    public_key: bytes,
    crypto_type: int,
    era: dict | str,
    nonce: int,
    tip: int = 0,
    tip_asset_id: Optional[int] = None,
    genesis_hash: str,
    era_block_hash: str,
    metadata_hash: Optional[bytes] = None,
) -> UnsignedExtrinsic:
    """Build the unsigned extrinsic: the payload plus every field a signer needs.

    Pure local work over the codec — no keys, no network. The result is
    self-contained: it can cross a process boundary (QR display, file export)
    and later be reunited with a signature via :func:`attach_signature`.
    """
    call_data, included_in_extrinsic, included_in_signed_data = codec.signature_payload_parts(
        call,
        era=era,
        nonce=nonce,
        tip=tip,
        tip_asset_id=tip_asset_id,
        genesis_hash=genesis_hash,
        era_block_hash=era_block_hash,
        metadata_hash=metadata_hash,
    )
    payload = call_data + included_in_extrinsic + included_in_signed_data
    if len(payload) > 256:
        # Substrate signing convention: oversized payloads are signed by hash.
        payload = blake2b(payload, digest_size=32).digest()
    return UnsignedExtrinsic(
        call_data=call_data,
        address=address,
        public_key=bytes(public_key),
        crypto_type=crypto_type,
        era=era,
        nonce=nonce,
        tip=tip,
        tip_asset_id=tip_asset_id,
        genesis_hash=genesis_hash,
        era_block_hash=era_block_hash,
        spec_version=codec.spec_version,
        transaction_version=codec.transaction_version,
        metadata_hash=metadata_hash,
        payload=payload,
        payload_json=signer_payload_json(
            codec,
            call=call,
            address=address,
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
            genesis_hash=genesis_hash,
            era_block_hash=era_block_hash,
            metadata_hash=metadata_hash,
        ),
        included_in_extrinsic=included_in_extrinsic,
        included_in_signed_data=included_in_signed_data,
    )


def _normalize_signature(signature: bytes | str, default_version: int) -> tuple[bytes, int]:
    """(raw 64-byte signature, signature version).

    A 65-byte signature carries its version in the first byte (the MultiSignature
    convention extensions and hardware devices return); a 64-byte one uses the
    signer's crypto type.
    """
    if isinstance(signature, str):
        signature = bytes.fromhex(signature.removeprefix("0x"))
    if len(signature) == 65:
        return signature[1:], signature[0]
    return signature, default_version


def attach_signature(
    codec: RuntimeCodec,
    unsigned: UnsignedExtrinsic,
    signature: bytes | str,
) -> SignedExtrinsic:
    """Assemble the submittable extrinsic from an unsigned one plus a signature.

    The second half of :func:`prepare_extrinsic`: the signature may come from
    anywhere — an in-process keypair, an extension, a QR round-trip. The call
    is spliced back in from its raw bytes, so nothing needs re-composing.
    """
    signature, signature_version = _normalize_signature(signature, unsigned.crypto_type)
    data, extrinsic_hash = codec.encode_signed_extrinsic(
        unsigned.call_data,
        public_key=unsigned.public_key,
        signature=signature,
        signature_version=signature_version,
        era=unsigned.era,
        nonce=unsigned.nonce,
        tip=unsigned.tip,
        tip_asset_id=unsigned.tip_asset_id,
        metadata_hash_enabled=unsigned.metadata_hash is not None,
    )
    return SignedExtrinsic(data=data, extrinsic_hash=extrinsic_hash)


async def resolve_metadata_hash(
    codec: RuntimeCodec, keypair: Any, genesis_hash: str
) -> Optional[bytes]:
    """The CheckMetadataHash digest this signer requires, or None.

    Signers that verify the runtime before signing match
    :class:`MetadataVerifyingSigner` (sync or async); everything they need to
    compute the RFC-0078 digest is in the context.
    """
    if not isinstance(keypair, MetadataVerifyingSigner):
        return None
    digest = keypair.metadata_digest(
        SigningContext(
            metadata_bytes=codec.metadata_bytes,
            spec_version=codec.spec_version,
            spec_name=codec.spec_name,
            transaction_version=codec.transaction_version,
            ss58_format=codec.ss58_format,
            genesis_hash=genesis_hash,
        )
    )
    if inspect.isawaitable(digest):
        digest = await digest
    return bytes(digest) if digest is not None else None


async def create_signed_extrinsic(
    codec: RuntimeCodec,
    call: Any,
    keypair: Keypair,
    *,
    era: dict | str,
    nonce: int,
    tip: int = 0,
    tip_asset_id: Optional[int] = None,
    genesis_hash: str,
    era_block_hash: str,
    signature: Optional[bytes | str] = None,
) -> SignedExtrinsic:
    """Sign ``call`` and assemble the extrinsic: prepare, sign, attach.

    ``era`` must already be normalized ("00" or a dict containing ``current``).
    ``signature`` short-circuits signing (externally-signed or fee-estimation
    paths); a 65-byte value carries the signature version in its first byte.
    """
    public_key = keypair.public_key
    assert public_key is not None
    if signature is not None:
        # No signing happens, so skip building the payloads entirely — fee
        # estimation runs this path for every quote.
        signature, signature_version = _normalize_signature(signature, keypair.crypto_type)
        data, extrinsic_hash = codec.encode_signed_extrinsic(
            call,
            public_key=bytes(public_key),
            signature=signature,
            signature_version=signature_version,
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
        )
        return SignedExtrinsic(data=data, extrinsic_hash=extrinsic_hash)
    unsigned = prepare_extrinsic(
        codec,
        call,
        address=keypair.ss58_address,
        public_key=bytes(public_key),
        crypto_type=keypair.crypto_type,
        era=era,
        nonce=nonce,
        tip=tip,
        tip_asset_id=tip_asset_id,
        genesis_hash=genesis_hash,
        era_block_hash=era_block_hash,
        metadata_hash=await resolve_metadata_hash(codec, keypair, genesis_hash),
    )
    return attach_signature(codec, unsigned, await sign_unsigned(unsigned, keypair))


async def sign_unsigned(unsigned: UnsignedExtrinsic, keypair: Any) -> bytes:
    """Obtain a signature for a prepared extrinsic from an in-process signer.

    Dispatches on the signer's shape: :class:`UnsignedExtrinsicSigner` (takes
    the whole prepared extrinsic — hardware clear-signing),
    :class:`ExtensionPayloadSigner` (takes the Polkadot-JS payload JSON) when
    matched, else ``sign`` over the raw payload bytes; any may be a coroutine.
    """
    if isinstance(keypair, UnsignedExtrinsicSigner):
        signed = keypair.sign_unsigned_extrinsic(unsigned)
        if inspect.isawaitable(signed):
            signed = await signed
        assert isinstance(signed, bytes)
        return signed
    if isinstance(keypair, ExtensionPayloadSigner):
        result = keypair.sign_extrinsic_payload(unsigned.payload_json)
        if inspect.isawaitable(result):
            result = await result
        signature_hex = result.get("signature") if isinstance(result, dict) else None
        if not isinstance(signature_hex, str):
            raise ValueError("extension signer did not return a signature")
        return bytes.fromhex(signature_hex.removeprefix("0x"))
    signed = keypair.sign(unsigned.payload)
    if inspect.isawaitable(signed):
        signed = await signed
    assert isinstance(signed, bytes)
    return signed


# Terminal transaction-pool statuses that mean the extrinsic will not be
# included from this submission.
_FATAL_WATCH_STATUSES = ("usurped", "retracted", "finalitytimeout", "dropped", "invalid")


def watch_status_block(
    status: Any, *, wait_for_inclusion: bool, wait_for_finalization: bool
) -> Optional[tuple[str, bool]]:
    """Interpret one ``author_submitAndWatchExtrinsic`` status update.

    Returns ``(block_hash, finalized)`` when the update is the terminal state
    the caller is waiting for, ``None`` when the watch should continue, and
    raises for statuses that mean the extrinsic is out of the running.
    """
    if isinstance(status, str):
        status = {status: None}
    normalized = {key.lower(): value for key, value in status.items()}
    for fatal in _FATAL_WATCH_STATUSES:
        if fatal in normalized:
            raise SubstrateRequestException(f"Extrinsic {fatal}: {status}")
    if "finalized" in normalized and wait_for_finalization:
        return normalized["finalized"], True
    if "inblock" in normalized and wait_for_inclusion and not wait_for_finalization:
        return normalized["inblock"], False
    return None


def resolve_outcome(extrinsic_events: list[dict], codec: RuntimeCodec) -> dict:
    """Success/fee/weight/error for one extrinsic from its triggered events.

    Returns ``{"is_success", "total_fee_amount", "weight", "error_message"}``.
    The fee is ``TransactionPayment.TransactionFeePaid`` when present, else the
    sum of Treasury/Balances deposits (older runtimes). Failures come from
    ``System.ExtrinsicFailed``, Bittensor's MevShield rejection events, or a
    nested ``Err`` inside Sudo/Proxy/Multisig wrapper events (those wrappers
    still emit ``System.ExtrinsicSuccess`` when only the inner call fails).
    Module errors are resolved to their metadata name/docs.
    """
    total_fee, has_fee_paid_event = extract_total_fee_amount(extrinsic_events)
    is_success: Optional[bool] = None
    weight: Any = None
    error_message: Optional[dict] = None
    possible_success = False

    for event in extrinsic_events:
        if is_extrinsic_success_event(event):
            possible_success = True
            weight = extract_success_weight(event)
        elif is_extrinsic_failure_event(event):
            possible_success = False
            is_success = False
            details = extract_failure_details(event)
            if details["has_weight"]:
                weight = details["weight"]
            if details["error_message"] is not None:
                error_message = details["error_message"]
                continue
            error_message = dispatch_error_message(details["dispatch_error"], codec)
        elif not has_fee_paid_event:
            total_fee += extract_fallback_deposit_fee_amount(event)

    # Outer ExtrinsicSuccess with a nested wrapper Err is still a failure.
    if possible_success and error_message is None:
        nested_error = nested_dispatch_error(extrinsic_events)
        if nested_error is not None:
            possible_success = False
            is_success = False
            error_message = dispatch_error_message(nested_error, codec)

    if possible_success and error_message is None:
        is_success = True
    return {
        "is_success": bool(is_success),
        "total_fee_amount": total_fee,
        "weight": weight,
        "error_message": error_message,
    }
