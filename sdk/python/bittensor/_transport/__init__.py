"""Private Substrate transport for the Bittensor SDK.

This package is internal: the SDK's supported surface is the `bittensor.Substrate`
contract (implemented over this transport by `bittensor.RpcSubstrate`) and the
domain namespaces on `bittensor.Client`. Nothing here is a public API.

Layering (each module knows only the ones below it):

    interface.py   SubstrateConnection — the facade the SDK consumes
    runtime.py     block -> RuntimeCodec resolution + caching
    storage.py     storage value/map decoding (miss semantics, change plumbing)
    extrinsics.py  signing payloads, nonce cache, outcome resolution
    runtime_api.py runtime API calls (modern V15 + frozen legacy definitions)
    codec.py       the SCALE codec seam over the Rust core (bittensor_core)
    rpc.py         JSON-RPC websocket session (no SCALE knowledge)
"""

from .contract import (
    BlockData,
    InclusionReport,
    MetadataIR,
    MultisigAccount,
    SignedExtrinsic,
    SigningContext,
    UnsignedExtrinsic,
)
from .interface import QueryMapResult, SubstrateConnection

__all__ = [
    "BlockData",
    "InclusionReport",
    "MetadataIR",
    "MultisigAccount",
    "QueryMapResult",
    "SignedExtrinsic",
    "SigningContext",
    "SubstrateConnection",
    "UnsignedExtrinsic",
]
