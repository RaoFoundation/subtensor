"""Chain-level reads: time, block metadata, global limits."""

from __future__ import annotations

from typing import Optional

from .._generated import storage as st
from .base import read, scalar_read


@read("timestamp", {}, category="Chain")
async def timestamp(view) -> dict:
    """Current chain time (from the Timestamp pallet) and the block it was read at."""
    view = await view.at()
    stamp = await view.timestamp()
    return {"block": view.block, "unix": stamp.timestamp(), "iso": stamp.isoformat()}


@read("block_time", {}, category="Chain")
async def block_time(view) -> float:
    """Seconds per block, detected from the chain (12.0 mainnet, 0.25 fast-blocks)."""
    return await view.block_time()


@read("is_fast_blocks", {}, category="Chain")
async def is_fast_blocks(view) -> bool:
    """Whether the chain runs fast blocks (0.25s slots, local/e2e testing mode)."""
    return await view.is_fast_blocks()


@read(
    "block_info",
    {"block": "integer"},
    category="Chain",
    param_docs={"block": "Block number to describe."},
)
async def block_info(view, block: int) -> Optional[dict]:
    """A block's hash, timestamp, and extrinsics (as module.function summaries).

    Programmatic callers wanting the fully decoded extrinsics and raw header
    should use `client.block_info()` instead.
    """
    info = await view.block_info(block)
    if info is None:
        return None
    calls = []
    for value in info.extrinsics:
        call = value.get("call") if isinstance(value, dict) else None
        if isinstance(call, dict):
            calls.append(f"{call.get('call_module')}.{call.get('call_function')}")
        else:
            calls.append(None)  # undecodable entry
    return {
        "number": info.number,
        "hash": info.hash,
        "timestamp": info.timestamp.isoformat(),
        "extrinsics": calls,
        "explorer_url": info.explorer_url,
    }


@read("mev_shield_next_key", {}, category="Chain")
async def mev_shield_next_key(view) -> Optional[str]:
    """The MEV Shield ML-KEM-768 public key (0x-hex) used to encrypt shielded txs, or None."""
    value = await view.query(st.MevShield.NextKey)
    if not value:
        return None
    return value if isinstance(value, str) else "0x" + bytes(value).hex()


scalar_read(
    "tx_rate_limit",
    st.SubtensorModule.TxRateLimit,
    per_netuid=False,
    doc="Global transaction rate limit in blocks.",
    category="Chain",
)
