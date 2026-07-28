"""Miner registration-collateral reads."""

from __future__ import annotations

import asyncio
from typing import Any, Optional

# TODO(codegen): switch to `st.SubtensorModule.MinerCollateral` /
# `CollateralLockShare` / `CollateralDrainRatio` once the storage registry is
# regenerated against spec >= 435.
from .._generated import storage as st
from .._generated.storage import Item
from .base import read

_MINER_COLLATERAL = Item("SubtensorModule", "MinerCollateral", "MinerCollateralState")
_COLLATERAL_LOCK_SHARE = Item("SubtensorModule", "CollateralLockShare", "u16")
_COLLATERAL_DRAIN_RATIO = Item("SubtensorModule", "CollateralDrainRatio", "U64F64")

_U16_MAX = 65535


def _fixed_to_float(value: Any) -> float:
    """Decode a U64F64 fixed-point value (``{'bits': ...}`` or raw int)."""
    if isinstance(value, dict):
        return int(value.get("bits") or 0) / 2**64
    return int(value or 0) / 2**64


def _collateral_record(view, netuid: int, hotkey: str, coldkey: str, state: Any) -> Optional[dict]:
    if not isinstance(state, dict):
        return None
    locked = int(state.get("locked") or 0)
    min_locked = int(state.get("min_locked") or 0)
    earned = int(state.get("earned") or 0)
    drain_ratio = _fixed_to_float(state.get("drain_ratio"))
    # Derived terms: headroom is the portion actively draining (locked above
    # the floor); shortfall is capture in progress (floor above the lock);
    # releasable work is the incentive still needed to release the headroom.
    headroom = max(locked - min_locked, 0)
    shortfall = max(min_locked - locked, 0)
    return {
        "hotkey": hotkey,
        "coldkey": coldkey,
        "netuid": netuid,
        "locked_alpha": view.balance(locked, netuid),
        "min_locked_alpha": view.balance(min_locked, netuid),
        "earned_alpha": view.balance(earned, netuid),
        "drain_ratio": drain_ratio,
        "headroom_alpha": view.balance(headroom, netuid),
        "shortfall_alpha": view.balance(shortfall, netuid),
        "releasable_work_alpha": view.balance(
            int(headroom / drain_ratio) if drain_ratio > 0 else 0, netuid
        ),
    }


@read(
    "miner_collateral",
    {"netuid": "integer", "hotkey_ss58": "string", "coldkey_ss58": "string"},
    category="Mining & collateral",
    param_docs={
        "netuid": "Subnet to query.",
        "hotkey_ss58": "Miner hotkey of the bonded stake position.",
        "coldkey_ss58": "Coldkey of the bonded stake position. Defaults to the hotkey owner.",
    },
)
async def miner_collateral(
    view, netuid: int, hotkey_ss58: str, coldkey_ss58: str | None = None
) -> Optional[dict]:
    """A `(hotkey, coldkey)` stake position's standing collateral, or None.

    Collateral is keyed by hotkey + coldkey so nominators on the same hotkey are
    never charged for the owner's bond. When `coldkey_ss58` is omitted, the
    hotkey owner is used.

    `locked_alpha` is non-withdrawable stake released through earned emission
    (miner incentive and validator dividends) at `drain_ratio` alpha per alpha
    earned; `min_locked_alpha` is the miner-set floor the lock self-maintains
    around (the drain stops at it and emission fills any shortfall);
    `earned_alpha` is lifetime emission since the collateral entry existed.
    Derived: `headroom_alpha` (draining portion above the floor),
    `shortfall_alpha` (capture in progress below the floor), and
    `releasable_work_alpha` (emission still needed to release the headroom).
    The lock survives deregistration and is credited on re-registration.
    """
    coldkey = coldkey_ss58
    if coldkey is None:
        owner = await view.query(st.SubtensorModule.Owner, [hotkey_ss58])
        if owner is None:
            return None
        coldkey = str(owner)
    state = await view.query(_MINER_COLLATERAL, [netuid, hotkey_ss58, coldkey])
    return _collateral_record(view, netuid, hotkey_ss58, coldkey, state)


@read(
    "subnet_collateral",
    {"netuid": "integer"},
    category="Mining & collateral",
    param_docs={"netuid": "Subnet to query."},
)
async def subnet_collateral(view, netuid: int) -> list[dict]:
    """Every `(hotkey, coldkey)` position with standing collateral on a subnet.

    The list validator code reads to enforce a per-machine collateral
    requirement: each record carries the locked amount, the miner's
    self-maintained floor, the drain-ratio snapshot, and the hotkey's current
    `uid` (None when the hotkey is deregistered but its collateral persists) —
    ready to join against the metagraph when scoring.
    """
    view = await view.at()
    rows = await view.query_map(_MINER_COLLATERAL, [netuid])
    records = []
    for key, state in rows:
        # NMap remainder after the netuid prefix: (hotkey, coldkey).
        if isinstance(key, (list, tuple)) and len(key) >= 2:
            hotkey, coldkey = str(key[0]), str(key[1])
        else:
            continue
        record = _collateral_record(view, netuid, hotkey, coldkey, state)
        if record:
            records.append(record)
    uids = await view.query_batch(
        st.SubtensorModule.Uids, [[netuid, entry["hotkey"]] for entry in records]
    )
    for entry, uid in zip(records, uids):
        entry["uid"] = int(uid) if uid is not None else None
    records.sort(key=lambda entry: -entry["locked_alpha"].rao)
    return records


@read(
    "collateral_policy",
    {"netuid": "integer"},
    category="Mining & collateral",
    param_docs={"netuid": "Subnet to query."},
)
async def collateral_policy(view, netuid: int) -> dict:
    """A subnet's collateral configuration.

    `lock_share` is the fraction of the registration price locked as
    collateral instead of burned (0 disables collateral); `drain_ratio` is
    the alpha released per alpha of hotkey emission earned, snapshot per
    miner at registration.
    """
    view = await view.at()
    lock_share, drain_ratio = await asyncio.gather(
        view.query(_COLLATERAL_LOCK_SHARE, [netuid]),
        view.query(_COLLATERAL_DRAIN_RATIO, [netuid]),
    )
    ratio = _fixed_to_float(drain_ratio) if drain_ratio is not None else 1.0
    return {
        "netuid": netuid,
        "lock_share": int(lock_share or 0) / _U16_MAX,
        "drain_ratio": ratio,
    }
