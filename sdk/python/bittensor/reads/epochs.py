"""Epoch and timing reads."""

from __future__ import annotations

import asyncio
from typing import Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from .base import read, scalar_read


@read(
    "next_epoch_start_block",
    {"netuid": "integer"},
    category="Epochs & timing",
    param_docs={"netuid": "Subnet to query."},
)
async def next_epoch_start_block(view, netuid: int) -> Optional[int]:
    """Block at which the subnet's next epoch is expected to fire, or None if tempo is 0.

    Chain-computed: `LastEpochBlock + tempo`, pulled earlier by any pending
    owner-triggered epoch. This is the source of truth — the epoch schedule is
    stateful, so the legacy client-side modular formula is wrong on subnets
    whose tempo changed or whose owner triggered an epoch.
    """
    value = await view.runtime(api.SubnetInfoRuntimeApi.get_next_epoch_start_block, [netuid])
    return None if value is None else int(value)


@read(
    "blocks_until_next_epoch",
    {"netuid": "integer"},
    category="Epochs & timing",
    param_docs={"netuid": "Subnet to query."},
)
async def blocks_until_next_epoch(view, netuid: int) -> Optional[int]:
    """Blocks until the subnet's next epoch fires, or None if tempo is 0.

    Derived from the chain's own `next_epoch_start_block` at one pinned block.
    The actual fire can shift by a few blocks (per-block epoch cap defers,
    owner triggers pull earlier) — to act on the epoch itself, use
    `client.wait_for_epoch()`.
    """
    view = await view.at()
    value = await view.runtime(api.SubnetInfoRuntimeApi.get_next_epoch_start_block, [netuid])
    return None if value is None else max(0, int(value) - view.block)


scalar_read(
    "blocks_since_last_step",
    st.SubtensorModule.BlocksSinceLastStep,
    per_netuid=True,
    doc="Blocks since the subnet's last epoch step ran.",
    category="Epochs & timing",
)


@read(
    "blocks_since_last_update",
    {"netuid": "integer", "uid": "integer"},
    category="Epochs & timing",
    param_docs={
        "netuid": "Subnet to query.",
        "uid": "UID of the neuron on that subnet.",
    },
)
async def blocks_since_last_update(view, netuid: int, uid: int) -> Optional[int]:
    """Blocks since a neuron last set or committed weights on a subnet
    (mechanism 0), or None.

    On commit-reveal subnets LastUpdate is bumped at commit time, not when
    the weights are revealed and applied. None means the subnet has no
    LastUpdate vector or the uid does not exist. The standard "is this
    validator still setting weights" health check.
    """
    view = await view.at()
    last = await view.query(st.SubtensorModule.LastUpdate, [netuid])
    if not last or uid >= len(last):
        return None
    return view.block - int(last[uid])


@read(
    "epoch_status",
    {"netuid": "integer"},
    category="Epochs & timing",
    param_docs={"netuid": "Subnet to query."},
)
async def epoch_status(view, netuid: int) -> dict:
    """Where a subnet is in its epoch cycle, in one block-pinned read.

    Bundles tempo, the last epoch run, the chain-computed next start block, and
    the remaining blocks/seconds (seconds use the chain's detected block time,
    so they are correct on fast-blocks chains too). `pending_epoch_at` is the
    block of an owner-triggered epoch, or None when none is pending.
    """
    view = await view.at()
    (
        tempo,
        last_epoch_block,
        since_step,
        pending_at,
        epoch_index,
        next_start,
        seconds_per_block,
    ) = await asyncio.gather(
        view.query(st.SubtensorModule.Tempo, [netuid]),
        view.query(st.SubtensorModule.LastEpochBlock, [netuid]),
        view.query(st.SubtensorModule.BlocksSinceLastStep, [netuid]),
        view.query(st.SubtensorModule.PendingEpochAt, [netuid]),
        view.query(st.SubtensorModule.SubnetEpochIndex, [netuid]),
        view.runtime(api.SubnetInfoRuntimeApi.get_next_epoch_start_block, [netuid]),
        view.block_time(),
    )
    remaining = None if next_start is None else max(0, int(next_start) - view.block)
    return {
        "netuid": netuid,
        "block": view.block,
        "tempo": int(tempo or 0),
        "epoch_index": int(epoch_index or 0),
        "last_epoch_block": int(last_epoch_block or 0),
        "blocks_since_last_step": int(since_step or 0),
        "pending_epoch_at": int(pending_at) if pending_at else None,
        "next_epoch_start_block": None if next_start is None else int(next_start),
        "blocks_remaining": remaining,
        "seconds_remaining": None if remaining is None else remaining * seconds_per_block,
    }
