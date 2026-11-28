"""Lending reads: parameters, positions, a subnet's market, and short quotes."""

from __future__ import annotations

from typing import Any, Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..result import BittensorError, chain_error_from_dispatch
from .base import read

_SIDES = {"short": "Short", "long": "Long"}

# Denominators of the market's two pool ratios (`Perquintill`, and a fee in parts of `u16::MAX`).
_PERQUINTILL = 10**18
_FEE_DENOMINATOR = 65_535


def _price(value: Any) -> float:
    """A U64F64 price (``{'bits': ...}`` or raw int) in TAO per alpha."""
    bits = int(value.get("bits") or 0) if isinstance(value, dict) else int(value or 0)
    return bits / 2**64


def _side(side: str) -> str:
    try:
        return _SIDES[side.lower()]
    except KeyError:
        raise BittensorError(f"side must be 'short' or 'long', got {side!r}") from None


def _position_record(view, netuid: int, position: dict) -> dict:
    """A stored position: its collateral and deposit in TAO, and its leg's own amounts."""
    (side, leg), *_ = position["leg"].items()
    accrual = position["accrual"]
    record = {
        "netuid": int(netuid),
        "side": side.lower(),
        "collateral": view.balance(int(position["collateral"]), 0),
        "deposit": view.balance(int(position["deposit"]), 0),
        "interest_due": view.balance(int(accrual["due"]), 0),
        "opened_at": int(position["opened_at"]),
        "last_accrued": int(accrual["last_accrued"]),
        "charge_at": int(position["charge_at"]),
    }
    if side == "Short":
        record["minted_alpha"] = view.balance(int(leg["minted"]), netuid)
        record["proceeds"] = view.balance(int(leg["proceeds"]), 0)
    else:
        record["locked_alpha"] = view.balance(int(leg["locked_alpha"]), netuid)
        record["debt"] = view.balance(int(leg["debt"]), 0)
    return record


def _grow_quote(view, quote: Optional[dict], netuid: int) -> dict:
    quote = _ok(quote)
    return {
        "netuid": netuid,
        "proceeds": view.balance(int(quote["proceeds"]), 0),
        "interest": view.balance(int(quote["interest"]), 0),
        "deposit": view.balance(int(quote["deposit"]), 0),
        "coverage_need": view.balance(int(quote["coverage_need"]), 0),
        "min_collateral": view.balance(int(quote["min_collateral"]), 0),
        "spot_after": _price(quote["spot_after"]),
    }


def _dissolving(view, netuid: int, record: Optional[dict]) -> Optional[dict]:
    """A subnet's dissolution record while it is being dissolved, else None."""
    if record is None:
        return None
    return {
        "pool_tao_at_flag": view.balance(int(record["t_flag"]), 0),
        "lending_alpha": view.balance(int(record["lending_alpha"]), netuid),
        "settled_tao": view.balance(int(record["settled_tao"]), 0),
    }


def _ok(quote: Optional[dict]) -> dict:
    """A quote's value; raises the chain error the quoted call would fail with."""
    if quote is None:
        raise BittensorError("this chain has no lending runtime API")
    if "Err" in quote:
        raise chain_error_from_dispatch(quote["Err"])
    return quote["Ok"]


@read("lending_params", {}, category="Lending")
async def lending_params(view) -> dict:
    """The lending switches and the four live parameter groups.

    `enabled` and `longs_enabled` gate opening and increasing shorts and longs.
    `lending`, `short`, `long` and `dereg` are the parameter groups as the chain
    stores them: rates and bands in basis points, windows in blocks or days, and
    the `lending` amounts (`min_pool_tao`, `min_notional`, `deposit`) in rao.
    `lending.apr_bps`, `rate_k` and `rate_cap_bps` are the interest rate: a
    position pays `min(rate_cap_bps, apr_bps * max(1, u^rate_k))` a year on its
    notional, with `u` its debt over its holdings at the slow price, so never
    less than `apr_bps`. A group changed by its setter is live from the block
    after the change, except a tightening (a higher rate term, a change of
    `rate_k` or of the TWAP window, or a shorter `lending.rate_delay_blocks`),
    which waits the delay in force at the call: `pending_lending_params` is then
    the waiting group and `effective_at`, the block it goes live, and `None`
    when nothing waits.
    """
    params = await view.runtime(api.LendingRuntimeApi.params, [])
    if params is None:
        raise BittensorError("this chain has no lending runtime API")
    record = {"enabled": bool(params["enabled"]), "longs_enabled": bool(params["longs_enabled"])}
    for group in ("lending", "short", "long", "dereg"):
        record[group] = {name: int(value) for name, value in params[group].items()}
    waiting = await view.query(st.Lending.PendingLendingParams)
    record["pending_lending_params"] = waiting and {
        **{name: int(value) for name, value in waiting[0].items()},
        "effective_at": int(waiting[1]),
    }
    return record


@read(
    "lending_position",
    {"coldkey_ss58": "string", "netuid": "integer", "side": "string"},
    category="Lending",
    param_docs={
        "coldkey_ss58": "Coldkey holding the position.",
        "netuid": "Subnet of the position.",
        "side": "`short` or `long`.",
    },
)
async def lending_position(view, coldkey_ss58: str, netuid: int, side: str) -> Optional[dict]:
    """The position a coldkey holds on one side of a subnet, or None.

    Its `netuid`, `side`, `collateral`, refundable `deposit` and `interest_due`
    (accrued, not yet charged to the collateral) in TAO, and the blocks it was
    opened (`opened_at`), last accrued interest (`last_accrued`) and is next
    charged (`charge_at`). A short also has `minted_alpha`, the alpha owed back,
    and `proceeds`, the TAO its sale paid into custody. A long has
    `locked_alpha` and `debt`.
    """
    position = await view.runtime(
        api.LendingRuntimeApi.position, [coldkey_ss58, netuid, _side(side)]
    )
    return _position_record(view, netuid, position) if position else None


@read(
    "lending_positions",
    {"coldkey_ss58": "string"},
    category="Lending",
    param_docs={"coldkey_ss58": "Coldkey holding the positions."},
)
async def lending_positions(view, coldkey_ss58: str) -> list[dict]:
    """Every lending position of a coldkey, on every subnet (see `lending_position`)."""
    held = await view.runtime(api.LendingRuntimeApi.positions_of, [coldkey_ss58])
    return [_position_record(view, int(row["netuid"]), row["position"]) for row in held or []]


@read(
    "lending_market",
    {"netuid": "integer"},
    category="Lending",
    param_docs={"netuid": "Subnet of the market."},
)
async def lending_market(view, netuid: int) -> Optional[dict]:
    """A subnet's lending market, or None for a subnet without price rings.

    The pool as lending prices it (`pool_tao`, `pool_alpha`, the TAO reserve's
    `quote_weight` and the `swap_fee`, both fractions), the books
    (`synthetic_outstanding`: alpha minted by shorts and not yet bought back;
    `residual_alpha`: alpha minted by shorts that defaulted, which stays issued
    with no position behind it; `long_locked_alpha` and `long_lifted_tao`), and
    the three price references in TAO per alpha: the live `spot`, the `twap`
    median and the `sma` of daily means.

    `dissolving` is None unless the subnet is being dissolved. Then it holds the
    pool TAO at the flag (`pool_tao_at_flag`), the lending alpha frozen at the flag
    (`lending_alpha`) and the TAO its shorts have settled so far (`settled_tao`,
    paid to the stakers). The record is removed once the owner refund has read it.
    """
    market = await view.runtime(api.LendingRuntimeApi.market, [netuid])
    if not market:
        return None
    residual = await view.query(st.Lending.ResidualAlpha, [netuid])
    dissolving = await view.query(st.Lending.DissolvingSubnets, [netuid])
    return {
        "dissolving": _dissolving(view, netuid, dissolving),
        "netuid": netuid,
        "pool_tao": view.balance(int(market["pool_tao"]), 0),
        "pool_alpha": view.balance(int(market["pool_alpha"]), netuid),
        "quote_weight": int(market["quote_weight"]) / _PERQUINTILL,
        "swap_fee": int(market["swap_fee"]) / _FEE_DENOMINATOR,
        "synthetic_outstanding": view.balance(int(market["synthetic_outstanding"]), netuid),
        "residual_alpha": view.balance(int(residual or 0), netuid),
        "long_locked_alpha": view.balance(int(market["long_locked_alpha"]), netuid),
        "long_lifted_tao": view.balance(int(market["long_lifted_tao"]), 0),
        "spot": _price(market["spot"]),
        "twap": _price(market["twap"]),
        "sma": _price(market["sma"]),
    }


@read(
    "quote_open_short",
    {"netuid": "integer", "amount_alpha": "number"},
    category="Lending",
    param_docs={"netuid": "Subnet to short.", "amount_alpha": "Alpha minted and sold."},
)
async def quote_open_short(view, netuid: int, amount_alpha: float) -> dict:
    """What `open_short` of an amount would do now.

    The sale's `proceeds`, the refundable `deposit` taken on top of the
    collateral, the position's `coverage_need` and the least collateral that
    meets it (`min_collateral`), all in TAO, and the pool's `spot_after` the
    sale. Raises the chain error that stops the open, such as `Disabled`,
    `PoolTooShallow` or `OpenImpactTooHigh`; the caller's balance is not checked.
    """
    rao = Balance.from_amount(amount_alpha, netuid).rao
    quote = await view.runtime(api.LendingRuntimeApi.quote_open_short, [netuid, rao])
    return _grow_quote(view, quote, netuid)


@read(
    "quote_increase_short",
    {"coldkey_ss58": "string", "netuid": "integer", "amount_alpha": "number"},
    category="Lending",
    param_docs={
        "coldkey_ss58": "Coldkey holding the short.",
        "netuid": "Subnet of the short.",
        "amount_alpha": "Alpha minted and sold on top of the short.",
    },
)
async def quote_increase_short(view, coldkey_ss58: str, netuid: int, amount_alpha: float) -> dict:
    """What `increase_short` of an amount on a coldkey's short would do now.

    As `quote_open_short`, with the `interest` the call settles from the
    position's collateral first and no deposit. `min_collateral` is the least
    collateral the call itself must bring for the larger position.
    """
    rao = Balance.from_amount(amount_alpha, netuid).rao
    quote = await view.runtime(
        api.LendingRuntimeApi.quote_increase_short, [coldkey_ss58, netuid, rao]
    )
    return _grow_quote(view, quote, netuid)


@read(
    "quote_close_short",
    {"coldkey_ss58": "string", "netuid": "integer", "amount_alpha": "number"},
    category="Lending",
    param_docs={
        "coldkey_ss58": "Coldkey holding the short.",
        "netuid": "Subnet of the short.",
        "amount_alpha": "Alpha bought back; more than the short's size quotes closing it whole.",
    },
)
async def quote_close_short(view, coldkey_ss58: str, netuid: int, amount_alpha: float) -> dict:
    """What `close_short` of an amount on a coldkey's short would do now.

    The `alpha` bought back (the request capped at the short's size), its
    `cost` in TAO with the parts paid `from_proceeds` and `from_collateral`,
    the `interest` settled from the collateral first, and whether the close is
    `full`, which returns the remaining collateral and the deposit.
    """
    rao = Balance.from_amount(amount_alpha, netuid).rao
    quote = _ok(
        await view.runtime(api.LendingRuntimeApi.quote_close_short, [coldkey_ss58, netuid, rao])
    )
    return {
        "netuid": netuid,
        "alpha": view.balance(int(quote["alpha"]), netuid),
        "cost": view.balance(int(quote["cost"]), 0),
        "from_proceeds": view.balance(int(quote["from_proceeds"]), 0),
        "from_collateral": view.balance(int(quote["from_collateral"]), 0),
        "interest": view.balance(int(quote["interest"]), 0),
        "full": bool(quote["full"]),
    }
