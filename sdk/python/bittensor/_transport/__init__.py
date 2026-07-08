"""Private Substrate transport for the Bittensor SDK.

This package is internal: the SDK's supported surface is the `bittensor.Substrate`
contract (implemented over this transport by `bittensor.RpcSubstrate`) and the
domain namespaces on `bittensor.Client`. Nothing here is a public API.

Layering (each module knows only the ones below it):

    interface.py   SubstrateConnection — the facade the SDK consumes
    runtime.py     block -> RuntimeCodec resolution + caching
    storage.py     storage keys and value/map decoding
    extrinsics.py  signing payloads, nonce cache, outcome resolution
    runtime_api.py runtime API calls (modern V15 + legacy Bittensor registry)
    codec.py       the ONLY module importing scalecodec (cyscale)
    rpc.py         JSON-RPC websocket session (no SCALE knowledge)
"""

# ruff: noqa: E402

import importlib.metadata


def _check_conflicts():
    """
    Verifies that py-scale-codec (`scalecodec` on PyPI) is not installed, since it
    shares the `scalecodec` namespace with `cyscale`, which this transport requires.
    """
    try:
        _ = importlib.metadata.distribution("scalecodec")
        raise RuntimeError(
            "\n\n"
            "Conflict detected: 'scalecodec' (py-scale-codec) is installed.\n"
            "This conflicts with 'cyscale', which uses the same namespace.\n\n"
            "Please remove it first:\n"
            "    pip uninstall scalecodec cyscale -y\n\n"
            "Then reinstall cyscale:\n"
            "    pip install cyscale --force-reinstall\n"
        )
    except importlib.metadata.PackageNotFoundError:
        pass  # Good — scalecodec is not installed


_check_conflicts()


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
