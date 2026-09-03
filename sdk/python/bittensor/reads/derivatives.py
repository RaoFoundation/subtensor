"""Derivatives reads: open long/short positions and the pallet parameters."""

from __future__ import annotations

from typing import Any, Optional

from .._generated import storage as st
from ..balance import Balance
from .base import read

# Mirrors `BLOCKS_PER_DAY` in the pallet: the borrow fee is quoted per day and
# never charged for less than one day.
_BLOCKS_PER_DAY = 7_200
_PERBILL = 1_000_000_000
_PERCENT = 100


def _variant(value: Any) -> str:
    """The variant name of a SCALE enum decoded as a string or a one-key dict."""
    if isinstance(value, dict):
        return str(next(iter(value)))
    return str(value)


def _accrued_fee_rao(fee_per_day_rao: int, blocks_open: int) -> int:
    """Mirrors the pallet's `accrued_fee`: per-day fee times days open, one-day minimum."""
    return fee_per_day_rao * max(blocks_open, _BLOCKS_PER_DAY) // _BLOCKS_PER_DAY


def _legs(view, legs: Any, netuid: int) -> dict:
    """The `Legs` enum: its variant is the side, and each leg is typed by it.

    `Short { proceeds: TAO, debt: alpha, escrow: TAO }`,
    `Long { proceeds: alpha, debt: TAO, escrow: alpha }`.
    """
    variant = _variant(legs)
    inner = legs.get(variant) if isinstance(legs, dict) else {}
    inner = inner if isinstance(inner, dict) else {}
    proceeds = int(inner.get("proceeds") or 0)
    debt = int(inner.get("debt") or 0)
    escrow = int(inner.get("escrow") or 0)
    if variant == "Short":
        return {
            "side": "Short",
            "proceeds": Balance.from_rao(proceeds),
            "debt": view.balance(debt, netuid),
            "escrow": Balance.from_rao(escrow),
        }
    return {
        "side": "Long",
        "proceeds": view.balance(proceeds, netuid),
        "debt": Balance.from_rao(debt),
        "escrow": view.balance(escrow, netuid),
    }


def _position_record(
    view, coldkey: str, netuid: int, side: str, raw: Any, now: int
) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    exposure = int(raw.get("exposure_tao") or 0)
    fee_per_day = int(raw.get("fee_per_day") or 0)
    opened_at = int(raw.get("opened_at") or 0)
    expires_at = int(raw.get("expires_at") or 0)
    blocks_open = max(0, now - opened_at)
    legs = _legs(view, raw.get("legs"), netuid)
    return {
        "coldkey": coldkey,
        "netuid": netuid,
        "side": side,
        "cushion": Balance.from_rao(int(raw.get("cushion") or 0)),
        "proceeds": legs["proceeds"],
        "debt": legs["debt"],
        "escrow": legs["escrow"],
        "exposure_tao": Balance.from_rao(exposure),
        "fee_per_day_tao": Balance.from_rao(fee_per_day),
        "opened_at": opened_at,
        "expires_at": expires_at,
        "expired": now >= expires_at,
        "blocks_open": blocks_open,
        "accrued_fee_tao": Balance.from_rao(_accrued_fee_rao(fee_per_day, blocks_open)),
    }


def _params_record(raw: Any) -> dict:
    raw = raw if isinstance(raw, dict) else {}
    return {
        "shorts_enabled": bool(raw.get("shorts_enabled", False)),
        "longs_enabled": bool(raw.get("longs_enabled", False)),
        "leverage_percent": int(raw.get("leverage_percent") or 0),
        "max_pool_share": int(raw.get("max_pool_share") or 0) / _PERCENT,
        "lifetime_blocks": int(raw.get("lifetime_blocks") or 0),
        "short_fee_per_day_tao": Balance.from_rao(int(raw.get("short_fee_per_day") or 0)),
        "long_rate_per_day": int(raw.get("long_rate_per_day") or 0) / _PERBILL,
        "min_deposit_tao": Balance.from_rao(int(raw.get("min_deposit_tao") or 0)),
    }


@read(
    "derivatives_params",
    {},
    category="Prices & swaps",
)
async def derivatives_params(view) -> dict:
    """The derivatives pallet's root-set parameters.

    `leverage_percent` sizes the borrowed slice from the cushion, `max_pool_share`
    caps how much of a pool's reserve may be lent per side, and `lifetime_blocks`
    is how long a position may stay open. Fees are fixed at open and charged at
    close with a one-day minimum: a short pays `short_fee_per_day_tao` times the
    share of the pool it lifted, a long pays `long_rate_per_day` times its TAO
    exposure.
    """
    return _params_record(await view.query(st.Derivatives.Params))


@read(
    "derivative_position",
    {"coldkey_ss58": "string", "netuid": "integer", "side": "string"},
    category="Prices & swaps",
    param_docs={
        "coldkey_ss58": "Coldkey that owns the position.",
        "netuid": "Subnet the position is on.",
        "side": "`Short` or `Long`.",
    },
)
async def derivative_position(view, coldkey_ss58: str, netuid: int, side: str) -> Optional[dict]:
    """One open position for a coldkey on a subnet and side, or None.

    `cushion` is the TAO the owner put up. `proceeds`, `debt`, and `escrow` are
    the position's `legs`, each already in
    its own token: a short holds TAO proceeds and TAO escrow and owes alpha; a
    long holds alpha proceeds and alpha escrow and owes TAO. `fee_per_day_tao`
    was fixed at open; `accrued_fee_tao` is what would be charged if closed now.
    """
    view = await view.at()
    raw = await view.query(st.Derivatives.Positions, [coldkey_ss58, (netuid, side)])
    return _position_record(view, coldkey_ss58, netuid, side, raw, view.block)


@read(
    "derivative_positions",
    {"coldkey_ss58": "string"},
    category="Prices & swaps",
    param_docs={"coldkey_ss58": "Coldkey whose positions to list."},
)
async def derivative_positions(view, coldkey_ss58: str) -> list[dict]:
    """Every open long and short a coldkey holds, across all subnets."""
    view = await view.at()
    rows = await view.query_map(st.Derivatives.Positions, [coldkey_ss58])
    records = []
    for key, raw in rows:
        # Remainder after the coldkey prefix: the (netuid, side) tuple.
        inner = key[0] if isinstance(key, (list, tuple)) and len(key) == 1 else key
        if not isinstance(inner, (list, tuple)) or len(inner) != 2:
            continue
        netuid, side = int(inner[0]), _variant(inner[1])
        record = _position_record(view, coldkey_ss58, netuid, side, raw, view.block)
        if record:
            records.append(record)
    records.sort(key=lambda r: (r["netuid"], r["side"]))
    return records
