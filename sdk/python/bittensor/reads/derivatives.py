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


def _settle_quote_rao(side: str, alpha_rao: int, tao_reserve: int, alpha_reserve: int) -> int:
    """The TAO side of closing now, on a constant-product pool.

    A short buys `alpha_rao` back: `T * q / (A - q)`. A long sells it:
    `T * q / (A + q)`. The chain's Balancer quote differs from this by its
    weights and rounding, so treat the result as an estimate; the chain decides.
    """
    if side == "Short":
        if alpha_rao >= alpha_reserve:
            return 2**63
        return tao_reserve * alpha_rao // (alpha_reserve - alpha_rao) + 1
    return tao_reserve * alpha_rao // (alpha_reserve + alpha_rao)


def _equity_rao(cushion: int, side: str, proceeds: int, debt: int, quote: int, fee: int) -> int:
    """Mirrors `Position::equity`: what a full close would leave the owner, in TAO rao.

    `cushion` is the cushion's TAO value: its TAO plus what its alpha would sell for.
    """
    if side == "Short":
        return cushion + proceeds - quote - fee
    return cushion + quote - debt - fee


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


def _cushion(view, cushion: Any, netuid: int) -> dict:
    """The `Cushion` struct: TAO, alpha, and the hotkey the alpha goes back to."""
    cushion = cushion if isinstance(cushion, dict) else {}
    hotkey = cushion.get("alpha_hotkey")
    return {
        "tao": Balance.from_rao(int(cushion.get("tao") or 0)),
        "alpha": view.balance(int(cushion.get("alpha") or 0), netuid),
        "alpha_hotkey": None if hotkey is None else str(hotkey),
    }


async def _reserves(view, netuid: int) -> tuple[int, int]:
    tao = await view.query(st.SubtensorModule.SubnetTAO, [netuid])
    alpha = await view.query(st.SubtensorModule.SubnetAlphaIn, [netuid])
    return int(tao or 0), int(alpha or 0)


def _position_record(
    view, coldkey: str, netuid: int, raw: Any, now: int, reserves: tuple[int, int]
) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    cushion = _cushion(view, raw.get("cushion"), netuid)
    exposure = int(raw.get("exposure_tao") or 0)
    fee_per_day = int(raw.get("fee_per_day") or 0)
    fee_accrued = int(raw.get("fee_accrued") or 0)
    last_touch = int(raw.get("last_touch") or 0)
    opened_at = int(raw.get("opened_at") or 0)
    expires_at = int(raw.get("expires_at") or 0)
    legs = _legs(view, raw.get("legs"), netuid)
    side = legs["side"]
    fee_owed = _fee_owed_rao(fee_accrued, fee_per_day, now - last_touch)
    # What changes hands in alpha: a short buys its debt back, a long sells its proceeds.
    alpha_to_settle = legs["debt"].rao if side == "Short" else legs["proceeds"].rao
    quote = _settle_quote_rao(side, alpha_to_settle, *reserves)
    # An alpha cushion is worth what it would sell for now.
    cushion_alpha_tao = (
        _settle_quote_rao("Long", cushion["alpha"].rao, *reserves) if cushion["alpha"].rao else 0
    )
    cushion_value = cushion["tao"].rao + cushion_alpha_tao
    equity = _equity_rao(
        cushion_value, side, legs["proceeds"].rao, legs["debt"].rao, quote, fee_owed
    )
    return {
        "coldkey": coldkey,
        "netuid": netuid,
        "side": side,
        "leverage": exposure / cushion_value if cushion_value else 0.0,
        "cushion": cushion["tao"],
        "cushion_alpha": cushion["alpha"],
        "cushion_alpha_hotkey": cushion["alpha_hotkey"],
        "cushion_value_tao": Balance.from_rao(cushion_value),
        "proceeds": legs["proceeds"],
        "debt": legs["debt"],
        "escrow": legs["escrow"],
        "exposure_tao": Balance.from_rao(exposure),
        "fee_per_day_tao": Balance.from_rao(fee_per_day),
        "opened_at": opened_at,
        "expires_at": expires_at,
        "last_touch": last_touch,
        "blocks_open": max(0, now - opened_at),
        "blocks_left": max(0, expires_at - now),
        "expired": now >= expires_at,
        "accrued_fee_tao": Balance.from_rao(fee_owed),
        "equity_tao": Balance.from_rao(equity),
        "healthy": equity >= fee_per_day,
    }


def _params_record(raw: Any) -> dict:
    raw = raw if isinstance(raw, dict) else {}
    return {
        "shorts_enabled": bool(raw.get("shorts_enabled", False)),
        "longs_enabled": bool(raw.get("longs_enabled", False)),
        "max_short_leverage_percent": int(raw.get("max_short_leverage_percent") or 0),
        "max_long_leverage_percent": int(raw.get("max_long_leverage_percent") or 0),
        "alpha_cushion_shorts": bool(raw.get("alpha_cushion_shorts", False)),
        "alpha_cushion_longs": bool(raw.get("alpha_cushion_longs", False)),
        "max_pool_share": int(raw.get("max_pool_share") or 0) / _PERCENT,
        "rate_per_day": int(raw.get("rate_per_day") or 0) / _PERBILL,
        "lifetime_blocks": int(raw.get("lifetime_blocks") or 0),
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
    leverage an owner may choose per side (`100` = 1x), and `max_pool_share`
    caps how much of a pool's reserve may be lent per side.
    `alpha_cushion_shorts` and `alpha_cushion_longs` say whether that side
    accepts an alpha cushion; TAO is always accepted. The fee is one rate for
    both sides, `rate_per_day` of a tranche's TAO exposure, fixed when the
    tranche is added: one day is booked at the add, the rest accrues per block
    and is paid at each settlement. A position expires `lifetime_blocks` after
    its first add; after that, or once its equity drops below one day of fee,
    anyone may close it. A subnet may override the switches, the cap, and the
    rate; see `derivatives_subnet_override`.
    """
    return _params_record(await view.query(st.Derivatives.Params))


def _override_record(raw: Any) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    share = raw.get("max_pool_share")
    rate = raw.get("rate_per_day")
    return {
        "shorts_enabled": bool(raw.get("shorts_enabled", False)),
        "longs_enabled": bool(raw.get("longs_enabled", False)),
        "max_pool_share": None if share is None else int(share) / _PERCENT,
        "rate_per_day": None if rate is None else int(rate) / _PERBILL,
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
    on this subnet, and `max_pool_share` and `rate_per_day` replace the global
    cap and fee rate when they are not None. Open positions are unaffected: a
    paused side can still be reduced and closed.
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
    has put up and `cushion_alpha` the alpha, returned to `cushion_alpha_hotkey`;
    `cushion_value_tao` prices both in TAO and `leverage` is `exposure_tao`
    over that, the blend of every tranche added. `proceeds`, `debt`, and
    `escrow` are the position's `legs`, each already in its own token: a short
    holds TAO proceeds and TAO escrow and owes alpha; a long holds alpha
    proceeds and alpha escrow and owes TAO. `fee_per_day_tao` is the summed
    rate of its tranches; `accrued_fee_tao` is what would be charged if settled
    now. `expires_at` is set by the first add and does not move; `expired` is
    whether that block has passed, after which anyone may close the position
    for one day of fee, and an owner's same-side add rolls it.

    `equity_tao` is an estimate of what a close now would pay the owner:
    cushion value plus proceeds, less debt priced on a constant-product curve,
    less the fee owed. Negative means underwater. `healthy` is whether that
    equity still covers one more day of fee; when it does not, anyone may close
    the position with `close_derivative` and is paid the fee for it. The
    chain's own quote decides; this is a preview.
    """
    view = await view.at()
    raw = await view.query(st.Derivatives.Positions, [coldkey_ss58, netuid])
    if not isinstance(raw, dict):
        return None
    reserves = await _reserves(view, netuid)
    return _position_record(view, coldkey_ss58, netuid, raw, view.block, reserves)


@read(
    "derivative_positions",
    {"coldkey_ss58": "string"},
    category="Prices & swaps",
    param_docs={"coldkey_ss58": "Coldkey whose positions to list."},
)
async def derivative_positions(view, coldkey_ss58: str) -> list[dict]:
    """Every open position a coldkey holds, one per subnet. Same fields as
    `derivative_position`."""
    view = await view.at()
    rows = await view.query_map(st.Derivatives.Positions, [coldkey_ss58])
    records = []
    for key, raw in rows:
        # Remainder after the coldkey prefix: the netuid.
        netuid = int(key[0] if isinstance(key, (list, tuple)) else key)
        if not isinstance(raw, dict):
            continue
        reserves = await _reserves(view, netuid)
        record = _position_record(view, coldkey_ss58, netuid, raw, view.block, reserves)
        if record:
            records.append(record)
    records.sort(key=lambda r: r["netuid"])
    return records


@read(
    "derivative_positions_on_subnet",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet whose open positions to list."},
)
async def derivative_positions_on_subnet(view, netuid: int) -> list[dict]:
    """Every open position on a subnet, whoever owns it. Same fields as
    `derivative_position`.

    The list a liquidator works from: filter on `healthy` being False or
    `expired` being True and call `close_derivative` with that `coldkey` as
    `owner_ss58`.
    """
    view = await view.at()
    owners = await view.query_map(st.Derivatives.OpenByNetuid, [netuid])
    reserves = await _reserves(view, netuid)
    records = []
    for key, _ in owners:
        coldkey = str(key[0] if isinstance(key, (list, tuple)) else key)
        raw = await view.query(st.Derivatives.Positions, [coldkey, netuid])
        record = _position_record(view, coldkey, netuid, raw, view.block, reserves)
        if record:
            records.append(record)
    records.sort(key=lambda r: r["equity_tao"].rao)
    return records
