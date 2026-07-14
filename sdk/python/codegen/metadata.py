"""Dump runtime metadata into the shared, language-neutral IR.

The IR itself is owned by the transport (``bittensor._transport.contract``) —
one definition, produced by the codec straight from decoded metadata — and is
deliberately small and JSON-serializable: it captures only what the emitters
currently use — pallet index, call names + parameter names, error names +
docs, and storage/constant/runtime-API names. Growing coverage (e.g. typed
call params) means growing that IR, not rewriting emitters.
"""

from __future__ import annotations

import asyncio

from bittensor._transport import SubstrateConnection
from bittensor._transport.contract import (
    CallArgIR,
    CallIR,
    ErrorIR,
    MetadataIR,
    PalletIR,
    RuntimeApiIR,
    StorageIR,
)
from bittensor.settings import SS58_FORMAT

__all__ = [
    "CallArgIR",
    "CallIR",
    "ErrorIR",
    "MetadataIR",
    "PalletIR",
    "RuntimeApiIR",
    "StorageIR",
    "dump",
    "dump_from_node",
]


async def dump_from_node(endpoint: str) -> MetadataIR:
    """Connect to a node and parse its current runtime metadata into the IR."""
    connection = SubstrateConnection(endpoint, ss58_format=SS58_FORMAT)
    await connection.initialize()
    try:
        return await connection.metadata_ir()
    finally:
        await connection.close()


def dump(endpoint: str) -> MetadataIR:
    return asyncio.run(dump_from_node(endpoint))
