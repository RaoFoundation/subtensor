"""`btcli deriv`: expiry-bounded long and short positions on subnet alpha.

One position per coldkey and subnet. `short` and `long` are the same call with
the side fixed: they add to the position, take from it, or flip it.
"""

from __future__ import annotations

import asyncio
from typing import Optional

import typer

from ...balance import Balance
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

POSITIONS_TITLE = "derivative positions (est. value at spot, before slippage)"


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
    `--leverage` the multiple of it, up to the short maximum in `btcli deriv
    params`. With no position, or a short, `--amount` is deposited as cushion.
    Against a long it takes that much off at the current price instead, and
    flips to a short if there is more.
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
    `--leverage` the multiple of it, up to the long maximum in `btcli deriv
    params`. With no position, or a long, `--amount` is deposited as cushion.
    Against a short it takes that much off at the current price instead, and
    flips to a long if there is more.
    """
    _submit_add(ctx_of(ctx), "Long", netuid, amount, leverage)


@app.command("close")
@with_tx_globals
def close_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=ClosePosition.field_help("netuid")),
    owner_ss58: Optional[str] = typer.Option(
        None,
        "--owner",
        help=ClosePosition.field_help("owner_ss58"),
    ),
):
    """Close your position on a subnet and settle it against the pool.

    The owner may close at any time. Pass `--owner` to close someone else's
    position once it has expired.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", owner_ss58) if owner_ss58 else None
    app_ctx.submit(ClosePosition(netuid=netuid, owner_ss58=owner))


@app.command("list")
@with_globals
def list_positions(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    netuid: Optional[int] = typer.Option(
        None, "--netuid", help="Only show positions on this subnet."
    ),
):
    """List a coldkey's open positions, one per subnet, with an estimated close value.

    The estimate prices the buyback or sale at spot and subtracts the borrow
    fee owed so far. The real settlement pays slippage on top.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        positions, prices, block = await asyncio.gather(
            client.read("derivative_positions", coldkey_ss58=owner),
            client.read("alpha_prices"),
            client.block(),
        )
        return positions, prices, block

    positions, prices, block = app_ctx.run(_op)
    if netuid is not None:
        positions = [p for p in positions if p["netuid"] == netuid]

    rows = []
    records = []
    for pos in positions:
        price = prices.get(pos["netuid"], 0.0)
        estimate = _estimated_close_value(pos, price)
        blocks_left = max(0, pos["expires_at"] - block)
        rows.append(
            [
                pos["netuid"],
                pos["side"],
                f"{pos['leverage']:g}x",
                str(pos["cushion"]),
                str(pos["proceeds"]),
                str(pos["debt"]),
                str(pos["accrued_fee_tao"]),
                "expired" if pos["expired"] else f"{blocks_left} blocks",
                str(estimate),
            ]
        )
        records.append(
            {
                "netuid": pos["netuid"],
                "side": pos["side"],
                "leverage": pos["leverage"],
                "cushion": str(pos["cushion"]),
                "proceeds": str(pos["proceeds"]),
                "debt": str(pos["debt"]),
                "escrow": str(pos["escrow"]),
                "exposure_tao": pos["exposure_tao"].tao,
                "fee_per_day_tao": pos["fee_per_day_tao"].tao,
                "accrued_fee_tao": pos["accrued_fee_tao"].tao,
                "opened_at": pos["opened_at"],
                "expires_at": pos["expires_at"],
                "expired": pos["expired"],
                "estimated_value_tao": estimate.tao,
            }
        )
    app_ctx.output.table(
        POSITIONS_TITLE,
        ["netuid", "side", "lev", "cushion", "proceeds", "debt", "fee", "expires in", "est. value"],
        rows,
        records,
        legend=[
            ("lev", "exposure over cushion: the blend of every tranche added"),
            ("cushion", "the TAO you have put up, returned as the position settles"),
            ("proceeds", "what the opening trades produced (TAO for a short, alpha for a long)"),
            ("debt", "what must be bought back or repaid to the pool at settlement"),
            ("fee", "borrow fee owed so far: a day per add, then the summed rate per block"),
            ("est. value", "cushion + proceeds - debt - fee at spot, in TAO"),
        ],
    )


def _estimated_close_value(pos: dict, tao_per_alpha: float) -> Balance:
    """Cushion plus proceeds minus debt and fee, everything valued in TAO at spot."""

    def tao_of(balance: Balance) -> int:
        if balance.netuid == 0:
            return balance.rao
        return int(balance.rao * tao_per_alpha)

    value = (
        pos["cushion"].rao
        + tao_of(pos["proceeds"])
        - tao_of(pos["debt"])
        - pos["accrued_fee_tao"].rao
    )
    return Balance.from_rao(max(0, value))


@app.command("params")
@with_globals
def show_params(
    ctx: typer.Context,
    netuid: Optional[int] = typer.Option(
        None,
        "--netuid",
        help="Also show this subnet's override of the switches and cap, if root set one.",
    ),
):
    """Show the derivatives pallet's parameters: max leverage, pool cap, lifetime, fees.

    With `--netuid`, also show whether that subnet is paused or capped differently
    from the global parameters.
    """
    app_ctx: AppContext = ctx_of(ctx)
    params = app_ctx.run(lambda client: client.read("derivatives_params"))
    app_ctx.output.detail("derivatives params", params)
    if netuid is None:
        return
    override = app_ctx.run(lambda client: client.read("derivatives_subnet_override", netuid=netuid))
    if override is None:
        app_ctx.output.message(f"netuid {netuid}: no override, global parameters apply")
    else:
        app_ctx.output.detail(f"netuid {netuid} override", override)
