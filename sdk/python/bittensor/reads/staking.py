"""Staking reads: positions, valuations, and per-coldkey stake settings."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from .base import read
from .prices import alpha_prices


@dataclass
class StakePosition:
    """One (coldkey, hotkey, netuid) stake position.

    ``stake`` is denominated in the subnet's own currency: subnet alpha, or TAO
    when ``netuid`` is 0. It is NOT a TAO value — use ``stake_value_for_coldkey``
    to mark alpha positions to TAO.
    """

    hotkey: str
    coldkey: str
    netuid: int
    stake: Balance
    is_registered: bool


@dataclass
class StakeValuation:
    """A coldkey's stake positions marked to TAO at spot prices, at one block.

    ``stake_value`` is alpha × spot price summed across positions, in TAO. It is
    an estimate: it excludes the slippage and fees a real unstake would incur.
    The exact on-chain quantities are the per-position alpha amounts in
    ``positions``; ``tao_per_alpha`` is the spot price per netuid at ``block``.
    """

    coldkey: str
    block: int
    positions: list[StakePosition]
    stake_value: Balance  # TAO (netuid 0), spot valuation
    tao_per_alpha: dict[int, float]

    def spot_value(self, stake: Balance) -> Balance:
        """Spot TAO value of one stake balance (exact when it is already TAO)."""
        if stake.netuid == 0:
            return Balance.from_rao(stake.rao)
        return Balance.from_rao(int(stake.rao * self.tao_per_alpha.get(stake.netuid, 0.0)))


def _stake_position(view, r: dict) -> StakePosition:
    return StakePosition(
        hotkey=str(r["hotkey"]),
        coldkey=str(r["coldkey"]),
        netuid=int(r["netuid"]),
        stake=view.balance(int(r["stake"]), int(r["netuid"])),
        is_registered=bool(r["is_registered"]),
    )


@read(
    "stake",
    {"coldkey_ss58": "string", "hotkey_ss58": "string", "netuid": "integer"},
    category="Staking",
    param_docs={
        "coldkey_ss58": "Coldkey that owns the stake.",
        "hotkey_ss58": "Hotkey the stake is held on.",
        "netuid": "Subnet to query.",
    },
)
async def stake(view, coldkey_ss58: str, hotkey_ss58: str, netuid: int) -> Balance:
    """Alpha staked by a coldkey to a hotkey on a subnet (TAO when netuid is 0)."""
    info = await view.runtime(
        api.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
        [hotkey_ss58, coldkey_ss58, netuid],
    )
    return view.balance(0 if info is None else int(info["stake"]), netuid)


@read(
    "stake_for_coldkey",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose stake positions to list."},
)
async def stake_for_coldkey(view, coldkey_ss58: str) -> list[StakePosition]:
    """Every stake position held by a coldkey, across all hotkeys and subnets.

    Each position's `stake` is denominated in the subnet's own currency: subnet
    alpha, or TAO when netuid is 0. Use `stake_value_for_coldkey` to mark alpha
    positions to TAO.
    """
    records = await view.runtime(api.StakeInfoRuntimeApi.get_stake_info_for_coldkey, [coldkey_ss58])
    return [_stake_position(view, r) for r in records or []]


@read(
    "stake_for_coldkeys",
    {"coldkey_ss58s": "array"},
    category="Staking",
    param_docs={"coldkey_ss58s": "Coldkeys whose stake positions to list."},
)
async def stake_for_coldkeys(view, coldkey_ss58s: list[str]) -> dict[str, list[StakePosition]]:
    """Every stake position for several coldkeys at once, in one runtime call.

    Each position's `stake` is denominated in the subnet's own currency: subnet
    alpha, or TAO when netuid is 0.
    """
    records = await view.runtime(
        api.StakeInfoRuntimeApi.get_stake_info_for_coldkeys, [coldkey_ss58s]
    )
    return {
        str(coldkey): [_stake_position(view, r) for r in positions or []]
        for coldkey, positions in records or []
    }


def _spot_value(positions: list[StakePosition], tao_per_alpha: dict[int, float]) -> Balance:
    """Mark alpha positions to TAO at spot prices; netuid-0 stake is already TAO."""
    value_rao = 0
    for position in positions:
        if position.netuid == 0:
            value_rao += position.stake.rao
        else:
            value_rao += int(position.stake.rao * tao_per_alpha.get(position.netuid, 0.0))
    return Balance.from_rao(value_rao)


@read(
    "stake_value_for_coldkey",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose stake to value."},
)
async def stake_value_for_coldkey(view, coldkey_ss58: str) -> StakeValuation:
    """A coldkey's stake marked to TAO at spot prices (excludes slippage/fees), block-pinned."""
    view = await view.at()
    records, tao_per_alpha = await asyncio.gather(
        view.runtime(api.StakeInfoRuntimeApi.get_stake_info_for_coldkey, [coldkey_ss58]),
        alpha_prices(view),
    )
    positions = [_stake_position(view, r) for r in records or []]
    return StakeValuation(
        coldkey=coldkey_ss58,
        block=view.block,
        positions=positions,
        stake_value=_spot_value(positions, tao_per_alpha),
        tao_per_alpha=tao_per_alpha,
    )


@read(
    "stake_value_for_coldkeys",
    {"coldkey_ss58s": "array"},
    category="Staking",
    param_docs={"coldkey_ss58s": "Coldkeys whose stake to value."},
)
async def stake_value_for_coldkeys(view, coldkey_ss58s: list[str]) -> dict[str, StakeValuation]:
    """Spot-price stake valuation for several coldkeys at once, at one block."""
    view = await view.at()
    records, tao_per_alpha = await asyncio.gather(
        view.runtime(api.StakeInfoRuntimeApi.get_stake_info_for_coldkeys, [coldkey_ss58s]),
        alpha_prices(view),
    )
    by_coldkey = {
        str(coldkey): [_stake_position(view, r) for r in positions or []]
        for coldkey, positions in records or []
    }
    return {
        ss58: StakeValuation(
            coldkey=ss58,
            block=view.block,
            positions=by_coldkey.get(ss58, []),
            stake_value=_spot_value(by_coldkey.get(ss58, []), tao_per_alpha),
            tao_per_alpha=tao_per_alpha,
        )
        for ss58 in coldkey_ss58s
    }


@read(
    "stake_availability",
    {"coldkey_ss58": "string", "netuid": "integer"},
    category="Staking",
    param_docs={
        "coldkey_ss58": "Coldkey whose free/locked stake to read.",
        "netuid": "Subnet to query.",
    },
)
async def stake_availability(view, coldkey_ss58: str, netuid: int) -> dict:
    """Free vs locked stake for a coldkey on one subnet.

    ``locked`` is conviction-locked mass (plus any miner collateral reserved
    against the coldkey on that subnet). ``available`` is what can still be
    unstaked or transferred without moving lock mass. Both are denominated in
    the subnet's own currency (TAO on netuid 0).
    """
    raw = await view.runtime(
        api.StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
        [[coldkey_ss58], [netuid]],
    )
    entry = ((raw or {}).get(coldkey_ss58) or {}).get(netuid) or {}
    # Some decoders string-key the inner map.
    if not entry:
        entry = ((raw or {}).get(coldkey_ss58) or {}).get(str(netuid)) or {}
    return {
        "netuid": netuid,
        "total": view.balance(int(entry.get("total") or 0), netuid),
        "locked": view.balance(int(entry.get("locked") or 0), netuid),
        "available": view.balance(int(entry.get("available") or 0), netuid),
    }


@read(
    "stake_availability_for_coldkey",
    {"coldkey_ss58": "string", "netuids": "array"},
    category="Staking",
    param_docs={
        "coldkey_ss58": "Coldkey whose free/locked stake to read.",
        "netuids": "Subnets to query (empty list returns no rows).",
    },
)
async def stake_availability_for_coldkey(view, coldkey_ss58: str, netuids: list[int]) -> list[dict]:
    """Free vs locked stake for a coldkey across many subnets (one runtime call)."""
    if not netuids:
        return []
    ordered = sorted({int(n) for n in netuids})
    raw = await view.runtime(
        api.StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
        [[coldkey_ss58], ordered],
    )
    per_netuid = (raw or {}).get(coldkey_ss58) or {}
    rows: list[dict] = []
    for netuid in ordered:
        entry = per_netuid.get(netuid) or per_netuid.get(str(netuid)) or {}
        rows.append(
            {
                "netuid": netuid,
                "total": view.balance(int(entry.get("total") or 0), netuid),
                "locked": view.balance(int(entry.get("locked") or 0), netuid),
                "available": view.balance(int(entry.get("available") or 0), netuid),
            }
        )
    return rows


@read(
    "staking_hotkeys",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose staked-to hotkeys to list."},
)
async def staking_hotkeys(view, coldkey_ss58: str) -> list[str]:
    """Every hotkey a coldkey has stake with, owned or nominated.

    Unlike `owned_hotkeys` this includes delegates the coldkey has nominated.
    Use `stake_for_coldkey` for the per-subnet amounts behind each entry.
    """
    value = await view.query(st.SubtensorModule.StakingHotkeys, [coldkey_ss58])
    return [str(hotkey) for hotkey in value or []]


@read(
    "auto_stake",
    {"coldkey_ss58": "string", "netuid": "integer"},
    category="Staking",
    param_docs={
        "coldkey_ss58": "Coldkey whose auto-stake destination to read.",
        "netuid": "Subnet to query.",
    },
)
async def auto_stake(view, coldkey_ss58: str, netuid: int) -> Optional[str]:
    """The hotkey a coldkey auto-stakes its rewards to on a subnet, or None if unset."""
    value = await view.query(st.SubtensorModule.AutoStakeDestination, [coldkey_ss58, netuid])
    return str(value) if value else None


@read(
    "auto_stake_all",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose auto-stake destinations to list."},
)
async def auto_stake_all(view, coldkey_ss58: str) -> list[dict]:
    """Every auto-stake destination configured for a coldkey.

    One entry per subnet where a destination hotkey is set; subnets with no
    entry have auto-stake unset.
    """
    # Scope the double-map to this coldkey (same pattern as locks_for_coldkey).
    rows = await view.query_map(st.SubtensorModule.AutoStakeDestination, [coldkey_ss58])
    return sorted(
        ({"netuid": int(netuid), "hotkey": str(hotkey)} for netuid, hotkey in rows),
        key=lambda row: row["netuid"],
    )
