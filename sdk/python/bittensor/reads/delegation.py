"""Delegation reads: delegates, takes, nominations, and child hotkeys."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..hyperparams import ratio_fraction
from .base import read


def _take_fraction(raw: int, item: Optional[st.Item] = None) -> float:
    """A take value as a 0..1 fraction, keyed on type identity.

    When the storage descriptor's metadata carries a ratio identity
    (post-newtype runtimes: PerU16), that identity decides the denominator;
    otherwise fall back to the pre-newtype reading — a bare u16 over 65535,
    which encodes identically.
    """
    if item is not None:
        fraction = ratio_fraction(getattr(item, "value_type_ident", None), raw)
        if fraction is not None:
            return fraction
    return ratio_fraction("PerU16", raw) or 0.0


@dataclass
class DelegateInfo:
    """A delegate hotkey: who owns it, its take, and where it is registered."""

    hotkey: str
    owner: str
    take: float  # fraction of emissions kept by the delegate, 0..1
    nominators: int
    registrations: list[int]
    validator_permits: list[int]
    return_per_1000: Balance
    total_daily_return: Balance


@dataclass
class DelegatedStake:
    """One nomination: stake a coldkey has delegated to a hotkey on a subnet."""

    delegate: DelegateInfo
    netuid: int
    stake: Balance


def _delegate_info(d: dict) -> DelegateInfo:
    return DelegateInfo(
        hotkey=str(d["delegate_ss58"]),
        owner=str(d["owner_ss58"]),
        take=_take_fraction(int(d["take"])),
        nominators=len(d.get("nominators") or []),
        registrations=[int(n) for n in d.get("registrations") or []],
        validator_permits=[int(n) for n in d.get("validator_permits") or []],
        return_per_1000=Balance.from_rao(int(d.get("return_per_1000") or 0)),
        total_daily_return=Balance.from_rao(int(d.get("total_daily_return") or 0)),
    )


@read("delegates", {}, category="Delegation")
async def delegates(view) -> list[DelegateInfo]:
    """Every delegate hotkey on the network, with take and registrations."""
    records = await view.runtime(api.DelegateInfoRuntimeApi.get_delegates, [])
    return [_delegate_info(d) for d in records or []]


@read(
    "delegate",
    {"hotkey_ss58": "string"},
    category="Delegation",
    param_docs={"hotkey_ss58": "Delegate hotkey to look up."},
)
async def delegate(view, hotkey_ss58: str) -> Optional[DelegateInfo]:
    """Delegate info for one hotkey, or None if it is not a delegate."""
    record = await view.runtime(api.DelegateInfoRuntimeApi.get_delegate, [hotkey_ss58])
    return _delegate_info(record) if record else None


@read(
    "is_delegate",
    {"hotkey_ss58": "string"},
    category="Delegation",
    param_docs={"hotkey_ss58": "Hotkey to check for delegate status."},
)
async def is_delegate(view, hotkey_ss58: str) -> bool:
    """Whether a hotkey is a delegate."""
    record = await view.runtime(api.DelegateInfoRuntimeApi.get_delegate, [hotkey_ss58])
    return record is not None


@read(
    "delegate_take",
    {"hotkey_ss58": "string"},
    category="Delegation",
    param_docs={"hotkey_ss58": "Delegate hotkey whose take to read."},
)
async def delegate_take(view, hotkey_ss58: str) -> dict:
    """A hotkey's delegate take (emission fraction it keeps) with the allowed min/max.

    `take`, `min`, and `max` are fractions in 0..1; `take_u16` is the raw
    on-chain u16 value.
    """
    raw, lo, hi = await asyncio.gather(
        view.query(st.SubtensorModule.Delegates, [hotkey_ss58]),
        view.query(st.SubtensorModule.MinDelegateTake),
        view.query(st.SubtensorModule.MaxDelegateTake),
    )
    return {
        "hotkey": hotkey_ss58,
        "take": _take_fraction(int(raw or 0), st.SubtensorModule.Delegates),
        "take_u16": int(raw or 0),
        "min": _take_fraction(int(lo or 0), st.SubtensorModule.MinDelegateTake),
        "max": _take_fraction(int(hi or 0), st.SubtensorModule.MaxDelegateTake),
    }


@read(
    "delegated",
    {"coldkey_ss58": "string"},
    category="Delegation",
    param_docs={"coldkey_ss58": "Coldkey whose nominations to list."},
)
async def delegated(view, coldkey_ss58: str) -> list[DelegatedStake]:
    """Every nomination a coldkey holds: (delegate, netuid, stake) per position.

    Each `stake` is denominated in the subnet's own currency: subnet alpha, or
    TAO when netuid is 0.
    """
    records = await view.runtime(api.DelegateInfoRuntimeApi.get_delegated, [coldkey_ss58])
    return [
        DelegatedStake(
            delegate=_delegate_info(info),
            netuid=int(netuid),
            stake=view.balance(int(stake), int(netuid)),
        )
        for info, (netuid, stake) in records or []
    ]


@read(
    "children",
    {"hotkey_ss58": "string", "netuid": "integer"},
    category="Delegation",
    param_docs={
        "hotkey_ss58": "Parent hotkey whose children to list.",
        "netuid": "Subnet to query.",
    },
)
async def children(view, hotkey_ss58: str, netuid: int) -> list[tuple[int, str]]:
    """Child hotkeys of a parent on a subnet, as (proportion, child_ss58) pairs.

    Proportions are u64-normalized fractions of the parent's stake, where
    u64::MAX means 100%.
    """
    entries = await view.query(st.SubtensorModule.ChildKeys, [hotkey_ss58, netuid])
    return [(int(prop), str(child)) for prop, child in entries or []]


@read(
    "parents",
    {"hotkey_ss58": "string", "netuid": "integer"},
    category="Delegation",
    param_docs={
        "hotkey_ss58": "Child hotkey whose parents to list.",
        "netuid": "Subnet to query.",
    },
)
async def parents(view, hotkey_ss58: str, netuid: int) -> list[tuple[int, str]]:
    """Parent hotkeys of a child on a subnet, as (proportion, parent_ss58) pairs.

    Proportions are u64-normalized fractions of the parent's stake, where
    u64::MAX means 100%.
    """
    entries = await view.query(st.SubtensorModule.ParentKeys, [hotkey_ss58, netuid])
    return [(int(prop), str(parent)) for prop, parent in entries or []]


@read(
    "pending_children",
    {"hotkey_ss58": "string", "netuid": "integer"},
    category="Delegation",
    param_docs={
        "hotkey_ss58": "Parent hotkey whose pending children to list.",
        "netuid": "Subnet to query.",
    },
)
async def pending_children(view, hotkey_ss58: str, netuid: int) -> dict:
    """Proposed child hotkeys of a parent still in cooldown, and when they apply.

    `set_children` normally does not take effect immediately: the proposal is
    parked here until `cooldown_block`, then promoted to the finalized set
    that the `children` read returns. On subnets whose subtoken is not yet
    enabled the cooldown is skipped and children apply immediately, so
    nothing lingers here. `children` is (proportion, child_ss58) pairs with
    u64-normalized proportions, matching the `children` read.
    """
    value = await view.query(st.SubtensorModule.PendingChildKeys, [netuid, hotkey_ss58])
    entries, cooldown_block = value or ([], 0)
    return {
        "children": [(int(prop), str(child)) for prop, child in entries or []],
        "cooldown_block": int(cooldown_block),
    }
