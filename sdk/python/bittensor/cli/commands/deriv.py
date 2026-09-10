"""`btcli deriv`: long and short positions on subnet alpha.

One position per coldkey and subnet. `short` and `long` are the same call with
the side fixed: they add to the position, take from it, or flip it. `close`
settles it; only the owner can. Once a week the chain collects the interest
out of the cushion, and forfeits a position whose cushion cannot pay. There is
no expiry.
"""

from __future__ import annotations

from typing import Optional

import typer

from ...intents import AddPosition, ClosePosition
from ...intents.derivatives import leverage_percent
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import _parse_money

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "Long and short positions on subnet alpha, borrowed from the subnet's own pool. "
        "One position per subnet; `short` and `long` add to it, against it, or through zero."
        f"\n\nGuide: {guide_docs_url('derivatives')}"
    ),
)

POSITIONS_TITLE = "derivative positions (equity estimated on a constant-product curve)"
POSITIONS_COLUMNS = [
    "netuid",
    "side",
    "lev",
    "cushion",
    "proceeds",
    "debt",
    "interest",
    "runway",
    "equity",
]
POSITIONS_LEGEND = [
    ("lev", "exposure over cushion: the blend of every tranche added"),
    ("cushion", "TAO you have put up, returned as the position settles"),
    ("proceeds", "what the opening trades produced (TAO for a short, alpha for a long)"),
    ("debt", "what must be bought back or repaid to the pool at settlement"),
    ("interest", "interest accrued since the chain last collected it from the cushion"),
    ("runway", "days the cushion keeps paying interest; at zero the chain forfeits the position"),
    ("equity", "cushion + proceeds - debt - interest, the debt priced with slippage, in TAO"),
]


def _add_options():
    """The option set `short` and `long` share."""
    return dict(
        netuid=typer.Option(..., "--netuid", help=AddPosition.field_help("netuid")),
        amount=typer.Option(..., "--amount", help=AddPosition.field_help("amount")),
        leverage=typer.Option(1.0, "--leverage", help=AddPosition.field_help("leverage")),
    )


def _submit_add(app_ctx: AppContext, side: str, netuid: int, amount: str, leverage: float) -> None:
    try:
        money = _parse_money(amount, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount`: {error}")
        raise typer.Exit(2)
    try:
        leverage_percent(leverage)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--leverage`: {error}")
        raise typer.Exit(2)
    app_ctx.submit(AddPosition(netuid=netuid, side=side, amount=money, leverage=leverage))


_ADD = _add_options()


@app.command("short")
@with_tx_globals
def add_short(
    ctx: typer.Context,
    netuid: int = _ADD["netuid"],
    amount: str = _ADD["amount"],
    leverage: float = _ADD["leverage"],
):
    """Add short exposure: borrow alpha from the pool and sell it for TAO now.

    Profit if alpha's price falls before you settle; the cushion covers the
    loss if it rises. `--amount` is the TAO the tranche is sized by and
    `--leverage` the multiple of it, up to 1x. With no position, or a short,
    `--amount` is deposited as cushion. Against a long it takes that much off
    at the current price instead, and flips to a short if there is more.
    """
    _submit_add(ctx_of(ctx), "Short", netuid, amount, leverage)


@app.command("long")
@with_tx_globals
def add_long(
    ctx: typer.Context,
    netuid: int = _ADD["netuid"],
    amount: str = _ADD["amount"],
    leverage: float = _ADD["leverage"],
):
    """Add long exposure: borrow TAO from the pool and buy alpha with it now.

    Profit if alpha's price rises before you settle; the cushion covers the
    loss if it falls. `--amount` is the TAO the tranche is sized by and
    `--leverage` the multiple of it, up to 2x. With no position, or a long,
    `--amount` is deposited as cushion. Against a short it takes that much off
    at the current price instead, and flips to a long if there is more.
    """
    _submit_add(ctx_of(ctx), "Long", netuid, amount, leverage)


@app.command("close")
@with_tx_globals
def close_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=ClosePosition.field_help("netuid")),
):
    """Close your position on a subnet and settle it against the pool.

    The trade is reversed at today's price, the pool is repaid with the
    interest owed, and you get what is left of your cushion.
    """
    ctx_of(ctx).submit(ClosePosition(netuid=netuid))


def _runway_cell(days: Optional[float]) -> str:
    if days is None:
        return "-"
    return f"{max(days, 0.0):.0f}d"


def _position_row(pos: dict) -> list:
    return [
        pos["netuid"],
        pos["side"],
        f"{pos['leverage']:g}x",
        str(pos["cushion"]),
        str(pos["proceeds"]),
        str(pos["debt"]),
        str(pos["interest_due_tao"]),
        _runway_cell(pos["runway_days"]),
        str(pos["equity_tao"]),
    ]


def _position_record(pos: dict) -> dict:
    return {
        "coldkey": pos["coldkey"],
        "netuid": pos["netuid"],
        "side": pos["side"],
        "leverage": pos["leverage"],
        "cushion": str(pos["cushion"]),
        "proceeds": str(pos["proceeds"]),
        "debt": str(pos["debt"]),
        "escrow": str(pos["escrow"]),
        "exposure_tao": pos["exposure_tao"].tao,
        "interest_per_year_tao": pos["interest_per_year_tao"].tao,
        "interest_due_tao": pos["interest_due_tao"].tao,
        "since": pos["since"],
        "due": pos["due"],
        "runway_days": pos["runway_days"],
        "equity_tao": pos["equity_tao"].tao,
    }


@app.command("list")
@with_globals
def list_positions(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    netuid: Optional[int] = typer.Option(
        None, "--netuid", help="Only show the position on this subnet."
    ),
):
    """List a coldkey's open positions, one per subnet, with interest, runway, and equity.

    Runway is how many days the cushion keeps paying interest at the current
    rate; when it reaches zero the chain forfeits the position to the pool.
    Add cushion to extend it. Equity prices the buyback or sale on a
    constant-product curve and subtracts the interest due; the chain's own
    quote decides at settlement.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    positions = app_ctx.run(lambda client: client.read("derivative_positions", coldkey_ss58=owner))
    if netuid is not None:
        positions = [p for p in positions if p["netuid"] == netuid]
    app_ctx.output.table(
        POSITIONS_TITLE,
        POSITIONS_COLUMNS,
        [_position_row(pos) for pos in positions],
        [_position_record(pos) for pos in positions],
        legend=POSITIONS_LEGEND,
    )


@app.command("params")
@with_globals
def show_params(ctx: typer.Context):
    """Show the two parameters, pool share and interest rate, and the fixed limits."""
    app_ctx: AppContext = ctx_of(ctx)
    params = app_ctx.run(lambda client: client.read("derivatives_params"))
    app_ctx.output.detail("derivatives params", params)
