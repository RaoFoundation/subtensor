"""Storage golden tests: keys and map decoding must match the old transport's
recorded output, byte for byte, offline."""

from __future__ import annotations

import pytest

from bittensor._transport.storage import (
    decode_map_pairs,
    decode_storage_value,
    decode_storage_values,
)
from tests.conftest import GOLDEN_FIXTURE, golden
from tests.conftest import golden_codec as codec

pytestmark = pytest.mark.skipif(not GOLDEN_FIXTURE.exists(), reason="golden fixture not recorded")


def _params(case: dict) -> list:
    # jsonable() turned tuples into lists; storage params are always flat lists.
    return list(case["params"])


def test_storage_keys_byte_identical():
    c = codec()
    for case in golden()["storage_keys"]:
        entry = c.storage_entry(case["pallet"], case["storage_function"])
        key = c.storage_key(entry, _params(case))
        assert "0x" + key.hex() == case["key_hex"], (
            f"{case['pallet']}.{case['storage_function']}({case['params']}) key diverged"
        )


def test_storage_key_batch_matches_singles():
    c = codec()
    g = golden()
    account_cases = [k for k in g["storage_keys"] if k["storage_function"] == "Account"]
    entry = c.storage_entry("System", "Account")
    params_list = [_params(case) for case in account_cases]
    batch = c.storage_key_batch(entry, params_list)
    singles = [c.storage_key(entry, params) for params in params_list]
    assert batch == singles


def test_storage_value_miss_semantics():
    c = codec()
    for case in golden()["storage_values"]:
        entry = c.storage_entry(case["pallet"], case["storage_function"])
        raw = bytes.fromhex(case["raw_hex"][2:]) if case["raw_hex"] is not None else None
        assert decode_storage_value(c, entry, raw) == case["decoded"]


def test_storage_values_bulk_matches_singles():
    c = codec()
    cases = golden()["storage_values"]
    by_item: dict[tuple, list] = {}
    for case in cases:
        by_item.setdefault((case["pallet"], case["storage_function"]), []).append(case)
    for (pallet, fn), group in by_item.items():
        entry = c.storage_entry(pallet, fn)
        raws = [
            bytes.fromhex(case["raw_hex"][2:]) if case["raw_hex"] is not None else None
            for case in group
        ]
        bulk = decode_storage_values(c, entry, raws)
        singles = [decode_storage_value(c, entry, raw) for raw in raws]
        assert bulk == singles


def _jsonable(value):
    if isinstance(value, (bytes, bytearray)):
        return "0x" + bytes(value).hex()
    if isinstance(value, (list, tuple)):
        return [_jsonable(v) for v in value]
    if isinstance(value, dict):
        return {str(k): _jsonable(v) for k, v in value.items()}
    return value


def test_query_map_pairs_match_old_decoding():
    c = codec()
    for case in golden()["query_maps"]:
        if not case["raw_changes"]:
            continue
        entry = c.storage_entry(case["pallet"], case["storage_function"])
        pairs = decode_map_pairs(
            c,
            entry,
            _params(case),
            [(k, v) for k, v in case["raw_changes"]],
        )
        got = [[_jsonable(k), _jsonable(v)] for k, v in pairs]
        expected = case["pairs"]
        assert got[: len(expected)] == expected, (
            f"{case['pallet']}.{case['storage_function']} map decoding diverged"
        )
