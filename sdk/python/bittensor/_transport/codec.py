"""The SCALE codec seam: the only module that imports ``scalecodec`` (cyscale).

A :class:`RuntimeCodec` is one runtime's complete encode/decode capability,
built from the raw metadata bytes of that runtime. Everything the rest of the
transport needs from cyscale flows through this class (plus the module-level
ss58 / multisig helpers), so replacing the codec engine later means rewriting
this file and nothing else.

All methods take and return plain Python values and ``bytes``. The one opaque
type that escapes is the composed call object returned by :meth:`compose_call`
— the SDK passes it around without looking inside, and every consumer of its
internals lives in this package.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from functools import cached_property
from hashlib import blake2b
from typing import Any, Optional

from scalecodec import ScaleBytes
from scalecodec.base import RuntimeConfigurationObject, ScaleType
from scalecodec.type_registry import load_type_registry_preset
from scalecodec.types import GenericCall, MultiAccountId
from scalecodec.utils.ss58 import is_valid_ss58_address as _is_valid_ss58
from scalecodec.utils.ss58 import ss58_decode as _ss58_decode
from scalecodec.utils.ss58 import ss58_encode as _ss58_encode

from .const import SS58_FORMAT
from .contract import (
    CallIR,
    ErrorIR,
    MetadataIR,
    MultisigAccount,
    PalletIR,
    RuntimeApiIR,
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


def ss58_encode(public_key: bytes | str, ss58_format: int = SS58_FORMAT) -> str:
    return _ss58_encode(public_key, ss58_format=ss58_format)


def ss58_decode(address: str, ss58_format: Optional[int] = None) -> str:
    """Hex public key (no 0x) for an ss58 address."""
    return _ss58_decode(address, valid_ss58_format=ss58_format)


def is_valid_ss58_address(value: str, ss58_format: Optional[int] = None) -> bool:
    return _is_valid_ss58(value, valid_ss58_format=ss58_format)


def _prime_runtime_configuration() -> None:
    """Ensure scalecodec's global RuntimeConfiguration singleton is populated.

    ``MultiAccountId.create_from_account_list`` builds its AccountId objects
    through that singleton, which starts empty in a fresh process. Everywhere
    else the codec passes an explicit ``RuntimeConfigurationObject``, so this
    is only needed for the account-list helper. Instantiating the object with
    a type registry populates the shared singleton state (scalecodec caches
    the registry at class level); doing it once per process is enough and
    repeat calls are cheap no-ops.
    """
    rc = RuntimeConfigurationObject(ss58_format=SS58_FORMAT)
    if rc.get_decoder_class("AccountId") is None:
        rc.update_type_registry(load_type_registry_preset(name="core") or {})


def multisig_account(
    signatories: list[str], threshold: int, ss58_format: int = SS58_FORMAT
) -> MultisigAccount:
    """Derive the deterministic M-of-N multisig account for a signer set."""
    _prime_runtime_configuration()
    multi = MultiAccountId.create_from_account_list(signatories, threshold)
    public_key = bytes.fromhex(multi.value.replace("0x", ""))
    return MultisigAccount(
        signatories=[ss58_encode(pub, ss58_format) for pub in multi.signatories],
        threshold=threshold,
        public_key=public_key,
        ss58_address=ss58_encode(public_key, ss58_format),
    )


class LegacyCodec:
    """Codec over the legacy (pre-scale-info) type registry.

    Only the legacy Bittensor runtime-call registry uses this: old runtimes
    whose runtime APIs return ``Vec<u8>`` payloads described by hand-written
    type definitions instead of the portable registry.
    """

    def __init__(self, extra_types: Optional[dict] = None, ss58_format: int = SS58_FORMAT):
        rc = RuntimeConfigurationObject(ss58_format=ss58_format)
        rc.update_type_registry(load_type_registry_preset(name="legacy") or {})
        if extra_types:
            rc.update_type_registry(extra_types)
        self._rc = rc

    def encode(self, type_string: str, value: Any) -> bytes:
        return bytes(self._rc.create_scale_object(type_string).encode(value).data)

    def decode(self, type_string: str, data: bytes) -> Any:
        obj = self._rc.create_scale_object(type_string, data=ScaleBytes(data))
        return obj.decode()


@dataclass
class StorageEntry:
    """Everything needed to build keys for / decode values of one storage item."""

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
        ss58_format: int = SS58_FORMAT,
        extra_types: Optional[dict] = None,
        is_v15: bool = True,
    ):
        """``metadata_bytes`` is a raw ``MetadataVersioned`` blob (magic ``meta`` +
        version + body) — the inner bytes of ``Metadata_metadata_at_version`` for
        V15, or the ``state_getMetadata`` response for V14.

        ``extra_types`` is the chain-specific type-registry overlay (for
        Bittensor: ``{"types": {"Balance": "u64"}}``).
        """
        if metadata_bytes[:4] != _METADATA_MAGIC:
            raise ValueError("metadata bytes must start with the 'meta' magic")
        self.metadata_bytes = metadata_bytes
        self.spec_version = spec_version
        self.transaction_version = transaction_version
        self.ss58_format = ss58_format
        self.extra_types = extra_types or {}
        self.is_v15 = is_v15

        rc = RuntimeConfigurationObject(ss58_format=ss58_format, implements_scale_info=True)
        rc.clear_type_registry()
        rc.update_type_registry(load_type_registry_preset(name="core") or {})
        metadata = rc.create_scale_object("MetadataVersioned", data=ScaleBytes(metadata_bytes))
        metadata.decode()
        self._metadata = metadata
        rc.set_active_spec_version_id(spec_version)
        rc.add_portable_registry(metadata)
        if self.extra_types:
            rc.update_type_registry(self.extra_types)
        # Weight encoding changed shape (v1 int -> v2 struct); probe which one
        # this runtime uses so "Weight" always resolves.
        try:
            rc.create_scale_object("sp_weights::weight_v2::Weight")
            rc.update_type_registry_types({"Weight": "sp_weights::weight_v2::Weight"})
        except NotImplementedError:
            rc.update_type_registry_types({"Weight": "WeightV1"})
        self._rc = rc

    # -- generic encode/decode ---------------------------------------------------

    def encode(self, type_string: str, value: Any) -> bytes:
        """SCALE-encode ``value`` as ``type_string``.

        A value that is already a decoded SCALE object contributes its original
        bytes; ``None`` encodes as the 0x00 Option-None byte (matching the
        behavior the SDK has always relied on for optional call params).
        """
        if value is None:
            return b"\x00"
        if isinstance(value, ScaleType):
            if value.data is not None and value.data.data is not None:
                return bytes(value.data.data)
            value = value.value
        return bytes(self._rc.create_scale_object(type_string).encode(value).data)

    def decode(self, type_string: str, data: bytes, *, strict: bool = True) -> Any:
        """Decode SCALE ``data`` as ``type_string``, returning a plain value."""
        obj = self._rc.create_scale_object(
            type_string, data=ScaleBytes(data), metadata=self._metadata
        )
        obj.decode(check_remaining=strict)
        return obj.value

    def batch_decode(self, type_strings: list[str], datas: list[bytes]) -> list[Any]:
        """Bulk decode on cyscale's fast path; the read-heavy hot loop."""
        if not type_strings:
            return []
        return self._rc.batch_decode(type_strings, datas)

    def type_id_of(self, type_name: str) -> Optional[int]:
        """Portable-registry type id for a named type (e.g. ``Vec<NeuronInfoLite>``)."""
        return self._registry_type_map.get(type_name)

    def type_name_of(self, type_id: int) -> Optional[str]:
        return self._type_id_to_name.get(type_id)

    def decode_by_type_name(self, type_name: str, data: bytes) -> Any:
        """Decode by portable-registry type *name* (legacy runtime-call results)."""
        type_id = self.type_id_of(type_name)
        if type_id is None:
            raise ValueError(f"Type {type_name!r} not found in this runtime's registry")
        return self.batch_decode([f"scale_info::{type_id}"], [data])[0]

    # -- calls ------------------------------------------------------------------------

    def compose_call(self, module: str, function: str, params: dict) -> GenericCall:
        call = self._rc.create_scale_object(type_string="Call", metadata=self._metadata)
        call.encode({"call_module": module, "call_function": function, "call_args": params or {}})
        return call

    @staticmethod
    def call_data(call: GenericCall) -> bytes:
        return bytes(call.data.data)

    @staticmethod
    def call_hash(call: GenericCall) -> bytes:
        return bytes(call.call_hash)

    def decode_call(self, data: bytes) -> Any:
        """Decode raw call bytes into the plain call dict (module/function/args)."""
        return self.decode("Call", data)

    def call_from_data(self, data: bytes) -> GenericCall:
        """Rebuild a composed call object from its raw SCALE bytes.

        The inverse of :meth:`call_data`; lets an extrinsic prepared in one
        step (or process) be assembled in another without re-composing.
        """
        call = self._rc.create_scale_object(
            type_string="Call", metadata=self._metadata, data=ScaleBytes(data)
        )
        call.decode()
        return call

    # -- storage ------------------------------------------------------------------------

    def storage_entry(self, pallet: str, storage_function: str) -> StorageEntry:
        metadata_pallet = self._metadata.get_metadata_pallet(pallet)
        if not metadata_pallet:
            raise StorageFunctionNotFound(f'Pallet "{pallet}" not found')
        item = metadata_pallet.get_storage_function(storage_function)
        if not item:
            raise StorageFunctionNotFound(
                f'Storage function "{pallet}.{storage_function}" not found'
            )
        return StorageEntry(
            pallet=pallet,
            name=item.value["name"],
            prefix=metadata_pallet.value["storage"]["prefix"],
            value_type=item.get_value_type_string(),
            param_types=item.get_params_type_string(),
            param_hashers=item.get_param_hashers(),
            modifier=item.value["modifier"],
            default_bytes=bytes(item.value_object["default"].value_object),
        )

    def encode_storage_param(self, type_string: str, value: Any) -> bytes:
        """Encode one storage-key parameter (ss58 addresses become raw AccountId)."""
        if isinstance(value, bytes):
            value = "0x" + value.hex()
        if type_string == "AccountId" and isinstance(value, str) and not value.startswith("0x"):
            value = "0x" + ss58_decode(value, self.ss58_format)
        return bytes(self._rc.create_scale_object(type_string).encode(value).data)

    # -- constants ----------------------------------------------------------------------

    def constant(self, module: str, name: str) -> Any:
        """Decoded value of a pallet constant, or None when it does not exist."""
        for pallet in self._metadata.pallets:
            if pallet.name != module or not pallet.constants:
                continue
            for constant in pallet.constants:
                if constant.value["name"] == name:
                    return self.decode(constant.type, bytes(constant.constant_value))
        return None

    # -- events / errors -------------------------------------------------------------------

    def module_error(self, module_index: int, error_index: int) -> dict:
        """``{"type": "Module", "name", "docs"}`` for a dispatch module error."""
        error = self._metadata.get_module_error(module_index=module_index, error_index=error_index)
        return {"type": "Module", "name": error.name, "docs": error.docs}

    # -- extrinsics ----------------------------------------------------------------------------

    @property
    def extrinsic_version(self) -> int:
        # int() unwraps the decoded SCALE U8 so consumers (and JSON export of
        # the signer payload) get a plain number.
        return int(self._metadata[1][1]["extrinsic"]["version"])

    def signed_extension_identifiers(self) -> list[str]:
        """Ordered identifiers of the runtime's signed extensions.

        Extension-style signers receive these in the payload JSON's
        ``signedExtensions``, so they frame the payload the way this runtime
        expects. Entries are decoded ``SignedExtensionMetadataV14`` objects
        whose values stringify to the identifier.
        """
        extrinsic_meta = self._metadata[1][1]["extrinsic"]
        if "signed_extensions" not in extrinsic_meta:
            return []
        out = []
        for entry in extrinsic_meta["signed_extensions"]:
            value = entry.value if hasattr(entry, "value") else entry
            identifier = value.get("identifier") if isinstance(value, dict) else None
            if isinstance(identifier, str):
                out.append(identifier)
        return out

    def encode_era(self, era: dict | str) -> bytes:
        era_obj = self._rc.create_scale_object("Era")
        era_obj.encode(era)
        return bytes(era_obj.data.data)

    def era_birth(self, era: dict, current: int) -> int:
        """The block at which a mortal era starts (its ``birth``)."""
        era_obj = self._rc.create_scale_object("Era")
        era_obj.encode(era)
        return era_obj.birth(current)

    def encode_compact(self, value: int) -> bytes:
        return bytes(self._rc.create_scale_object("Compact").encode(int(value)).data)

    # The signature payload is ``call ++ extra ++ additional``: each signed
    # extension the runtime declares contributes its "extra" bytes (signed
    # alongside the call) and then its "additional" bytes (implied data both
    # sides must agree on). This table IS the payload wire format — order
    # matters and is pinned by the golden signing-payload vectors.
    _PAYLOAD_FIELDS: tuple[tuple[str, str, str], ...] = (
        # (payload field, signed extension, which of its type slots to use)
        ("era", "CheckMortality", "extrinsic"),
        ("era", "CheckEra", "extrinsic"),
        ("nonce", "CheckNonce", "extrinsic"),
        ("tip", "ChargeTransactionPayment", "extrinsic"),
        ("asset_id", "ChargeAssetTxPayment", "extrinsic"),
        ("mode", "CheckMetadataHash", "extrinsic"),
        ("spec_version", "CheckSpecVersion", "additional_signed"),
        ("transaction_version", "CheckTxVersion", "additional_signed"),
        ("genesis_hash", "CheckGenesis", "additional_signed"),
        ("block_hash", "CheckMortality", "additional_signed"),
        ("block_hash", "CheckEra", "additional_signed"),
        ("metadata_hash", "CheckMetadataHash", "additional_signed"),
    )

    def signature_payload(
        self,
        call: GenericCall,
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

        The field order follows ``_PAYLOAD_FIELDS`` filtered to the signed
        extensions this runtime actually declares. Payloads longer than 256
        bytes are blake2b-256 hashed, per the Substrate signing convention.

        ``metadata_hash`` flips ``CheckMetadataHash`` to ``Enabled`` and signs
        the given RFC-0078 metadata digest into the payload — required by
        signers that verify the runtime before signing (Ledger's generic app).
        """
        payload = self._rc.create_scale_object("ExtrinsicPayloadValue")
        signed_extensions = self._metadata.get_signed_extensions()
        payload.type_mapping = [["call", "CallBytes"]] + [
            [field, signed_extensions[extension][slot]]
            for field, extension, slot in self._PAYLOAD_FIELDS
            if extension in signed_extensions
        ]
        if metadata_hash is not None and "CheckMetadataHash" not in signed_extensions:
            raise ValueError("this runtime does not declare CheckMetadataHash")
        payload.encode(
            {
                "call": str(call.data),
                "era": era,
                "nonce": nonce,
                "tip": tip,
                "spec_version": self.spec_version,
                "genesis_hash": genesis_hash,
                "block_hash": era_block_hash,
                "transaction_version": self.transaction_version,
                "asset_id": {"tip": tip, "asset_id": tip_asset_id},
                "metadata_hash": (
                    "0x" + metadata_hash.hex() if metadata_hash is not None else None
                ),
                "mode": "Enabled" if metadata_hash is not None else "Disabled",
            }
        )
        data = bytes(payload.data.data)
        if len(data) > 256:
            return blake2b(data, digest_size=32).digest()
        return data

    def encode_signed_extrinsic(
        self,
        call: GenericCall,
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
        if self.extrinsic_version != 4:
            raise NotImplementedError(f"Extrinsic version {self.extrinsic_version} not supported")
        extrinsic = self._rc.create_scale_object(type_string="Extrinsic", metadata=self._metadata)
        value = {
            "account_id": "0x" + public_key.hex(),
            "signature": "0x" + signature.hex(),
            # The call object goes in whole so its already-encoded bytes are
            # embedded verbatim. Spreading call_module/call_args here would
            # re-encode them — which scalecodec cannot do for a *decoded* call
            # carrying a nested call (Proxy.proxy, Utility.batch, ...), the
            # normal case when a signature returns from an offline signer.
            "call": call,
            "nonce": nonce,
            "era": era,
            "tip": tip,
            "asset_id": {"tip": tip, "asset_id": tip_asset_id},
            "mode": "Enabled" if metadata_hash_enabled else "Disabled",
        }
        # Multi-crypto chains carry the signature scheme in an enum wrapper.
        signature_cls = self._rc.get_decoder_class("ExtrinsicSignature")
        enum_cls = self._rc.get_decoder_class("Enum")
        if (
            signature_cls is not None
            and enum_cls is not None
            and issubclass(signature_cls, enum_cls)
        ):
            value["signature_version"] = signature_version
        extrinsic.encode(value)
        return bytes(extrinsic.data.data), "0x" + extrinsic.extrinsic_hash.hex()

    def decode_extrinsic(self, data: bytes | str, *, strict: bool = True) -> Any:
        """Decode one raw extrinsic (hex or bytes) into its plain value dict."""
        if isinstance(data, str):
            data = bytes.fromhex(data.removeprefix("0x"))
        extrinsic = self._rc.create_scale_object(
            type_string="Extrinsic", metadata=self._metadata, data=ScaleBytes(data)
        )
        extrinsic.decode(check_remaining=strict)
        return extrinsic.value

    # -- runtime APIs (modern, V15) -------------------------------------------------------------

    @cached_property
    def runtime_api_map(self) -> dict[str, dict[str, Any]]:
        """``{api: {method: definition}}`` from V15 metadata (empty for V14)."""
        if not self.is_v15:
            return {}
        v15 = self._metadata.get_metadata().value_object[1].value
        return {
            api_entry["name"]: {m["name"]: m for m in api_entry["methods"]}
            for api_entry in v15["apis"]
        }

    # -- registry name map (for the legacy Bittensor runtime-call registry) ----------------------

    @cached_property
    def _name_maps(self) -> tuple[dict[str, int], dict[int, str]]:
        registry_type_map: dict[str, int] = {}
        type_id_to_name: dict[int, str] = {}
        portable_registry = self._metadata.portable_registry
        types = [st.value for st in portable_registry.value_object["types"].value_object]
        type_by_id = {entry["id"]: entry for entry in types}

        for type_entry in types:
            type_id = type_entry["id"]
            type_def = type_entry["type"]["def"]
            type_path = type_entry["type"].get("path")
            if type_entry.get("params") or "variant" in type_def:
                continue
            if type_path:
                name = type_path[-1]
                registry_type_map[name] = type_id
                type_id_to_name[type_id] = name
            elif "primitive" in type_def:
                name = type_def["primitive"]
                registry_type_map[name] = type_id
                type_id_to_name[type_id] = name

        pending = set(type_by_id) - set(type_id_to_name)

        def resolve(type_id_: int) -> Optional[str]:
            entry = type_by_id[type_id_]
            type_def_ = entry["type"]["def"]
            type_path_ = entry["type"].get("path", [])
            type_params = entry["type"].get("params", [])
            if type_path_:
                base = type_path_[-1]
                if type_params:
                    inner = []
                    for param in type_params:
                        dep = param["type"]
                        if dep not in type_id_to_name:
                            return None
                        inner.append(type_id_to_name[dep])
                    return f"{base}<{', '.join(inner)}>"
                if "variant" in type_def_:
                    return None
                return base
            if "sequence" in type_def_:
                inner_name = type_id_to_name.get(type_def_["sequence"]["type"])
                return f"Vec<{inner_name}>" if inner_name else None
            if "array" in type_def_:
                inner_name = type_id_to_name.get(type_def_["array"]["type"])
                length = type_def_["array"].get("len")
                if inner_name:
                    return f"[{inner_name}; {length}]" if length else f"[{inner_name}]"
                return None
            if "compact" in type_def_:
                inner_name = type_id_to_name.get(type_def_["compact"]["type"])
                return f"Compact<{inner_name}>" if inner_name else None
            if "tuple" in type_def_:
                names = []
                for inner_id in type_def_["tuple"]:
                    if inner_id not in type_id_to_name:
                        return None
                    names.append(type_id_to_name[inner_id])
                return f"({', '.join(names)})"
            return None

        progressed = True
        while progressed and pending:
            progressed = False
            for type_id in list(pending):
                name = resolve(type_id)
                if name is not None:
                    type_id_to_name[type_id] = name
                    registry_type_map[name] = type_id
                    pending.discard(type_id)
                    progressed = True
        return registry_type_map, type_id_to_name

    @property
    def _registry_type_map(self) -> dict[str, int]:
        return self._name_maps[0]

    @property
    def _type_id_to_name(self) -> dict[int, str]:
        return self._name_maps[1]

    # -- metadata IR (for codegen) -----------------------------------------------------------------

    def metadata_ir(self) -> MetadataIR:
        def join_docs(docs: list) -> str:
            return " ".join(d.strip() for d in (docs or [])).strip()

        pallets: list[PalletIR] = []
        for pallet in self._metadata.pallets:
            calls = [
                CallIR(
                    name=call.name,
                    args=[arg["name"] for arg in call.value.get("fields", [])],
                    docs=join_docs(call.value.get("docs", [])),
                )
                for call in (pallet.calls or [])
            ]
            errors = [
                ErrorIR(index=error_index, name=error.name, docs=join_docs(error.docs))
                for error_index, error in enumerate(pallet.errors or [])
            ]
            # Skip pseudo-entries like `:__STORAGE_VERSION__:`.
            storage = [
                item.value["name"]
                for item in (pallet.storage or [])
                if ":" not in item.value["name"]
            ]
            constants = [c.value["name"] for c in (pallet.constants or [])]
            pallets.append(
                PalletIR(
                    name=pallet.name,
                    index=int(pallet.value["index"]),
                    calls=calls,
                    errors=errors,
                    storage=storage,
                    constants=constants,
                )
            )
        runtime_apis = [
            RuntimeApiIR(name=api, methods=list(methods))
            for api, methods in self.runtime_api_map.items()
        ]
        return MetadataIR(
            spec_version=self.spec_version, pallets=pallets, runtime_apis=runtime_apis
        )
