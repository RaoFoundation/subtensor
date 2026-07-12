"""The decoded-value shape contract: the active codec must reproduce the corpus.

The corpus (tests/fixtures/shape_corpus/corpus.json, recorded by
scripts/record_shape_corpus.py) pins the exact plain-Python shape every
portable-registry type decodes to. It is the definition of done for any codec
replacement: byte inputs are fixed, decoded outputs must be *equal* — not
semantically equivalent. See sdk/bittensor-core-spec.md §4.1.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from tests.conftest import golden_codec

CORPUS = Path(__file__).parent.parent / "fixtures" / "shape_corpus" / "corpus.json"


def jsonable(value):
    """Same scrub the recorder used, so comparisons are apples-to-apples."""
    if isinstance(value, (bytes, bytearray)):
        return "0x" + bytes(value).hex()
    if isinstance(value, (list, tuple)):
        return [jsonable(v) for v in value]
    if isinstance(value, dict):
        return {str(k): jsonable(v) for k, v in value.items()}
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return repr(value)


def load_corpus() -> dict:
    return json.loads(CORPUS.read_text())


def test_corpus_exists_and_covers_registry():
    corpus = load_corpus()
    assert corpus["types_covered"] / corpus["types_total"] > 0.95


def test_decoded_shapes_match_corpus():
    corpus = load_corpus()
    codec = golden_codec()

    type_strings: list[str] = []
    datas: list[bytes] = []
    expected: list = []
    coords: list[tuple[int, int]] = []  # (type id, sample index) for failure messages
    for entry in corpus["types"]:
        for i, sample in enumerate(entry["samples"]):
            type_strings.append(f"scale_info::{entry['id']}")
            datas.append(bytes.fromhex(sample["scale_hex"][2:]))
            expected.append(sample["decoded"])
            coords.append((entry["id"], i))

    decoded = codec.batch_decode(type_strings, datas)
    assert len(decoded) == len(expected)

    mismatches = []
    for (type_id, i), got, want in zip(coords, (jsonable(v) for v in decoded), expected):
        if got != want:
            mismatches.append((type_id, i, want, got))
    if mismatches:
        preview = "\n".join(
            f"  type {tid} sample {i}: expected {want!r}, got {got!r}"
            for tid, i, want, got in mismatches[:10]
        )
        pytest.fail(
            f"{len(mismatches)}/{len(expected)} corpus samples decoded to a different "
            f"shape:\n{preview}"
        )
