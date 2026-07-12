"""The transport's typed result shapes.

These dataclasses are the values the transport hands to the SDK: plain,
eagerly-populated data — no lazy awaitable properties, no duck typing, no SCALE
objects. Everything here is JSON-native or bytes.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from functools import cached_property
from hashlib import blake2b
from typing import Any, ClassVar, Optional


@dataclass(frozen=True)
class CallBytes:
    """A composed call: raw SCALE call bytes plus the spec version they were
    composed against.

    This is the one opaque-ish value the transport hands out for composed
    calls. It is a plain value, not a codec object: everything derived from a
    call (its hash for multisig, batch embedding, decode-back for display) is
    a pure function of these bytes, so the value serializes across process and
    language boundaries for free.
    """

    data: bytes
    spec_version: int

    @cached_property
    def call_hash(self) -> bytes:
        return blake2b(self.data, digest_size=32).digest()


@dataclass
class SignedExtrinsic:
    """A signed extrinsic ready for submission."""

    data: bytes  # full SCALE-encoded extrinsic (length-prefixed)
    extrinsic_hash: str  # 0x-hex blake2b-256 of ``data``

    @property
    def data_hex(self) -> str:
        return "0x" + self.data.hex()


@dataclass
class SigningContext:
    """The raw materials handed to a signer before it signs.

    Passed to a signer's optional ``metadata_digest`` hook so drivers that
    must prove the runtime to the user (e.g. Ledger's generic app, which
    refuses to blind-sign) can compute the RFC-0078 merkleized-metadata
    digest for the exact runtime the payload targets.
    """

    metadata_bytes: bytes  # raw MetadataVersioned blob (V15 when available)
    spec_version: int
    spec_name: str  # from state_getRuntimeVersion; part of the RFC-0078 digest
    transaction_version: int
    ss58_format: int
    genesis_hash: str


@dataclass
class UnsignedExtrinsic:
    """An extrinsic prepared for signing: everything except the signature.

    One shape serves every signing flow. ``payload`` is the exact bytes an
    sr25519/ed25519 signer signs (already blake2b-hashed when oversized, per
    the Substrate convention); ``payload_json`` is the Polkadot-JS
    ``SignerPayloadJSON`` for extension-style signers; and the individual
    fields carry full fidelity so QR (Polkadot Vault / UOS) and hardware
    drivers can re-frame the transaction however their device expects.
    Reunite it with a signature via ``Client.submit_signature`` (or the
    transport's ``attach_signature``).

    ``to_dict``/``from_dict`` round-trip through JSON-native values (bytes as
    0x-hex), so a prepared extrinsic can cross a process or machine boundary —
    written to a file, shown as a QR, carried to an air-gapped signer — and be
    reconstructed on the submitting side.
    """

    call_data: bytes  # SCALE-encoded call
    address: str  # ss58 address of the signing account
    public_key: bytes
    crypto_type: int  # 0 = ed25519, 1 = sr25519; the default signature version
    era: dict | str  # normalized: "00" (immortal) or a dict with ``current``
    nonce: int
    tip: int
    tip_asset_id: Optional[int]
    genesis_hash: str
    era_block_hash: str
    spec_version: int
    transaction_version: int
    metadata_hash: Optional[bytes]  # CheckMetadataHash digest; None = mode Disabled
    payload: bytes  # the exact bytes to sign
    payload_json: dict  # Polkadot-JS SignerPayloadJSON
    # The payload's wire seams (payload = call_data ++ included_in_extrinsic ++
    # included_in_signed_data, before any oversize hashing). Hardware signers
    # that prove the runtime on-device (Ledger's generic app) need the parts to
    # build the RFC-0078 extrinsic proof.
    included_in_extrinsic: bytes
    included_in_signed_data: bytes

    _BYTES_FIELDS: ClassVar[tuple[str, ...]] = (
        "call_data",
        "public_key",
        "metadata_hash",
        "payload",
        "included_in_extrinsic",
        "included_in_signed_data",
    )

    @property
    def payload_hex(self) -> str:
        return "0x" + self.payload.hex()

    def to_dict(self) -> dict:
        """A JSON-native dict (bytes as 0x-hex) that ``from_dict`` reconstructs exactly."""
        out = asdict(self)
        for name in self._BYTES_FIELDS:
            if out[name] is not None:
                out[name] = "0x" + out[name].hex()
        return out

    @classmethod
    def from_dict(cls, data: dict) -> "UnsignedExtrinsic":
        values = dict(data)
        for name in cls._BYTES_FIELDS:
            if isinstance(values.get(name), str):
                values[name] = bytes.fromhex(values[name].removeprefix("0x"))
        return cls(**values)


@dataclass
class MultisigAccount:
    """A deterministic multisig account derived from a signer set."""

    signatories: list[str]  # ss58, sorted the way the chain sorts them
    threshold: int
    public_key: bytes
    ss58_address: str


@dataclass
class InclusionReport:
    """Outcome of a submitted extrinsic, fully resolved at construction.

    When the submission did not wait for inclusion only ``extrinsic_hash`` is
    set. When it did, the block coordinates, triggered events, success flag,
    fee, and (on failure) the resolved error are all present.
    """

    extrinsic_hash: str
    finalized: bool = False
    block_hash: Optional[str] = None
    block_number: Optional[int] = None
    extrinsic_idx: Optional[int] = None
    is_success: Optional[bool] = None  # None when inclusion was not awaited
    total_fee_amount: Optional[int] = None
    weight: Any = None  # int (WeightV1) or {"ref_time", "proof_size"} (WeightV2)
    triggered_events: list[dict] = field(default_factory=list)
    # {"type": "Module"|"System"|"MevShield", "name": ..., "docs": ...} on failure
    error_message: Optional[dict] = None


@dataclass
class BlockData:
    """One decoded block: header plus decoded extrinsics.

    ``extrinsics`` entries are plain decoded dicts (address/call/signature...),
    or None where an extrinsic failed to decode and decoding errors were
    ignored.
    """

    header: dict
    extrinsics: list[Optional[dict]]


# --- Metadata IR (for codegen) -------------------------------------------------
#
# The single, transport-owned description of the runtime that sdk codegen
# walks without touching decoded SCALE metadata objects. Deliberately small
# and JSON-serializable (``asdict`` works on the whole tree).


@dataclass
class CallArgIR:
    name: str
    # The parameter's type identity: the last path segment of its registry
    # type ("TaoBalance", "NetUid", "AccountId32"), or a primitive/structural
    # name ("u64", "Vec<u16>") when the type has no path.
    type_ident: str


@dataclass
class CallIR:
    name: str
    args: list[CallArgIR]  # parameters, in call order
    docs: str


@dataclass
class ErrorIR:
    index: int
    name: str
    docs: str


@dataclass
class RuntimeApiIR:
    name: str
    methods: list[str]  # method names


@dataclass
class StorageIR:
    name: str
    # The storage VALUE's type identity (for maps: the value after all keys),
    # same convention as CallArgIR.type_ident: last path segment of the
    # registry type ("PerU16", "TaoBalance"), or a primitive/structural name
    # ("u64", "Vec<u16>") when the type has no path.
    value_type_ident: str


@dataclass
class PalletIR:
    name: str
    index: int
    calls: list[CallIR]
    errors: list[ErrorIR]
    storage: list[StorageIR]  # storage entries, in metadata order
    constants: list[str]  # constant names


@dataclass
class MetadataIR:
    spec_version: int
    pallets: list[PalletIR]
    runtime_apis: list[RuntimeApiIR]

    def to_dict(self) -> dict:
        return asdict(self)
