"""Shared golden-fixture access for the offline transport tests.

The fixture is a corpus recorded from a live localnet (see record_golden.py);
``golden_codec()`` rebuilds the codec purely from the raw metadata bytes it
embeds, so every consumer decodes against the exact runtime the vectors were
recorded under — no node needed.
"""

from __future__ import annotations

import json
from functools import lru_cache
from pathlib import Path

from bittensor._transport.codec import RuntimeCodec, strip_option_opaque_metadata

GOLDEN_FIXTURE = Path(__file__).parent / "fixtures" / "golden.json"


@lru_cache(maxsize=1)
def golden() -> dict:
    return json.loads(GOLDEN_FIXTURE.read_text())


@lru_cache(maxsize=1)
def golden_codec() -> RuntimeCodec:
    g = golden()
    inner = strip_option_opaque_metadata(bytes.fromhex(g["metadata"]["v15_hex"][2:]))
    assert inner is not None
    return RuntimeCodec(
        inner,
        spec_version=g["network"]["spec_version"],
        transaction_version=g["network"]["transaction_version"],
        spec_name="node-subtensor",
        ss58_format=g["network"]["ss58_format"],
    )
