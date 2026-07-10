"""Storage-value decoding and ``state_queryStorageAt`` change plumbing.

Substrate state is one big key-value store; key construction (prefix hashing,
parameter encoding, map-key recovery) lives in the Rust core behind
:class:`~.codec.RuntimeCodec`. This module keeps the two pieces that are
transport policy, not codec work:

- the Default-vs-Option miss semantics:
    - key present  -> decode raw bytes as the item's value type
    - key missing, modifier Default  -> decode the item's default bytes
    - key missing, modifier Optional -> decode as ``Option<T>`` (yields None)
- turning a page of raw ``state_queryStorageAt`` changes into decoded
  (key, value) pairs.
"""

from __future__ import annotations

from typing import Any, Optional

from .codec import RuntimeCodec, StorageEntry


def decode_storage_value(codec: RuntimeCodec, entry: StorageEntry, raw: Optional[bytes]) -> Any:
    """Decode one storage response, applying the Default/Option miss semantics."""
    if raw is not None:
        return codec.decode(entry.value_type, raw)
    return codec.decode(entry.decode_type(False), entry.default_bytes)


def decode_storage_values(
    codec: RuntimeCodec, entry: StorageEntry, raws: list[Optional[bytes]]
) -> list[Any]:
    """Bulk variant of :func:`decode_storage_value` on the batch-decode fast path."""
    type_strings = []
    datas = []
    for raw in raws:
        if raw is not None:
            type_strings.append(entry.value_type)
            datas.append(raw)
        else:
            type_strings.append(entry.decode_type(False))
            datas.append(entry.default_bytes)
    return codec.batch_decode(type_strings, datas)


def decode_map_pairs(
    codec: RuntimeCodec,
    entry: StorageEntry,
    fixed_params: list,
    changes: list[tuple[str, Optional[str]]],
    *,
    ignore_decoding_errors: bool = False,
) -> list[tuple[Any, Any]]:
    """Decode ``state_queryStorageAt`` changes for a map into (key, value) pairs.

    ``fixed_params`` are the map parameters that were fixed in the key prefix;
    the remaining ("free") key components are recovered from each storage key's
    trailing bytes. Multi-key maps yield tuple keys.
    """
    free_count = len(entry.param_types) - len(fixed_params)
    if free_count < 1:
        raise ValueError(f"{entry.pallet}.{entry.name} is not a map beyond the given params")

    if not ignore_decoding_errors:
        return codec.decode_map_changes(entry, changes, fixed=len(fixed_params))

    # Tolerant path: values in bulk, keys individually so a bad key becomes None.
    raw_keys = []
    raw_values = []
    for key_hex, value_hex in changes:
        if value_hex is None:
            # Key deleted between the key listing and the value fetch.
            continue
        raw_keys.append(bytes.fromhex(key_hex.removeprefix("0x")))
        raw_values.append(bytes.fromhex(value_hex.removeprefix("0x")))
    values = codec.batch_decode([entry.value_type] * len(raw_values), raw_values)
    pairs = []
    for raw_key, value in zip(raw_keys, values):
        try:
            parts = codec.decode_storage_key_params(entry, raw_key, fixed=len(fixed_params))
            key = tuple(parts) if free_count > 1 else parts[0]
        except (ValueError, IndexError, TypeError):
            key = None
        pairs.append((key, value))
    return pairs
