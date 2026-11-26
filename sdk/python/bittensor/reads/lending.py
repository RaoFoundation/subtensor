"""Block-pinned synthetic shorts, subnet short books and runtime quotes."""

from __future__ import annotations

from typing import Any, Optional, cast

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..intents._money import alpha_amount
from ..intents.lending import _ok
from ..result import BittensorError
from .base import read


def _state(raw: Any) -> str:
    return str(raw if isinstance(raw, str) else next(iter(raw)))


async def _position_record(view, owner: str, netuid: int, raw: Any) -> Optional[dict]:
    """Stored short plus its runtime health at the pinned block."""
    if not isinstance(raw, dict):
        return None
    health = await view.runtime(api.LendingRuntimeApi.short_health, [owner, netuid])
    health = health if isinstance(health, dict) else {}
    funds = int(raw["collateral"]) + int(raw["proceeds"])
    coverage = health.get("coverage_cost")
    return {
        "coldkey": owner,
        "netuid": netuid,
        "custody": raw["custody"],
        "minted": view.balance(int(raw["minted"]), netuid),
        "collateral": view.balance(int(raw["collateral"])),
        "proceeds": view.balance(int(raw["proceeds"])),
        "funds": view.balance(funds),
        "deposit": view.balance(int(raw["deposit"])),
        "zone_minted": view.balance(int(raw["zone_minted"]), netuid),
        "zone_proceeds": view.balance(int(raw["zone_proceeds"])),
        "opened_at": int(raw["opened_at"]),
        "due": int(raw["due"]),
        "deleveraging": _state(raw["state"]) != "Open",
        "terms_version": int(raw["terms_version"]),
        "accrued_interest": view.balance(int(health.get("accrued_interest") or 0)),
        "day_interest": view.balance(int(health.get("day_interest") or 0)),
        "equity_twap": int(health.get("equity_twap") or 0),
        "coverage_cost": None if coverage is None else view.balance(int(coverage)),
        "covered": coverage is not None and funds >= int(coverage),
    }


@read(
    "lending_position",
    {"coldkey_ss58": "string", "netuid": "integer"},
    category="Prices & swaps",
    param_docs={"coldkey_ss58": "Owner of the short.", "netuid": "Subnet of the short."},
)
async def lending_position(view, coldkey_ss58: str, netuid: int) -> Optional[dict]:
    """A coldkey's synthetic short on one subnet, with its health, or None.

    ``minted`` is alpha still owed to the book. ``collateral`` and the locked
    sale ``proceeds`` are TAO; together they are ``funds``. ``deposit`` is
    refunded on a full close and burned on default. ``zone_minted`` and
    ``zone_proceeds`` track alpha minted in the prune zone, which settles at
    its own proceeds on deregistration. ``coverage_cost`` is the position's
    share of buying the whole book back after a stressed pump (None when the
    pumped pool cannot supply the book); ``covered`` says whether ``funds``
    meet it. ``equity_twap`` is ``funds`` less the buy-back cost at the median
    TWAP, in rao; a negative value starts deleveraging. ``accrued_interest``
    is interest owed since the last charge.
    """
    view = await view.at()
    raw = await view.query(st.Lending.Positions, [coldkey_ss58, netuid])
    return await _position_record(view, coldkey_ss58, netuid, raw)


@read(
    "lending_positions",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet whose shorts to list."},
)
async def lending_positions(view, netuid: int) -> list[dict]:
    """Every short on a subnet, pinned to one block and sorted by owner."""
    view = await view.at()
    records = []
    for key, _ in await view.query_map(st.Lending.OpenByNetuid, [netuid]) or []:
        owner = str(key[0] if isinstance(key, (list, tuple)) else key)
        raw = await view.query(st.Lending.Positions, [owner, netuid])
        record = await _position_record(view, owner, netuid, raw)
        if record is not None:
            records.append(record)
    return sorted(records, key=lambda record: record["coldkey"])


@read(
    "lending_positions_for_coldkey",
    {"coldkey_ss58": "string"},
    category="Prices & swaps",
    param_docs={"coldkey_ss58": "Owner whose shorts to list across every subnet."},
)
async def lending_positions_for_coldkey(view, coldkey_ss58: str) -> list[dict]:
    """Every short one coldkey holds, across subnets, pinned to one block."""
    view = await view.at()
    records = []
    for key, raw in await view.query_map(st.Lending.Positions, [coldkey_ss58]) or []:
        netuid = int(key[0] if isinstance(key, (list, tuple)) else key)
        record = await _position_record(view, coldkey_ss58, netuid, raw)
        if record is not None:
            records.append(record)
    return sorted(records, key=lambda record: record["netuid"])


@read(
    "lending_open_quote",
    {"netuid": "integer", "mint_alpha": "string"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet to short.", "mint_alpha": "Alpha to mint and sell."},
)
async def lending_open_quote(view, netuid: int, mint_alpha: str) -> dict:
    """Quote a fresh short: sale proceeds, required collateral, deposit and interest.

    ``required_collateral`` is the least TAO that, with the proceeds, covers
    the position's whole-book share and a day of interest. The ``deposit`` is
    charged on top. ``annual_interest`` is the flat APR on ``notional``
    (``mint × EMA``). ``in_prune_zone`` means the minted alpha would settle at
    its own proceeds on deregistration. A refusal (disabled, breaker on, net
    mint limit spent, below the minimum notional, shallow pool) is an error.
    """
    view = await view.at()
    mint = cast(Balance, alpha_amount(mint_alpha, netuid))
    if mint.rao <= 0:
        raise BittensorError("mint_alpha must be greater than zero")
    raw = _ok(await view.runtime(api.LendingRuntimeApi.quote_open_short, [netuid, mint.rao]))
    return {
        "mint": mint,
        "proceeds": view.balance(int(raw["proceeds"])),
        "required_collateral": view.balance(int(raw["required_collateral"])),
        "deposit": view.balance(int(raw["deposit"])),
        "notional": view.balance(int(raw["notional"])),
        "apr_bps": int(raw["apr_bps"]),
        "annual_interest": view.balance(int(raw["annual_interest"])),
        "in_prune_zone": bool(raw["in_prune_zone"]),
    }


@read(
    "lending_close_quote",
    {"coldkey_ss58": "string", "netuid": "integer", "alpha": "string"},
    category="Prices & swaps",
    param_docs={
        "coldkey_ss58": "Owner of the short.",
        "netuid": "Subnet of the short.",
        "alpha": "Alpha to buy back. Omit to quote closing the whole position.",
    },
)
async def lending_close_quote(view, coldkey_ss58: str, netuid: int, alpha: str = "") -> dict:
    """Quote buying back short alpha now: TAO cost and refund.

    ``alpha`` is clamped to the position. ``tao_in`` is the buy-back cost,
    swap fees included, paid from proceeds then collateral. ``refund`` is what
    a full close returns to the owner, deposit included; a partial close
    refunds nothing. Fails with ``InsufficientFunds`` when custody cannot pay.
    """
    view = await view.at()
    amount = (1 << 64) - 1 if not alpha else cast(Balance, alpha_amount(alpha, netuid)).rao
    raw = _ok(
        await view.runtime(api.LendingRuntimeApi.quote_close_short, [coldkey_ss58, netuid, amount])
    )
    return {
        "alpha": view.balance(int(raw["alpha"]), netuid),
        "tao_in": view.balance(int(raw["tao_in"])),
        "refund": view.balance(int(raw["refund"])),
    }


@read(
    "lending_subnet",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet whose short book to read."},
)
async def lending_subnet(view, netuid: int) -> dict:
    """A subnet's short book: outstanding mint, residual dilution and breaker.

    ``synthetic_outstanding`` is alpha minted by open shorts and not yet
    bought back; ``deleveraging_minted`` is the part in forced closes.
    ``residual_alpha`` is alpha left by defaulted shorts, which
    ``residual_reserve`` (TAO from interest) retires over time.
    ``breaker_tripped`` blocks new opens and increases while residual alpha is
    high. ``dissolving`` means the subnet is deregistering and shorts settle.
    """
    view = await view.at()
    outstanding = await view.query(st.Lending.SyntheticOutstanding, [netuid])
    deleveraging = await view.query(st.Lending.DeleveragingMinted, [netuid])
    residual = await view.query(st.Lending.ResidualAlpha, [netuid])
    reserve = await view.query(st.Lending.ResidualReserve, [netuid])
    breaker = await view.query(st.Lending.BreakerTripped, [netuid])
    positions = await view.query(st.Lending.PositionCount, [netuid])
    dissolution = await view.query(st.Lending.Dissolutions, [netuid])
    return {
        "netuid": netuid,
        "positions": int(positions or 0),
        "synthetic_outstanding": view.balance(int(outstanding or 0), netuid),
        "deleveraging_minted": view.balance(int(deleveraging or 0), netuid),
        "residual_alpha": view.balance(int(residual or 0), netuid),
        "residual_reserve": view.balance(int(reserve or 0)),
        "breaker_tripped": bool(breaker),
        "dissolving": dissolution is not None,
    }


@read("lending_params", {}, category="Prices & swaps")
async def lending_params(view) -> dict:
    """Whether shorts are enabled, and the root-settable ``SynthParams``.

    Basis-point fields are parts of 10,000. ``min_notional``,
    ``min_pool_depth`` and ``deposit`` are rao.
    """
    view = await view.at()
    enabled = await view.query(st.Lending.Enabled, [])
    params = await view.query(st.Lending.SynthParamsStore, [])
    return {"enabled": bool(enabled), **(params if isinstance(params, dict) else {})}


@read("lending_current_terms_version", {}, category="Prices & swaps")
async def lending_current_terms_version(view) -> dict:
    """Terms version stamped on newly opened shorts."""
    view = await view.at()
    return {
        "terms_version": int(await view.runtime(api.LendingRuntimeApi.current_terms_version, []))
    }
