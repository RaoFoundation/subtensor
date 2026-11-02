"""`btcli gamma`: gamma minting, deposits, and spending."""

from __future__ import annotations

from decimal import Decimal
from typing import Optional

import typer

from ...intents import SpendGamma
from ...settings import guide_docs_url, query_docs_url, tx_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import intent_command

app = typer.Typer(
    no_args_is_help=True,
    help=f"Gamma: dollar-valued service credits.\n\nGuide: {guide_docs_url('gamma')}",
)


def _hotkey_option(help: Optional[str]):
    """A required hotkey option, which never falls back to the wallet's hotkey."""
    return typer.Option(
        ...,
        address_cli_name("hotkey_ss58"),
        help=f"{help} {ss58_param_help('hotkey_ss58', wallet_default=False)}",
    )


def _gamma(value: Optional[str]) -> str:
    return "no cap" if value is None else f"{Decimal(value):,f} gamma"


@app.command(
    "show",
    help="Show the gamma deposit at a hotkey on a subnet.\n\n"
    f"Docs: {query_docs_url('gamma_deposit')}",
)
@with_globals
def show_deposit(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet of the deposit."),
    hotkey: str = _hotkey_option("Hotkey of the deposit."),
):
    app_ctx: AppContext = ctx_of(ctx)
    hotkey_ss58 = app_ctx.resolve_address("hotkey_ss58", hotkey)

    async def _op(client):
        return await client.read("gamma_deposit", netuid=netuid, hotkey_ss58=hotkey_ss58)

    deposit = app_ctx.run(_op)
    if deposit is None:
        app_ctx.output.error(
            f"no gamma deposit at {hotkey_ss58} on netuid {netuid}",
            help="list a coldkey's deposits with `btcli gamma list`",
        )
        raise typer.Exit(1)
    app_ctx.output.detail(
        f"gamma deposit on netuid {netuid}",
        {
            "hotkey": deposit["hotkey"],
            "coldkey": deposit["coldkey"],
            "spender": deposit["spender"],
            "balance": _gamma(deposit["balance_gamma"]),
            "daily spend cap": _gamma(deposit["daily_spend_cap_gamma"]),
            "spendable": _gamma(deposit["spendable_gamma"]),
        },
        deposit,
    )


@app.command(
    "list",
    help="List every gamma deposit of a coldkey, on every subnet.\n\n"
    f"Docs: {query_docs_url('gamma_deposits')}",
)
@with_globals
def list_deposits(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=f"Coldkey owning the deposits. {ss58_param_help('coldkey_ss58')}",
    ),
):
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return await client.read("gamma_deposits", coldkey_ss58=coldkey)

    deposits = app_ctx.run(_op)
    rows = [
        [
            d["netuid"],
            d["hotkey"],
            _gamma(d["balance_gamma"]),
            _gamma(d["daily_spend_cap_gamma"]),
            _gamma(d["spendable_gamma"]),
            d["spender"],
        ]
        for d in deposits
    ]
    app_ctx.output.columns(
        "gamma deposits",
        ["netuid", "hotkey", "balance", "daily spend cap", "spendable", "spender"],
        rows,
        deposits,
        right_align={0, 2, 3, 4},
    )


# The deposit owner's calls are the generated intent commands (`tx mint-gamma`
# etc.), mounted as `btcli stake` mounts its verbs, so both spellings stay in sync.
for _alias, _op in (
    ("mint", "mint_gamma"),
    ("mint-with-stake", "mint_gamma_with_stake"),
    ("move", "move_gamma"),
    ("close", "close_gamma_deposit"),
    ("set-spender", "set_gamma_deposit_spender"),
    ("set-cap", "set_gamma_deposit_daily_spend_cap"),
):
    app.command(_alias)(intent_command(_op))


@app.command(
    "spend",
    help="Spend gamma from the deposits you are the spender of.\n\n"
    "Each --spend spends, in order, its AMOUNT from the deposit at its HOTKEY, and spent gamma is "
    "destroyed. Entries whose hotkey has no deposit, whose deposit you are not the spender of, or "
    "whose AMOUNT is zero or above the deposit's balance or what its daily spend cap allows, are "
    "skipped. Sign as the spender, or as its GammaSpend proxy with --proxy-for SPENDER.\n\n"
    f"Docs: {tx_docs_url(SpendGamma.op)}",
)
@with_tx_globals
def spend(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SpendGamma.field_help("netuid")),
    spends: list[str] = typer.Option(
        ...,
        "--spend",
        help="HOTKEY=AMOUNT or HOTKEY=AMOUNT:REFERENCE, spending AMOUNT gamma from the "
        "deposit at HOTKEY, with an optional 32-byte hex reference that the entry's GammaSpent "
        "or GammaSpendSkipped event repeats. Repeatable. HOTKEY is an "
        f"{ss58_param_help('hotkey_ss58', wallet_default=False).replace('HOTKEY', 'NAME')}",
    ),
):
    app_ctx: AppContext = ctx_of(ctx)
    entries = []
    for entry in spends:
        hotkey, separator, amount = entry.rpartition("=")
        if not separator or not hotkey:
            app_ctx.output.error(f"invalid value for `--spend`: {entry!r} is not HOTKEY=AMOUNT")
            raise typer.Exit(2)
        amount, _, reference = amount.partition(":")
        entries.append(
            {
                "hotkey_ss58": app_ctx.resolve_address("hotkey_ss58", hotkey, flag="--spend"),
                "amount_gamma": amount,
                "reference": reference or None,
            }
        )
    app_ctx.submit(SpendGamma(netuid=netuid, spends=entries))
