"""The SCALE codec seam, backed by the Rust core (``bittensor_core``).

A :class:`RuntimeCodec` is one runtime's complete encode/decode capability,
built from the raw ``MetadataVersioned`` bytes of that runtime. Everything the
rest of the transport needs from the codec engine flows through this class
(plus the module-level ss58 / multisig helpers).

All methods take and return plain Python values and ``bytes``. The one named
value that escapes is :class:`~.contract.CallBytes` — raw SCALE call bytes
plus the spec version they were composed against — returned by
:meth:`RuntimeCodec.compose_call` and accepted back anywhere a call goes.
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass
from functools import cached_property
from typing import Any, Optional

import bittensor_core as _core

from .const import SS58_FORMAT
from .contract import (
    CallArgIR,
    CallBytes,
    CallIR,
    ErrorIR,
    MetadataIR,
    MultisigAccount,
    PalletIR,
    RuntimeApiIR,
    StorageIR,
)
from .errors import StorageFunctionNotFound

logger = logging.getLogger("bittensor.transport")

_METADATA_MAGIC = b"meta"


def strip_option_opaque_metadata(data: bytes) -> Optional[bytes]:
    """Raw metadata bytes from a SCALE ``Option<OpaqueMetadata>`` response, or None."""
    if not data or data[0] == 0:
        return None
    data = data[1:]  # Option::Some
    mode = data[0] & 0x03  # compact-encoded Vec<u8> length
    if mode == 0:
        length, offset = data[0] >> 2, 1
    elif mode == 1:
        length, offset = int.from_bytes(data[:2], "little") >> 2, 2
    elif mode == 2:
        length, offset = int.from_bytes(data[:4], "little") >> 2, 4
    else:
        byte_count = (data[0] >> 2) + 4
        length, offset = int.from_bytes(data[1 : 1 + byte_count], "little"), 1 + byte_count
    return data[offset : offset + length]


# --- module-level address helpers ------------------------------------------------


def _public_key_bytes(public_key: bytes | str) -> bytes:
    if isinstance(public_key, str):
        return bytes.fromhex(public_key.removeprefix("0x"))
    return bytes(public_key)


def ss58_encode(public_key: bytes | str, ss58_format: int = SS58_FORMAT) -> str:
    return _core.ss58_encode(_public_key_bytes(public_key), ss58_format)


def ss58_decode(address: str, ss58_format: Optional[int] = None) -> str:
    """Hex public key (no 0x) for an ss58 address.

    ``ss58_format`` additionally requires the address to carry exactly that
    format prefix.
    """
    public_key = bytes(_core.ss58_decode(address))
    if ss58_format is not None and _core.ss58_encode(public_key, ss58_format) != address:
        raise ValueError(f"{address} is not an ss58 format {ss58_format} address")
    return public_key.hex()


def is_valid_ss58_address(value: str, ss58_format: Optional[int] = None) -> bool:
    try:
        ss58_decode(value, ss58_format)
    except (ValueError, TypeError):
        return False
    return True


def multisig_account(
    signatories: list[str], threshold: int, ss58_format: int = SS58_FORMAT
) -> MultisigAccount:
    """Derive the deterministic M-of-N multisig account for a signer set."""
    keys = [bytes.fromhex(ss58_decode(address)) for address in signatories]
    account, sorted_keys = _core.multisig_account_id(keys, threshold)
    return MultisigAccount(
        signatories=[ss58_encode(key, ss58_format) for key in sorted_keys],
        threshold=threshold,
        public_key=bytes(account),
        ss58_address=ss58_encode(bytes(account), ss58_format),
    )


def _composed_calls_to_bytes(value: Any) -> Any:
    """Replace embedded :class:`CallBytes` with their raw bytes, recursively.

    The core's call encoder accepts pre-composed calls as raw SCALE bytes, so
    nested composition (Sudo, batches, proxies, multisig) is a byte splice.
    """
    if isinstance(value, CallBytes):
        return value.data
    if isinstance(value, dict):
        return {key: _composed_calls_to_bytes(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_composed_calls_to_bytes(item) for item in value]
    return value


@dataclass
class StorageEntry:
    """Everything needed to build keys for / decode values of one storage item.

    Type references (``value_type``, ``param_types``) are ``scale_info::N``
    strings valid for the runtime that produced the entry.
    """

    pallet: str
    name: str
    prefix: str  # the pallet's storage prefix string (usually the pallet name)
    value_type: str
    param_types: list[str]
    param_hashers: list[str]
    modifier: str  # "Default" | "Optional"
    default_bytes: bytes

    def decode_type(self, has_data: bool) -> str:
        """The type a raw response decodes as: misses on Optional items are Option<T>."""
        if has_data or self.modifier == "Default":
            return self.value_type
        return f"Option<{self.value_type}>"


class RuntimeCodec:
    """One runtime's SCALE encode/decode capability, built from raw metadata bytes."""

    def __init__(
        self,
        metadata_bytes: bytes,
        *,
        spec_version: int,
        transaction_version: int,
        spec_name: str = "",
        ss58_format: int = SS58_FORMAT,
    ):
        """``metadata_bytes`` is a raw ``MetadataVersioned`` blob (magic ``meta`` +
        version + body) — the inner bytes of ``Metadata_metadata_at_version`` for
        V15, or the ``state_getMetadata`` response for V14.

        ``spec_name`` (from ``state_getRuntimeVersion``) feeds the RFC-0078
        metadata digest for signers that verify the runtime before signing.
        """
        if metadata_bytes[:4] != _METADATA_MAGIC:
            raise ValueError("metadata bytes must start with the 'meta' magic")
        self.metadata_bytes = bytes(metadata_bytes)
        self.spec_version = spec_version
        self.transaction_version = transaction_version
        self.spec_name = spec_name
        self.ss58_format = ss58_format
        self._rt = _core.Runtime(metadata_bytes, spec_version, transaction_version, ss58_format)

    @property
    def is_v15(self) -> bool:
        return self._rt.is_v15

    # -- generic encode/decode ---------------------------------------------------

    def encode(self, type_string: str, value: Any) -> bytes:
        """SCALE-encode ``value`` as ``type_string``.

        A composed call contributes its raw bytes; ``None`` encodes as the
        0x00 Option-None byte (matching the behavior the SDK has always relied
        on for optional call params).
        """
        if value is None:
            return b"\x00"
        return self._rt.encode(type_string, _composed_calls_to_bytes(value))

    def decode(self, type_string: str, data: bytes, *, strict: bool = True) -> Any:
        """Decode SCALE ``data`` as ``type_string``, returning a plain value."""
        return self._rt.decode(type_string, bytes(data), strict)

    def batch_decode(self, type_strings: list[str], datas: list[bytes]) -> list[Any]:
        """Bulk decode in one crossing into the core; the read-heavy hot loop."""
        if not type_strings:
            return []
        return self._rt.batch_decode(type_strings, [bytes(data) for data in datas])

    def type_id_of(self, type_name: str) -> Optional[int]:
        """Portable-registry type id for a named type (e.g. ``Vec<NeuronInfoLite>``)."""
        return self._rt.type_id_of(type_name)

    def type_name_of(self, type_id: int) -> Optional[str]:
        return self._rt.type_name_of(type_id)

    def registry_types(self) -> list[dict]:
        """The portable registry as plain data: ``[{"id", "type": {...}}]``.

        For registry-walking tooling (the shape-corpus recorder); not a hot
        path.
        """
        return json.loads(self._rt.registry_json())["types"]

    def decode_by_type_name(self, type_name: str, data: bytes) -> Any:
        """Decode by portable-registry type *name* (legacy runtime-call results)."""
        type_id = self.type_id_of(type_name)
        if type_id is None:
            raise ValueError(f"Type {type_name!r} not found in this runtime's registry")
        return self.decode(f"scale_info::{type_id}", data)

    # -- calls ------------------------------------------------------------------------

    def compose_call(self, module: str, function: str, params: dict) -> CallBytes:
        data = self._rt.compose_call(module, function, _composed_calls_to_bytes(params or {}))
        return CallBytes(data=bytes(data), spec_version=self.spec_version)

    @staticmethod
    def call_data(call: CallBytes | bytes) -> bytes:
        """The raw SCALE bytes of a composed call (accepts raw bytes as-is)."""
        if isinstance(call, CallBytes):
            return call.data
        return bytes(call)

    @classmethod
    def call_hash(cls, call: CallBytes | bytes) -> bytes:
        if isinstance(call, CallBytes):
            return call.call_hash
        return CallBytes(data=bytes(call), spec_version=0).call_hash

    def decode_call(self, data: bytes) -> Any:
        """Decode raw call bytes into the plain call dict (module/function/args)."""
        return self._rt.decode_call(bytes(data))

    def call_from_data(self, data: bytes) -> CallBytes:
        """Rebuild a composed call value from its raw SCALE bytes.

        Lets an extrinsic prepared in one step (or process) be assembled in
        another without re-composing.
        """
        return CallBytes(data=bytes(data), spec_version=self.spec_version)

    # -- storage ------------------------------------------------------------------------

    def storage_entry(self, pallet: str, storage_function: str) -> StorageEntry:
        try:
            entry = self._rt.storage_entry(pallet, storage_function)
        except KeyError as error:
            raise StorageFunctionNotFound(str(error).strip("'\"")) from None
        return StorageEntry(
            pallet=entry.pallet,
            name=entry.name,
            prefix=entry.prefix,
            value_type=entry.value_type,
            param_types=list(entry.param_types),
            param_hashers=list(entry.param_hashers),
            modifier=entry.modifier,
            default_bytes=bytes(entry.default_bytes),
        )

    def storage_key(self, entry: StorageEntry, params: list) -> bytes:
        """The full storage key for one item (params may be a partial prefix)."""
        if len(params) > len(entry.param_types):
            raise ValueError(
                f"Storage function {entry.pallet}.{entry.name} accepts at most "
                f"{len(entry.param_types)} parameters, {len(params)} given"
            )
        return bytes(self._rt.storage_key(entry.pallet, entry.name, list(params)))

    def storage_key_batch(self, entry: StorageEntry, params_list: list[list]) -> list[bytes]:
        """Keys for many parameter sets of one item, in one crossing."""
        return [
            bytes(key)
            for key in self._rt.storage_key_batch(
                entry.pallet, entry.name, [list(params) for params in params_list]
            )
        ]

    def decode_storage_key_params(self, entry: StorageEntry, key: bytes, *, fixed: int) -> list:
        """Recover the free map-key components from one full storage key.

        ``fixed`` leading parameters were part of the queried prefix and are
        skipped, not returned.
        """
        return self._rt.decode_storage_key_params(entry.pallet, entry.name, bytes(key), fixed)

    def decode_map_pairs(
        self, entry: StorageEntry, raw_keys: list[bytes], raw_values: list[bytes], *, fixed: int
    ) -> list[tuple[Any, Any]]:
        """Decode one page of a storage map (keys and values) in one crossing.

        Single free key yields a scalar key, multiple yield a tuple.
        """
        return self._rt.decode_map_pairs(entry.pallet, entry.name, raw_keys, raw_values, fixed)

    def decode_map_changes(
        self, entry: StorageEntry, changes: list[tuple[str, Optional[str]]], *, fixed: int
    ) -> list[tuple[Any, Any]]:
        """Decode raw ``state_queryStorageAt`` changes (hex strings) in one crossing.

        ``None`` values (keys deleted between the key listing and the value
        fetch) are skipped. Single free key yields a scalar key, multiple
        yield a tuple.
        """
        # The RPC delivers changes as 2-element lists; the binding wants tuples.
        pairs = [(k, v) for k, v in changes]
        return self._rt.decode_map_changes(entry.pallet, entry.name, pairs, fixed)

    # -- constants ----------------------------------------------------------------------

    def constant(self, module: str, name: str) -> Any:
        """Decoded value of a pallet constant, or None when it does not exist."""
        return self._rt.constant(module, name)

    # -- events / errors -------------------------------------------------------------------

    def module_error(self, module_index: int, error_index: int) -> dict:
        """``{"type": "Module", "name", "docs"}`` for a dispatch module error."""
        name, docs = self._rt.module_error(module_index, error_index)
        return {"type": "Module", "name": name, "docs": docs}

    # -- extrinsics ----------------------------------------------------------------------------

    @property
    def extrinsic_version(self) -> int:
        return self._rt.extrinsic_version

    def signed_extension_identifiers(self) -> list[str]:
        """Ordered identifiers of the runtime's signed extensions.

        Extension-style signers receive these in the payload JSON's
        ``signedExtensions``, so they frame the payload the way this runtime
        expects.
        """
        return list(self._rt.signed_extension_identifiers())

    def encode_era(self, era: dict | str) -> bytes:
        return bytes(self._rt.encode_era(era))

    def era_birth(self, era: dict, current: int) -> int:
        """The block at which a mortal era starts (its ``birth``)."""
        return _core.era_birth(int(era["period"]), int(current))

    def encode_compact(self, value: int) -> bytes:
        return bytes(self._rt.encode("Compact", int(value)))

    def signature_payload_parts(
        self,
        call: CallBytes | bytes,
        *,
        era: dict | str,
        nonce: int,
        tip: int,
        tip_asset_id: Optional[int],
        genesis_hash: str,
        era_block_hash: str,
        metadata_hash: Optional[bytes] = None,
    ) -> tuple[bytes, bytes, bytes]:
        """The signature payload split at its wire seams:
        ``(call_data, included_in_extrinsic, included_in_signed_data)``.

        Their concatenation is the exact unhashed payload. Hardware signers
        that prove the runtime on-device (Ledger's generic app) need the parts
        separately to build the RFC-0078 extrinsic proof.
        """
        if metadata_hash is not None and (
            "CheckMetadataHash" not in self.signed_extension_identifiers()
        ):
            raise ValueError("this runtime does not declare CheckMetadataHash")
        extra, additional = self._rt.signature_payload_parts(
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
            genesis_hash=_h256(genesis_hash),
            era_block_hash=_h256(era_block_hash),
            metadata_hash=metadata_hash,
        )
        return self.call_data(call), bytes(extra), bytes(additional)

    def signature_payload(
        self,
        call: CallBytes | bytes,
        *,
        era: dict | str,
        nonce: int,
        tip: int,
        tip_asset_id: Optional[int],
        genesis_hash: str,
        era_block_hash: str,
        metadata_hash: Optional[bytes] = None,
    ) -> bytes:
        """The exact bytes a signer signs for this call.

        Payloads longer than 256 bytes are blake2b-256 hashed, per the
        Substrate signing convention.

        ``metadata_hash`` flips ``CheckMetadataHash`` to ``Enabled`` and signs
        the given RFC-0078 metadata digest into the payload — required by
        signers that verify the runtime before signing (Ledger's generic app).
        """
        if metadata_hash is not None and (
            "CheckMetadataHash" not in self.signed_extension_identifiers()
        ):
            raise ValueError("this runtime does not declare CheckMetadataHash")
        return bytes(
            self._rt.signature_payload(
                self.call_data(call),
                era=era,
                nonce=nonce,
                tip=tip,
                tip_asset_id=tip_asset_id,
                genesis_hash=_h256(genesis_hash),
                era_block_hash=_h256(era_block_hash),
                metadata_hash=metadata_hash,
            )
        )

    def encode_signed_extrinsic(
        self,
        call: CallBytes | bytes,
        *,
        public_key: bytes,
        signature: bytes,
        signature_version: int,
        era: dict | str,
        nonce: int,
        tip: int,
        tip_asset_id: Optional[int],
        metadata_hash_enabled: bool = False,
    ) -> tuple[bytes, str]:
        """Assemble the full signed extrinsic; returns (bytes, 0x-hex hash).

        ``metadata_hash_enabled`` must match the mode the signature payload was
        built with: the digest itself is implied data (``additional_signed``),
        only the mode byte travels in the extrinsic.
        """
        data, extrinsic_hash = self._rt.encode_signed_extrinsic(
            self.call_data(call),
            public_key=bytes(public_key),
            signature=bytes(signature),
            signature_version=signature_version,
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
            metadata_hash_enabled=metadata_hash_enabled,
        )
        return bytes(data), "0x" + bytes(extrinsic_hash).hex()

    def decode_extrinsic(self, data: bytes | str, *, strict: bool = True) -> Any:
        """Decode one raw extrinsic (hex or bytes) into its plain value dict."""
        if isinstance(data, str):
            data = bytes.fromhex(data.removeprefix("0x"))
        return self._rt.decode_extrinsic(bytes(data), strict)

    # -- runtime APIs (modern, V15) -------------------------------------------------------------

    @cached_property
    def runtime_api_map(self) -> dict[str, dict[str, Any]]:
        """``{api: {method: {"name", "inputs": [(name, type_string)], "output":
        type_string, "docs"}}}`` from V15 metadata (empty for V14)."""
        return self._rt.runtime_api_map()

    # -- metadata IR (for codegen) -----------------------------------------------------------------

    def metadata_ir(self) -> MetadataIR:
        ir = self._rt.metadata_ir()
        pallets = [
            PalletIR(
                name=pallet["name"],
                index=pallet["index"],
                calls=[
                    CallIR(
                        name=call["name"],
                        args=[
                            CallArgIR(name=arg["name"], type_ident=arg["type_ident"])
                            for arg in call["args"]
                        ],
                        docs=call["docs"],
                    )
                    for call in pallet["calls"]
                ],
                errors=[
                    ErrorIR(index=error["index"], name=error["name"], docs=error["docs"])
                    for error in pallet["errors"]
                ],
                storage=[
                    StorageIR(name=item["name"], value_type_ident=item["value_type_ident"])
                    for item in pallet["storage"]
                ],
                constants=pallet["constants"],
            )
            for pallet in ir["pallets"]
        ]
        runtime_apis = [
            RuntimeApiIR(name=api["name"], methods=api["methods"]) for api in ir["runtime_apis"]
        ]
        return MetadataIR(
            spec_version=ir["spec_version"], pallets=pallets, runtime_apis=runtime_apis
        )


def _h256(value: str | bytes) -> bytes:
    """32 hash bytes from 0x-hex or bytes."""
    if isinstance(value, str):
        value = bytes.fromhex(value.removeprefix("0x"))
    return bytes(value)
