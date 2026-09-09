"""`btcli deriv`: long and short positions on subnet alpha.

One position per coldkey and subnet. `short` and `long` are the same call with
the side fixed: they add to the position, take from it, or flip it. `close`
ends a position: by its owner at any time, or by anyone once it can no longer
pay a day of rent; `closable` lists those. There is no expiry.
"""

from __future__ import annotations

from typing import Optional

import typer

from ...intents import AddPosition, ClosePosition
from ...intents.derivatives import DepositAssetChoice, leverage_percent
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
    "rent",
    "equity",
    "health",
]
POSITIONS_LEGEND = [
    ("lev", "exposure over cushion value: the blend of every tranche added"),
    ("cushion", "what you have put up (TAO, plus alpha if any), returned as the position settles"),
    ("proceeds", "what the opening trades produced (TAO for a short, alpha for a long)"),
    ("debt", "what must be bought back or repaid to the pool at settlement"),
    ("rent", "rent owed so far: a day per add, then `rate_per_year` of exposure per block"),
    ("equity", "cushion + proceeds - debt - rent, the debt priced with slippage, in TAO"),
    ("health", "ok while equity covers one more day of rent; else closable by anyone"),
]


def _add_options():
    """The option set `short` and `long` share."""
    return dict(
        netuid=typer.Option(..., "--netuid", help=AddPosition.field_help("netuid")),
        amount=typer.Option(..., "--amount", help=AddPosition.field_help("amount")),
        leverage=typer.Option(1.0, "--leverage", help=AddPosition.field_help("leverage")),
        deposit_in=typer.Option(
            DepositAssetChoice.tao,
            "--in",
            help=AddPosition.field_help("deposit_in"),
        ),
        hotkey_ss58=typer.Option(
            None, address_cli_name("hotkey_ss58"), help=AddPosition.field_help("hotkey_ss58")
        ),
    )


def _submit_add(
    app_ctx: AppContext,
    side: str,
    netuid: int,
    amount: str,
    leverage: float,
    deposit_in: DepositAssetChoice,
    hotkey_ss58: Optional[str],
) -> None:
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
    # Only an alpha cushion lives on a hotkey; a TAO cushion must not prompt for one.
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58) if deposit_in == "alpha" else None
    app_ctx.submit(
        AddPosition(
            netuid=netuid,
            side=side,
            amount=money,
            leverage=leverage,
            deposit_in=deposit_in.value,
            hotkey_ss58=hotkey,
        )
    )


_ADD = _add_options()


@app.command("short")
@with_tx_globals
def add_short(
    ctx: typer.Context,
    netuid: int = _ADD["netuid"],
    amount: str = _ADD["amount"],
    leverage: float = _ADD["leverage"],
    deposit_in: DepositAssetChoice = _ADD["deposit_in"],
    hotkey_ss58: Optional[str] = _ADD["hotkey_ss58"],
):
    """Add short exposure: borrow alpha from the pool and sell it for TAO now.

    Profit if alpha's price falls before you settle; the cushion covers the
    loss if it rises. `--amount` is the TAO the tranche is sized by and
    `--leverage` the multiple of it, up to the short maximum in `btcli deriv
    params`. With no position, or a short, `--amount` is deposited as cushion
    (TAO, or the subnet's alpha with `--in alpha` from stake on `--hotkey`).
    Against a long it takes that much off at the current price instead, and
    flips to a short if there is more.
    """
    _submit_add(ctx_of(ctx), "Short", netuid, amount, leverage, deposit_in, hotkey_ss58)


@app.command("long")
@with_tx_globals
def add_long(
    ctx: typer.Context,
    netuid: int = _ADD["netuid"],
    amount: str = _ADD["amount"],
    leverage: float = _ADD["leverage"],
    deposit_in: DepositAssetChoice = _ADD["deposit_in"],
    hotkey_ss58: Optional[str] = _ADD["hotkey_ss58"],
):
    """Add long exposure: borrow TAO from the pool and buy alpha with it now.

    Profit if alpha's price rises before you settle; the cushion covers the
    loss if it falls. `--amount` is the TAO the tranche is sized by and
    `--leverage` the multiple of it, up to the long maximum in `btcli deriv
    params`. With no position, or a long, `--amount` is deposited as cushion
    (TAO, or the subnet's alpha with `--in alpha` from stake on `--hotkey`).
    Against a short it takes that much off at the current price instead, and
    flips to a long if there is more.
    """
    _submit_add(ctx_of(ctx), "Long", netuid, amount, leverage, deposit_in, hotkey_ss58)


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
    position once it is unhealthy (see `deriv closable`); you are paid the rent
    owed, at least one day of it.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", owner_ss58) if owner_ss58 else None
    app_ctx.submit(ClosePosition(netuid=netuid, owner_ss58=owner))


def _position_row(pos: dict, with_owner: bool) -> list:
    row = [
        pos["netuid"],
        pos["side"],
        f"{pos['leverage']:g}x",
        _cushion_cell(pos),
        str(pos["proceeds"]),
        str(pos["debt"]),
        str(pos["accrued_fee_tao"]),
        str(pos["equity_tao"]),
        "ok" if pos["healthy"] else "closable",
    ]
    return [pos["coldkey"], *row] if with_owner else row


def _cushion_cell(pos: dict) -> str:
    alpha = pos["cushion_alpha"]
    return f"{pos['cushion']} + {alpha}" if alpha.rao else str(pos["cushion"])


def _position_record(pos: dict) -> dict:
    return {
        "coldkey": pos["coldkey"],
        "netuid": pos["netuid"],
        "side": pos["side"],
        "leverage": pos["leverage"],
        "cushion": str(pos["cushion"]),
        "cushion_alpha": str(pos["cushion_alpha"]),
        "cushion_alpha_hotkey": pos["cushion_alpha_hotkey"],
        "proceeds": str(pos["proceeds"]),
        "debt": str(pos["debt"]),
        "escrow": str(pos["escrow"]),
        "exposure_tao": pos["exposure_tao"].tao,
        "fee_per_day_tao": pos["fee_per_day_tao"].tao,
        "accrued_fee_tao": pos["accrued_fee_tao"].tao,
        "opened_at": pos["opened_at"],
        "equity_tao": pos["equity_tao"].tao,
        "healthy": pos["healthy"],
    }


def _positions_table(app_ctx: AppContext, title: str, positions: list[dict], with_owner: bool):
    columns = ["owner", *POSITIONS_COLUMNS] if with_owner else POSITIONS_COLUMNS
    app_ctx.output.table(
        title,
        columns,
        [_position_row(pos, with_owner) for pos in positions],
        [_position_record(pos) for pos in positions],
        legend=POSITIONS_LEGEND,
    )


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
    """List a coldkey's open positions, one per subnet, with estimated equity and health.

    Equity prices the buyback or sale on a constant-product curve and subtracts
    the rent owed so far; the chain's own quote decides at settlement.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    positions = app_ctx.run(lambda client: client.read("derivative_positions", coldkey_ss58=owner))
    if netuid is not None:
        positions = [p for p in positions if p["netuid"] == netuid]
    _positions_table(app_ctx, POSITIONS_TITLE, positions, with_owner=False)


@app.command("closable")
@with_globals
def list_closable(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet whose positions to scan."),
    all_positions: bool = typer.Option(
        False, "--all", help="Show every position on the subnet, healthy ones too."
    ),
):
    """List positions on a subnet that anyone may close, lowest equity first.

    A position is closable once its equity no longer covers one day of rent.
    Closing one with `deriv close --owner <coldkey>` pays you the rent owed,
    topped up by the pool to one day. Health here is an estimate; the chain
    rejects a close of a position it still finds healthy.
    """
    app_ctx: AppContext = ctx_of(ctx)
    positions = app_ctx.run(
        lambda client: client.read("derivative_positions_on_subnet", netuid=netuid)
    )
    if not all_positions:
        positions = [p for p in positions if not p["healthy"]]
    title = f"netuid {netuid}: {'all' if all_positions else 'closable'} positions"
    _positions_table(app_ctx, title, positions, with_owner=True)


@app.command("params")
@with_globals
def show_params(
    ctx: typer.Context,
    netuid: Optional[int] = typer.Option(
        None,
        "--netuid",
        help="Also show this subnet's override of the switches, cap, and rate, if root set one.",
    ),
):
    """Show the derivatives pallet's parameters: max leverage, pool cap, yearly rent.

    With `--netuid`, also show whether that subnet is paused, capped, or priced
    differently from the global parameters.
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
