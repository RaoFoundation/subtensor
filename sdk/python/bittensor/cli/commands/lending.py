"""`btcli lending`: short positions, quotes, the market, and the lending parameters."""

from __future__ import annotations

from typing import Optional

import typer

from ...intents.lending import (
    PARAM_GROUPS,
    SetLendingDeregParams,
    SetLendingLongParams,
    SetLendingParams,
    SetLendingShortParams,
)
from ...settings import query_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import intent_command

app = typer.Typer(
    no_args_is_help=True,
    help="Lending: short a subnet's alpha against TAO collateral.",
)
params_app = typer.Typer(no_args_is_help=True, help="The lending switches and parameter groups.")
short_app = typer.Typer(no_args_is_help=True, help="Open, grow, close and collateralize a short.")
quote_app = typer.Typer(no_args_is_help=True, help="What a short call would do now.")
positions_app = typer.Typer(no_args_is_help=True, help="A coldkey's lending positions.")
app.add_typer(params_app, name="params")
app.add_typer(short_app, name="short")
app.add_typer(quote_app, name="quote")
app.add_typer(positions_app, name="positions")

_SETTERS = {
    "lending": SetLendingParams,
    "short": SetLendingShortParams,
    "long": SetLendingLongParams,
    "dereg": SetLendingDeregParams,
}

LONGS_PENDING = (
    "lending longs are pending: their specification is under review and they are disabled on chain"
)


def _coldkey_option(help: str):
    return typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=f"{help} {ss58_param_help('coldkey_ss58')}",
    )


def _position_row(position: dict) -> list:
    short = position["side"] == "short"
    return [
        position["netuid"],
        position["side"],
        str(position["minted_alpha"] if short else position["locked_alpha"]),
        str(position["proceeds"] if short else position["debt"]),
        str(position["collateral"]),
        str(position["deposit"]),
        str(position["interest_due"]),
        position["charge_at"],
    ]


@params_app.command(
    "show",
    help="Show the lending switches and the live parameter groups.\n\n"
    f"Docs: {query_docs_url('lending_params')}",
)
@with_globals
def show_params(ctx: typer.Context):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("lending_params")

    params = app_ctx.run(_op)
    fields = {"enabled": params["enabled"], "longs enabled": params["longs_enabled"]}
    for group in PARAM_GROUPS:
        for name, value in params[group].items():
            fields[f"{group}.{name}"] = f"{value:,}"
    for name, value in (params.get("pending_rate_increase") or {}).items():
        fields[f"pending rate increase.{name}"] = f"{value:,}"
    app_ctx.output.detail("lending parameters", fields, params)


@params_app.command("set")
@with_tx_globals
def set_params(
    ctx: typer.Context,
    group: str = typer.Argument(..., help=f"Parameter group: {', '.join(PARAM_GROUPS)}."),
    changes: list[str] = typer.Argument(
        ..., help="Fields to change as NAME=VALUE; amounts are in rao."
    ),
):
    """Change fields of one lending parameter group (root only).

    The other fields of the group keep their value. The chain checks the whole
    group against its bounds and applies it from the next block. List the
    fields with `btcli lending params show`.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if group not in _SETTERS:
        app_ctx.output.error(
            f"unknown parameter group `{group}`", help=f"use one of {', '.join(PARAM_GROUPS)}"
        )
        raise typer.Exit(2)
    parsed: dict[str, int] = {}
    for change in changes:
        name, sep, value = change.partition("=")
        try:
            if not sep:
                raise ValueError
            parsed[name.strip()] = int(value.strip().replace("_", "").replace(",", ""))
        except ValueError:
            app_ctx.output.error(
                f"invalid change `{change}`", help="write each change as NAME=VALUE, an integer"
            )
            raise typer.Exit(2)
    app_ctx.submit(_SETTERS[group](changes=parsed))


@positions_app.command(
    "list",
    help="List every lending position of a coldkey.\n\n"
    f"Docs: {query_docs_url('lending_positions')}",
)
@with_globals
def list_positions(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = _coldkey_option("Coldkey holding the positions."),
):
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return await client.read("lending_positions", coldkey_ss58=coldkey)

    positions = app_ctx.run(_op)
    app_ctx.output.columns(
        "lending positions",
        [
            "netuid",
            "side",
            "size",
            "proceeds",
            "collateral",
            "deposit",
            "interest due",
            "charge at",
        ],
        [_position_row(position) for position in positions],
        positions,
        right_align={0, 2, 3, 4, 5, 6, 7},
    )


@app.command(
    "market",
    help="Show a subnet's lending market: pool, books and price references.\n\n"
    f"Docs: {query_docs_url('lending_market')}",
)
@with_globals
def show_market(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet of the market."),
):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("lending_market", netuid=netuid)

    market = app_ctx.run(_op)
    if market is None:
        app_ctx.output.error(f"netuid {netuid} has no lending market")
        raise typer.Exit(1)
    app_ctx.output.detail(
        f"lending market on netuid {netuid}",
        {
            "pool TAO": str(market["pool_tao"]),
            "pool alpha": str(market["pool_alpha"]),
            "quote weight": f"{market['quote_weight']:.2%}",
            "swap fee": f"{market['swap_fee']:.3%}",
            "synthetic outstanding": str(market["synthetic_outstanding"]),
            "long locked alpha": str(market["long_locked_alpha"]),
            "long lifted TAO": str(market["long_lifted_tao"]),
            "spot": f"{market['spot']:.9f}",
            "twap": f"{market['twap']:.9f}",
            "sma": f"{market['sma']:.9f}",
        },
        market,
    )


def _show_grow_quote(app_ctx: AppContext, title: str, quote: dict) -> None:
    app_ctx.output.detail(
        title,
        {
            "proceeds": str(quote["proceeds"]),
            "minimum collateral": str(quote["min_collateral"]),
            "coverage need": str(quote["coverage_need"]),
            "deposit": str(quote["deposit"]),
            "interest settled": str(quote["interest"]),
            "spot after": f"{quote['spot_after']:.9f}",
        },
        quote,
    )


@quote_app.command(
    "open",
    help=f"Quote opening a short.\n\nDocs: {query_docs_url('quote_open_short')}",
)
@with_globals
def quote_open(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to short."),
    amount_alpha: float = typer.Option(
        ..., "--amount-alpha", "--amount", help="Alpha minted and sold."
    ),
):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("quote_open_short", netuid=netuid, amount_alpha=amount_alpha)

    _show_grow_quote(app_ctx, f"open a short on netuid {netuid}", app_ctx.run(_op))


@quote_app.command(
    "increase",
    help=f"Quote adding to a short.\n\nDocs: {query_docs_url('quote_increase_short')}",
)
@with_globals
def quote_increase(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet of the short."),
    amount_alpha: float = typer.Option(
        ..., "--amount-alpha", "--amount", help="Alpha minted and sold on top of the short."
    ),
    coldkey_ss58: Optional[str] = _coldkey_option("Coldkey holding the short."),
):
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return await client.read(
            "quote_increase_short", coldkey_ss58=coldkey, netuid=netuid, amount_alpha=amount_alpha
        )

    _show_grow_quote(app_ctx, f"increase the short on netuid {netuid}", app_ctx.run(_op))


@quote_app.command(
    "close",
    help=f"Quote buying back a short.\n\nDocs: {query_docs_url('quote_close_short')}",
)
@with_globals
def quote_close(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet of the short."),
    amount_alpha: float = typer.Option(
        ...,
        "--amount-alpha",
        "--amount",
        help="Alpha bought back; more than the short's size quotes closing it whole.",
    ),
    coldkey_ss58: Optional[str] = _coldkey_option("Coldkey holding the short."),
):
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return await client.read(
            "quote_close_short", coldkey_ss58=coldkey, netuid=netuid, amount_alpha=amount_alpha
        )

    quote = app_ctx.run(_op)
    app_ctx.output.detail(
        f"close the short on netuid {netuid}",
        {
            "alpha bought back": str(quote["alpha"]),
            "cost": str(quote["cost"]),
            "from proceeds": str(quote["from_proceeds"]),
            "from collateral": str(quote["from_collateral"]),
            "interest settled": str(quote["interest"]),
            "closes the position": "yes" if quote["full"] else "no",
        },
        quote,
    )


@app.command("long")
@with_globals
def long_pending(ctx: typer.Context):
    """Lending longs: pending, not available yet."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.error(LONGS_PENDING, help="shorts are under `btcli lending short`")
    raise typer.Exit(1)


# The short calls and the two switches are the generated intent commands
# (`tx open-short` etc.), mounted here so both spellings stay in sync.
for _alias, _op in (
    ("open", "open_short"),
    ("increase", "increase_short"),
    ("close", "close_short"),
    ("add-collateral", "short_add_collateral"),
):
    short_app.command(_alias)(intent_command(_op))

for _alias, _op in (
    ("enable", "set_lending_enabled"),
    ("enable-longs", "set_lending_longs_enabled"),
):
    params_app.command(_alias)(intent_command(_op))
