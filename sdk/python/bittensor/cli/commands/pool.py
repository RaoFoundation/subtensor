"""`btcli pool`: the canonical tUSD/TAO pool."""

from __future__ import annotations

from typing import Optional

import typer

from ...settings import guide_docs_url, query_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals
from ..tx import intent_command

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "The canonical pool: swap between tUSD and TAO, and provide its liquidity."
        f"\n\nGuide: {guide_docs_url('rails')}"
    ),
)


def _percent(fraction: float) -> str:
    return (
        f"{fraction:.4%}".rstrip("0").rstrip(".") if "." in f"{fraction:.4%}" else f"{fraction:.4%}"
    )


def _usd(price: Optional[float]) -> str:
    return "none yet" if price is None else f"${price:,.4f}"


@app.command(
    "show",
    help="Show the pool: reserves, prices, fee and caps, and an account's shares.\n\n"
    f"Docs: {query_docs_url('canonical_pool')}",
)
@with_globals
def show(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=f"Account whose shares are shown. {ss58_param_help('coldkey_ss58')}",
    ),
):
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return (
            await client.read("canonical_pool"),
            await client.read("pool_shares", coldkey_ss58=coldkey),
        )

    pool, shares = app_ctx.run(_op)
    app_ctx.output.detail(
        "canonical pool",
        {
            "status": "on" if pool["enabled"] else "off",
            "reserves": f"{pool['reserve_tao']} and {pool['reserve_tusd']} tUSD",
            "price": _usd(pool["price_usd"]),
            "price over time": _usd(pool["twap_usd"]),
            "fee": _percent(pool["fee"]),
            "largest swap": f"{_percent(pool['max_swap_fraction'])} of the paying reserve",
            "flow limit": (
                f"{_percent(pool['window_fraction'])} of the reserve per "
                f"{pool['window_blocks']} blocks, each way"
            ),
            "your shares": (
                f"{shares['shares']} of {shares['total_shares']}"
                f" (redeem {shares['tao']} and {shares['tusd']} tUSD)"
            ),
        },
        {"pool": pool, "shares": shares},
    )


@app.command(
    "quote",
    help="Quote a swap: what selling an amount pays now, after the fee.\n\n"
    f"Docs: {query_docs_url('quote_pool_swap')}",
)
@with_globals
def quote(
    ctx: typer.Context,
    sell: str = typer.Option(..., "--sell", help="The asset sold: tusd or tao."),
    amount: float = typer.Option(..., "--amount", help="How much of the sold asset."),
):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("quote_pool_swap", sell=sell, amount=amount)

    result = app_ctx.run(_op)
    received = result["receive"]
    app_ctx.output.detail(
        "pool quote",
        {
            "sell": f"{amount} {'TAO' if result['sell'] == 'tao' else 'tUSD'}",
            "receive": f"{received} tUSD" if result["sell"] == "tao" else str(received),
        },
        result,
    )


# The pool's calls are the generated intent commands (`tx swap-pool` etc.), mounted
# here so both spellings stay in sync.
for _alias, _op in (
    ("swap", "swap_pool"),
    ("add", "add_pool_liquidity"),
    ("remove", "remove_pool_liquidity"),
):
    app.command(_alias)(intent_command(_op))
