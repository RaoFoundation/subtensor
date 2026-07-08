"""Storage keys and storage-value decoding.

Substrate state is one big key-value store; a storage item's key is
``twox128(pallet_prefix) ++ twox128(item_name) ++ hashed(param)...``. This
module owns that construction (including the batched fast path that reuses the
prefix and encoders across thousands of keys) and the one place where the
Default-vs-Option miss semantics live:

- key present  -> decode raw bytes as the item's value type
- key missing, modifier Default  -> decode the item's default bytes
- key missing, modifier Optional -> decode as ``Option<T>`` (yields None)
"""

from __future__ import annotations

from hashlib import blake2b
from typing import Any, Optional

import xxhash

from .codec import RuntimeCodec, StorageEntry


def _blake2_256(data: bytes) -> bytes:
    return blake2b(data, digest_size=32).digest()


def _blake2_128(data: bytes) -> bytes:
    return blake2b(data, digest_size=16).digest()


def _blake2_128_concat(data: bytes) -> bytes:
    return blake2b(data, digest_size=16).digest() + data


def _xxh64_reversed(data: bytes, seed: int) -> bytes:
    digest = bytearray(xxhash.xxh64(data, seed=seed).digest())
    digest.reverse()
    return bytes(digest)


def twox128(data: bytes) -> bytes:
    return _xxh64_reversed(data, 0) + _xxh64_reversed(data, 1)


def _twox64_concat(data: bytes) -> bytes:
    return _xxh64_reversed(data, 0) + data


HASHERS = {
    "Blake2_256": _blake2_256,
    "Blake2_128": _blake2_128,
    "Blake2_128Concat": _blake2_128_concat,
    "Twox128": twox128,
    "Twox64Concat": _twox64_concat,
    "Identity": lambda data: data,
}

# How many hash bytes precede the raw key material for reversible hashers —
# what makes map keys decodable straight from the storage key.
CONCAT_HASH_LENGTHS = {"Blake2_128Concat": 16, "Twox64Concat": 8, "Identity": 0}


def storage_prefix(entry: StorageEntry) -> bytes:
    return twox128(entry.prefix.encode()) + twox128(entry.name.encode())


def _hasher_for(entry: StorageEntry, index: int):
    if index >= len(entry.param_hashers):
        raise ValueError(
            f"{entry.pallet}.{entry.name} metadata declares no hasher for param #{index + 1}"
        )
    name = entry.param_hashers[index]
    try:
        return HASHERS[name or "Twox128"]
    except KeyError:
        raise ValueError(f'Unknown storage hasher "{name}"') from None


def storage_key(codec: RuntimeCodec, entry: StorageEntry, params: list) -> bytes:
    """The full storage key for one item (params may be a partial prefix)."""
    if len(params) > len(entry.param_types):
        raise ValueError(
            f"Storage function {entry.pallet}.{entry.name} accepts at most "
            f"{len(entry.param_types)} parameters, {len(params)} given"
        )
    key = storage_prefix(entry)
    for index, param in enumerate(params):
        encoded = codec.encode_storage_param(entry.param_types[index], param)
        key += _hasher_for(entry, index)(encoded)
    return key


def storage_key_batch(
    codec: RuntimeCodec, entry: StorageEntry, params_list: list[list]
) -> list[bytes]:
    """Keys for many parameter sets of one item; prefix and hashers resolve once."""
    prefix = storage_prefix(entry)
    hashers = [_hasher_for(entry, index) for index in range(len(entry.param_types))]
    keys = []
    for params in params_list:
        key = prefix
        for index, param in enumerate(params):
            encoded = codec.encode_storage_param(entry.param_types[index], param)
            key += hashers[index](encoded)
        keys.append(key)
    return keys


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
    prefix_hex: str,
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

    first_free = len(fixed_params)
    if free_count == 1:
        # Single free key: skip the hash prefix; decode the raw key directly.
        hash_len = _concat_len(entry, first_free)
        key_type = entry.param_types[first_free]
    else:
        # Multiple free keys: decode as a tuple interleaved with hash paddings.
        parts = []
        for index in range(first_free, len(entry.param_types)):
            parts.append(f"[u8; {_concat_len(entry, index)}]")
            parts.append(entry.param_types[index])
        key_type = f"({', '.join(parts)})"
        hash_len = None

    key_types = [key_type] * len(changes)
    value_types = [entry.value_type] * len(changes)
    raw_keys = []
    raw_values = []
    for key_hex, value_hex in changes:
        raw_key = bytes.fromhex(key_hex[len(prefix_hex) :])
        raw_keys.append(raw_key[hash_len:] if hash_len else raw_key)
        raw_values.append(
            bytes.fromhex(value_hex.removeprefix("0x")) if value_hex is not None else b""
        )

    decoded = codec.batch_decode(key_types + value_types, raw_keys + raw_values)
    middle = len(decoded) // 2
    pairs = []
    for key, value in zip(decoded[:middle], decoded[middle:]):
        if free_count > 1:
            try:
                # Strip the [u8; N] hash paddings: keep the odd tuple slots.
                key = tuple(key[i * 2 + 1] for i in range(free_count))
            except (IndexError, TypeError):
                if not ignore_decoding_errors:
                    raise
                key = None
        pairs.append((key, value))
    return pairs


def _concat_len(entry: StorageEntry, index: int) -> int:
    if index >= len(entry.param_hashers):
        raise ValueError(
            f"{entry.pallet}.{entry.name} metadata declares no hasher for param #{index + 1}"
        )
    hasher = entry.param_hashers[index]
    try:
        return CONCAT_HASH_LENGTHS[hasher]
    except KeyError:
        raise ValueError(
            f"Cannot recover map keys hashed with {hasher!r} "
            f"({entry.pallet}.{entry.name} param {index})"
        ) from None
