"""Derivatives reads: open long/short positions and the pallet parameters."""

from __future__ import annotations

from typing import Any, Optional

from .._generated import constants
from .._generated import storage as st
from ..balance import Balance
from .base import read

# Mirrors `BLOCKS_PER_YEAR` in the pallet: the interest is a yearly rate accrued per block.
_BLOCKS_PER_YEAR = 365 * 7_200
_PERCENT = 100


def _variant(value: Any) -> str:
    """The variant name of a SCALE enum decoded as a string or a one-key dict."""
    if isinstance(value, dict):
        return str(next(iter(value)))
    return str(value)


def _interest_due_rao(interest_owed: int, interest_per_year: int, blocks_since: int) -> int:
    """Mirrors the pallet's `Position::interest_due`: the carried balance plus the rate since."""
    return interest_owed + interest_per_year * max(0, blocks_since) // _BLOCKS_PER_YEAR


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


def _equity_rao(
    cushion: int, side: str, proceeds: int, debt: int, quote: int, interest: int
) -> int:
    """Mirrors `Position::equity`: what a full close would leave the owner, in TAO rao."""
    if side == "Short":
        return cushion + proceeds - quote - interest
    return cushion + quote - debt - interest


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


async def _reserves(view, netuid: int) -> tuple[int, int]:
    tao = await view.query(st.SubtensorModule.SubnetTAO, [netuid])
    alpha = await view.query(st.SubtensorModule.SubnetAlphaIn, [netuid])
    return int(tao or 0), int(alpha or 0)


def _position_record(
    view, coldkey: str, netuid: int, raw: Any, now: int, reserves: tuple[int, int]
) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    cushion = int(raw.get("cushion") or 0)
    exposure = int(raw.get("exposure_tao") or 0)
    interest_per_year = int(raw.get("interest_per_year") or 0)
    interest_owed = int(raw.get("interest_owed") or 0)
    since = int(raw.get("since") or 0)
    due = int(raw.get("due") or 0)
    legs = _legs(view, raw.get("legs"), netuid)
    side = legs["side"]
    interest_due = _interest_due_rao(interest_owed, interest_per_year, now - since)
    # What changes hands in alpha: a short buys its debt back, a long sells its proceeds.
    alpha_to_settle = legs["debt"].rao if side == "Short" else legs["proceeds"].rao
    quote = _settle_quote_rao(side, alpha_to_settle, *reserves)
    equity = _equity_rao(cushion, side, legs["proceeds"].rao, legs["debt"].rao, quote, interest_due)
    # How long the cushion keeps paying at this rate before the chain forfeits the position.
    left = cushion - interest_due
    runway_days = left / interest_per_year * 365 if interest_per_year else None
    return {
        "coldkey": coldkey,
        "netuid": netuid,
        "side": side,
        "leverage": exposure / cushion if cushion else 0.0,
        "cushion": Balance.from_rao(cushion),
        "proceeds": legs["proceeds"],
        "debt": legs["debt"],
        "escrow": legs["escrow"],
        "exposure_tao": Balance.from_rao(exposure),
        "interest_per_year_tao": Balance.from_rao(interest_per_year),
        "interest_due_tao": Balance.from_rao(interest_due),
        "since": since,
        "due": due,
        "runway_days": runway_days,
        "equity_tao": Balance.from_rao(equity),
    }


@read(
    "derivatives_params",
    {},
    category="Prices & swaps",
)
async def derivatives_params(view) -> dict:
    """The derivatives pallet's two root-set parameters, plus its constants.

    `pool_share` is the largest share of a pool's reserve that all open
    positions of one side may borrow together; zero means root has paused new
    positions. `interest_rate` is the interest, as a fraction of a tranche's TAO
    exposure per year, the same for both sides, fixed when the tranche is
    added and accrued per block; once a week, on the position's own block, the
    chain takes it from the cushion, buys alpha with it, and recycles the alpha.
    Both are fractions (`0.25` = 25%).

    The rest are fixed by the runtime: `max_short_leverage` and
    `max_long_leverage` bound the leverage an owner may choose per side (`1.0`
    = 1x), and `min_deposit_tao` is the smallest deposit one add may put up.
    """
    view = await view.at()
    raw = await view.query(st.Derivatives.Params)
    raw = raw if isinstance(raw, dict) else {}
    return {
        "pool_share": int(raw.get("pool_share") or 0) / _PERCENT,
        "interest_rate": int(raw.get("interest_rate") or 0) / _PERCENT,
        "max_short_leverage": int(await view.constant(constants.Derivatives.MaxShortLeverage))
        / _PERCENT,
        "max_long_leverage": int(await view.constant(constants.Derivatives.MaxLongLeverage))
        / _PERCENT,
        "min_deposit_tao": Balance.from_rao(
            int(await view.constant(constants.Derivatives.MinDeposit))
        ),
    }


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
    has put up and `leverage` is `exposure_tao` over it, the blend of every
    tranche added. `proceeds`, `debt`, and `escrow` are the position's `legs`,
    each already in its own token: a short holds TAO proceeds and TAO escrow
    and owes alpha; a long holds alpha proceeds and alpha escrow and owes TAO.
    `interest_per_year_tao` is the summed interest of its tranches;
    `interest_due_tao` is what has accrued since the chain last collected, at
    block `since`; `due` is the block it collects next, one week after the last
    time. `runway_days` is how long the cushion keeps paying at this rate; at a
    collection it cannot pay, the chain forfeits the position to the pool and
    the owner gets nothing. Add cushion to extend it. There is no expiry, and
    only the owner can close.

    `equity_tao` is an estimate of what a close now would pay the owner:
    cushion plus proceeds, less debt priced on a constant-product curve, less
    the interest due. The chain's own quote decides; this is a preview.
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
    """Every open position on a subnet, whoever owns it, largest exposure first.
    Same fields as `derivative_position`."""
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
    records.sort(key=lambda r: -r["exposure_tao"].rao)
    return records
