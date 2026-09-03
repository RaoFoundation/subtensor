"""`btcli deriv`: expiry-bounded long and short positions on subnet alpha."""

from __future__ import annotations

import asyncio
from typing import Optional

import typer

from ...balance import Balance
from ...intents import ClosePosition, OpenLong, OpenShort, RollPosition
from ...intents.derivatives import SideChoice
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import _parse_money

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "Long and short positions on subnet alpha, borrowed from the subnet's own pool."
        f"\n\nGuide: {guide_docs_url('derivatives')}"
    ),
)

POSITIONS_TITLE = "derivative positions (est. value at spot, before slippage)"


def _open_options():
    """The option set `short` and `long` share."""
    return dict(
        netuid=typer.Option(..., "--netuid", help=OpenShort.field_help("netuid")),
        amount=typer.Option(..., "--amount", help=OpenShort.field_help("amount")),
    )


def _submit_open(app_ctx: AppContext, intent_cls: type, netuid: int, amount: str) -> None:
    try:
        money = _parse_money(amount, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount`: {error}")
        raise typer.Exit(2)
    app_ctx.submit(intent_cls(netuid=netuid, amount=money))


_OPEN = _open_options()


@app.command("short")
@with_tx_globals
def open_short(
    ctx: typer.Context,
    netuid: int = _OPEN["netuid"],
    amount: str = _OPEN["amount"],
):
    """Open a short: borrow alpha from the pool and sell it for TAO now.

    Profit if alpha's price falls before you close; the cushion covers the
    loss if it rises. `--amount` is the TAO cushion, taken from the coldkey
    balance.
    """
    _submit_open(ctx_of(ctx), OpenShort, netuid, amount)


@app.command("long")
@with_tx_globals
def open_long(
    ctx: typer.Context,
    netuid: int = _OPEN["netuid"],
    amount: str = _OPEN["amount"],
):
    """Open a long: borrow TAO from the pool and buy alpha with it now.

    Profit if alpha's price rises before you close; the cushion covers the
    loss if it falls. `--amount` is the TAO cushion, taken from the coldkey
    balance.
    """
    _submit_open(ctx_of(ctx), OpenLong, netuid, amount)


@app.command("close")
@with_tx_globals
def close_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=ClosePosition.field_help("netuid")),
    side: SideChoice = typer.Option(
        ..., "--side", help=ClosePosition.field_help("side"), case_sensitive=False
    ),
    owner_ss58: Optional[str] = typer.Option(
        None,
        "--owner",
        help=ClosePosition.field_help("owner_ss58"),
    ),
):
    """Close a position and settle it against the pool.

    The owner may close at any time. Pass `--owner` to close someone else's
    position once it has expired.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", owner_ss58) if owner_ss58 else None
    app_ctx.submit(ClosePosition(netuid=netuid, side=str(side.value), owner_ss58=owner))


@app.command("roll")
@with_tx_globals
def roll_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=RollPosition.field_help("netuid")),
    side: SideChoice = typer.Option(
        ..., "--side", help=RollPosition.field_help("side"), case_sensitive=False
    ),
    top_up: Optional[str] = typer.Option(None, "--add", help=RollPosition.field_help("top_up")),
):
    """Settle a position at today's price and reopen it in one transaction.

    Use this to stay in a trade past its expiry. The loss or profit so far is
    realized, the fee so far is paid, and the TAO that comes back becomes the
    cushion of a fresh position with a full lifetime. `--add` puts more TAO in.
    """
    app_ctx: AppContext = ctx_of(ctx)
    money = None
    if top_up is not None:
        try:
            money = _parse_money(top_up, False)
        except ValueError as error:
            app_ctx.output.error(f"invalid value for `--add`: {error}")
            raise typer.Exit(2)
    app_ctx.submit(RollPosition(netuid=netuid, side=str(side.value), top_up=money))


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
    """List a coldkey's open longs and shorts with an estimated close value.

    The estimate prices the buyback or sale at spot and subtracts the borrow
    fee accrued so far. The real settlement pays slippage on top.
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
        ["netuid", "side", "cushion", "proceeds", "debt", "fee", "expires in", "est. value"],
        rows,
        records,
        legend=[
            ("cushion", "the TAO you put up, returned after settlement"),
            ("proceeds", "what the opening trade produced (TAO for a short, alpha for a long)"),
            ("debt", "what must be bought back or repaid to the pool at close"),
            ("fee", "borrow fee accrued so far at the rate fixed at open (one-day minimum)"),
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
def show_params(ctx: typer.Context):
    """Show the derivatives pallet's parameters: leverage, pool cap, lifetime, fees."""
    app_ctx: AppContext = ctx_of(ctx)
    params = app_ctx.run(lambda client: client.read("derivatives_params"))
    app_ctx.output.detail("derivatives params", params)
