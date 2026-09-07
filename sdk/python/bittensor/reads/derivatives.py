"""Derivatives reads: open long/short positions and the pallet parameters."""

from __future__ import annotations

from typing import Any, Optional

from .._generated import storage as st
from ..balance import Balance
from .base import read

# Mirrors `BLOCKS_PER_DAY` in the pallet: the borrow fee is quoted per day and
# accrues per block on top of the day each add books up front.
_BLOCKS_PER_DAY = 7_200
_PERBILL = 1_000_000_000
_PERCENT = 100


def _variant(value: Any) -> str:
    """The variant name of a SCALE enum decoded as a string or a one-key dict."""
    if isinstance(value, dict):
        return str(next(iter(value)))
    return str(value)


def _fee_owed_rao(fee_accrued_rao: int, fee_per_day_rao: int, blocks_since_touch: int) -> int:
    """Mirrors the pallet's `Position::fee_owed`: the booked balance plus the rate since."""
    return fee_accrued_rao + fee_per_day_rao * max(0, blocks_since_touch) // _BLOCKS_PER_DAY


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


def _cushion_rao(cushion: Any) -> int:
    """The `Cushion` enum, `Tao(amount)` today, as TAO rao."""
    if isinstance(cushion, dict):
        return int(cushion.get(_variant(cushion)) or 0)
    return int(cushion or 0)


def _position_record(view, coldkey: str, netuid: int, raw: Any, now: int) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    cushion = _cushion_rao(raw.get("cushion"))
    exposure = int(raw.get("exposure_tao") or 0)
    fee_per_day = int(raw.get("fee_per_day") or 0)
    fee_accrued = int(raw.get("fee_accrued") or 0)
    last_touch = int(raw.get("last_touch") or 0)
    opened_at = int(raw.get("opened_at") or 0)
    expires_at = int(raw.get("expires_at") or 0)
    legs = _legs(view, raw.get("legs"), netuid)
    return {
        "coldkey": coldkey,
        "netuid": netuid,
        "side": legs["side"],
        "leverage": exposure / cushion if cushion else 0.0,
        "cushion": Balance.from_rao(cushion),
        "proceeds": legs["proceeds"],
        "debt": legs["debt"],
        "escrow": legs["escrow"],
        "exposure_tao": Balance.from_rao(exposure),
        "fee_per_day_tao": Balance.from_rao(fee_per_day),
        "opened_at": opened_at,
        "last_touch": last_touch,
        "expires_at": expires_at,
        "expired": now >= expires_at,
        "blocks_open": max(0, now - opened_at),
        "accrued_fee_tao": Balance.from_rao(
            _fee_owed_rao(fee_accrued, fee_per_day, now - last_touch)
        ),
    }


def _params_record(raw: Any) -> dict:
    raw = raw if isinstance(raw, dict) else {}
    return {
        "shorts_enabled": bool(raw.get("shorts_enabled", False)),
        "longs_enabled": bool(raw.get("longs_enabled", False)),
        "max_short_leverage_percent": int(raw.get("max_short_leverage_percent") or 0),
        "max_long_leverage_percent": int(raw.get("max_long_leverage_percent") or 0),
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
    """The derivatives pallet's root-set global parameters.

    `max_short_leverage_percent` and `max_long_leverage_percent` bound the
    leverage an owner may choose per side (`100` = 1x), `max_pool_share` caps how
    much of a pool's reserve may be lent per side, and `lifetime_blocks` is how
    long a position may stay open. Each tranche's fee is fixed when it is added:
    a short pays `short_fee_per_day_tao` times the share of the pool it lifted,
    a long pays `long_rate_per_day` times its TAO exposure; both are scaled by
    `1 / (1 - share)^4` for the tranche's own slippage. One day is booked at the
    add, the rest accrues per block and is paid at each settlement. A subnet may
    override the switches and the cap; see `derivatives_subnet_override`.
    """
    return _params_record(await view.query(st.Derivatives.Params))


def _override_record(raw: Any) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    share = raw.get("max_pool_share")
    return {
        "shorts_enabled": bool(raw.get("shorts_enabled", False)),
        "longs_enabled": bool(raw.get("longs_enabled", False)),
        "max_pool_share": None if share is None else int(share) / _PERCENT,
    }


@read(
    "derivatives_subnet_override",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet to look up."},
)
async def derivatives_subnet_override(view, netuid: int) -> Optional[dict]:
    """Root-set per-subnet overrides of the derivatives parameters, or None.

    None means the subnet runs on the global `derivatives_params`. When set,
    `shorts_enabled` and `longs_enabled` replace the global switches for adds
    on this subnet, and `max_pool_share` replaces the global cap when it is not
    None. Open positions are unaffected: a paused side can still be reduced and
    closed.
    """
    return _override_record(await view.query(st.Derivatives.SubnetOverrides, [netuid]))


@read(
    "derivative_position",
    {"coldkey_ss58": "string", "netuid": "integer"},
    category="Prices & swaps",
    param_docs={
        "coldkey_ss58": "Coldkey that owns the position.",
        "netuid": "Subnet the position is on.",
    },
)
async def derivative_position(view, coldkey_ss58: str, netuid: int) -> Optional[dict]:
    """A coldkey's open position on a subnet, or None. There is at most one.

    `side` is the direction of its net exposure. `cushion` is the TAO the owner
    has put up in total and `leverage` is `exposure_tao / cushion`, the blend
    of every tranche added. `proceeds`, `debt`, and `escrow` are the position's
    `legs`, each already in its own token: a short holds TAO proceeds and TAO
    escrow and owes alpha; a long holds alpha proceeds and alpha escrow and owes
    TAO. `fee_per_day_tao` is the summed rate of its tranches; `accrued_fee_tao`
    is what would be charged if settled now.
    """
    view = await view.at()
    raw = await view.query(st.Derivatives.Positions, [coldkey_ss58, netuid])
    return _position_record(view, coldkey_ss58, netuid, raw, view.block)


@read(
    "derivative_positions",
    {"coldkey_ss58": "string"},
    category="Prices & swaps",
    param_docs={"coldkey_ss58": "Coldkey whose positions to list."},
)
async def derivative_positions(view, coldkey_ss58: str) -> list[dict]:
    """Every open position a coldkey holds, one per subnet."""
    view = await view.at()
    rows = await view.query_map(st.Derivatives.Positions, [coldkey_ss58])
    records = []
    for key, raw in rows:
        # Remainder after the coldkey prefix: the netuid.
        netuid = key[0] if isinstance(key, (list, tuple)) else key
        record = _position_record(view, coldkey_ss58, int(netuid), raw, view.block)
        if record:
            records.append(record)
    records.sort(key=lambda r: r["netuid"])
    return records
