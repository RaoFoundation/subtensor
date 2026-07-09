"""`btcli deriv`: covered long/short derivatives (runtime API + raw calls)."""

from __future__ import annotations

import json
from typing import Optional

import typer

from ...balance import Balance
from ..call import _resolve_builder
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import _parse_money

app = typer.Typer(no_args_is_help=True, help="Covered long/short derivatives.")


def _side(side: str) -> str:
    normalized = side.lower()
    if normalized not in ("short", "long"):
        raise typer.BadParameter("side must be 'short' or 'long'")
    return normalized


def _amount_rao(raw: str) -> int:
    """Parse a TAO amount option exactly (no float round-trip) into rao."""
    try:
        return Balance.from_tao(_parse_money(raw, False)).rao
    except ValueError as error:
        raise typer.BadParameter(str(error), param_hint="--amount-tao")


def _submit_raw_call(
    app_ctx: AppContext, *, target: str, params: dict, signer: str, success: str
) -> None:
    """Confirm and submit a raw call, honoring --dry-run like `btcli call`."""
    builder = _resolve_builder(target)
    if app_ctx.dry_run:
        app_ctx.run(lambda client: client.compose(builder(**params)))
        app_ctx.output.detail(
            "dry run: raw call", {"target": target, "signer": signer, "params": params}
        )
        return
    app_ctx.confirm(f"submit {target} with {json.dumps(params)}?")

    async def _op(client):
        call = await client.compose(builder(**params))
        return await client.submit_call(call, app_ctx.signer(signer), signer=signer)

    result = app_ctx.run(_op)
    if not app_ctx.output.result(result, success):
        raise typer.Exit(1)


@app.command("quote")
@with_globals
def quote_open(
    ctx: typer.Context,
    side: str = typer.Option(..., "--side", help="Position side: short or long."),
    netuid: int = typer.Option(..., "--netuid", help="Subnet whose derivative market to quote."),
    amount_tao: str = typer.Option(
        ..., "--amount-tao", "--amount", help="Position input, in TAO."
    ),
):
    """Quote opening a derivative position.

    Read-only: asks the runtime API what opening the position would cost
    right now, without signing or submitting anything.
    """
    app_ctx: AppContext = ctx_of(ctx)
    side_name = _side(side)
    rao = _amount_rao(amount_tao)

    async def _op(client):
        method = f"quote_open_{side_name}"
        return await client.runtime(("DerivativesRuntimeApi", method), [netuid, rao])

    quote = app_ctx.run(_op)
    app_ctx.output.detail(
        f"{side_name} open quote", quote if isinstance(quote, dict) else {"quote": quote}
    )


@app.command("positions")
@with_globals
def show_positions(
    ctx: typer.Context,
    side: str = typer.Option(..., "--side", help="Position side: short or long."),
    netuid: Optional[int] = typer.Option(
        None, "--netuid", help="Show only the position on this subnet; omit to list all."
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """List derivative positions for a coldkey."""
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    side_name = _side(side)

    async def _op(client):
        if netuid is None:
            method = f"get_{side_name}_positions"
            return await client.runtime(("DerivativesRuntimeApi", method), [owner])
        method = f"get_{side_name}_position"
        return await client.runtime(("DerivativesRuntimeApi", method), [owner, netuid])

    positions = app_ctx.run(_op)
    app_ctx.output.detail(f"{side_name} positions", positions)


@app.command("market")
@with_globals
def show_market(
    ctx: typer.Context,
    side: str = typer.Option(..., "--side", help="Position side: short or long."),
    netuid: int = typer.Option(..., "--netuid", help="Subnet whose derivative market to show."),
):
    """Show derivative market state for a subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    side_name = _side(side)

    async def _op(client):
        method = f"get_subnet_{side_name}_state"
        return await client.runtime(("DerivativesRuntimeApi", method), [netuid])

    state = app_ctx.run(_op)
    app_ctx.output.detail(f"{side_name} market netuid {netuid}", state)


@app.command("open")
@with_tx_globals
def open_position(
    ctx: typer.Context,
    side: str = typer.Option(..., "--side", help="Position side: short or long."),
    netuid: int = typer.Option(..., "--netuid", help="Subnet to open the position on."),
    amount_tao: str = typer.Option(
        ..., "--amount-tao", "--amount", help="Position input, in TAO."
    ),
    limit_price: Optional[int] = typer.Option(
        None,
        "--limit-price",
        help="Limit price in parts-per-billion; omitted means no price limit.",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Open a derivative position via raw call (requires chain support).

    Composes the raw SubtensorModule.open_short/open_long call, prompts for
    confirmation, and signs with the hotkey.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    side_name = _side(side)
    rao = _amount_rao(amount_tao)
    target = f"SubtensorModule.open_{side_name}"
    params = {
        "hotkey": hotkey,
        "netuid": netuid,
        "position_input": rao,
        "limit_price": limit_price,
    }
    _submit_raw_call(
        app_ctx,
        target=target,
        params=params,
        signer="hotkey",
        success=f"opened {side_name} position",
    )


@app.command("topup")
@with_tx_globals
def top_up(
    ctx: typer.Context,
    side: str = typer.Option(..., "--side", help="Position side: short or long."),
    netuid: int = typer.Option(..., "--netuid", help="Subnet the position lives on."),
    amount_tao: str = typer.Option(
        ..., "--amount-tao", "--amount", help="Top-up amount, in TAO."
    ),
):
    """Top up an existing derivative position."""
    app_ctx: AppContext = ctx_of(ctx)
    side_name = _side(side)
    rao = _amount_rao(amount_tao)
    target = f"SubtensorModule.top_up_{side_name}"
    params = {"netuid": netuid, "amount": rao, "limit_price": None}
    _submit_raw_call(
        app_ctx,
        target=target,
        params=params,
        signer="coldkey",
        success=f"topped up {side_name} position",
    )


@app.command("close")
@with_tx_globals
def close_position(
    ctx: typer.Context,
    side: str = typer.Option(..., "--side", help="Position side: short or long."),
    netuid: int = typer.Option(..., "--netuid", help="Subnet the position lives on."),
    fraction: float = typer.Option(
        1.0,
        "--fraction",
        help="Fraction of the position to close, above 0 and up to 1 (the default, all of it).",
    ),
    from_holdings: bool = typer.Option(
        False,
        "--from-holdings",
        help="Submit the close_short/close_long call instead of the self-closing variant.",
    ),
):
    """Close (part of) a derivative position."""
    app_ctx: AppContext = ctx_of(ctx)
    side_name = _side(side)
    if not 0 < fraction <= 1:
        raise typer.BadParameter(
            f"must be above 0 and up to 1, got {fraction}", param_hint="--fraction"
        )
    suffix = "" if from_holdings else "_self"
    target = f"SubtensorModule.close_{side_name}{suffix}"
    fraction_ppb = int(fraction * 1_000_000_000)
    owner = app_ctx.resolve_address("coldkey_ss58", None)
    params = {"netuid": netuid, "fraction_ppb": fraction_ppb}
    if from_holdings:
        params["coldkey"] = owner
    _submit_raw_call(
        app_ctx,
        target=target,
        params=params,
        signer="coldkey",
        success=f"closed {side_name} position",
    )
