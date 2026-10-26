"""`btcli deposits`: gamma minting, deposits, and spending."""

from __future__ import annotations

from typing import Optional

import typer

from ...intents import (
    CloseGammaDeposit,
    MintGamma,
    MintGammaWithStake,
    MoveGamma,
    SetGammaDepositDailySpendCap,
    SetGammaDepositSpender,
    SpendGamma,
)
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import _parse_money

app = typer.Typer(
    no_args_is_help=True,
    help=f"Gamma: dollar-valued service credits.\n\nGuide: {guide_docs_url('gamma')}",
)


def _netuid_option(help: str = "Subnet of the deposit."):
    return typer.Option(..., "--netuid", help=help)


def _hotkey_option(param: str, help: str, help_param: Optional[str] = None):
    return typer.Option(
        ..., address_cli_name(param), help=f"{help} {ss58_param_help(help_param or param)}"
    )


def _deposit_hotkey_option():
    return _hotkey_option(
        "hotkey_ss58",
        "Hotkey of the deposit: an API key, not a staking hotkey.",
        help_param="deposit_hotkey_ss58",
    )


def _money(app_ctx: AppContext, value: str, flag: str, allow_all: bool = False):
    try:
        return _parse_money(value, allow_all)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `{flag}`: {error}")
        raise typer.Exit(2)


def _spender_option():
    return typer.Option(
        None,
        address_cli_name("spender_ss58"),
        help=f"{MintGamma.field_help('spender_ss58')} {ss58_param_help('spender_ss58')}",
    )


@app.command("show")
@with_globals
def show_deposit(
    ctx: typer.Context,
    netuid: int = _netuid_option(),
    hotkey: str = _deposit_hotkey_option(),
):
    """Show the gamma deposit at a hotkey: owner, spender, balance, cap, and spendable."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey_ss58 = app_ctx.resolve_address("hotkey_ss58", hotkey)

    async def _op(client):
        return await client.read("gamma_deposit", netuid=netuid, hotkey_ss58=hotkey_ss58)

    deposit = app_ctx.run(_op)
    if deposit is None:
        app_ctx.output.error(f"no gamma deposit at {hotkey_ss58} on netuid {netuid}")
        raise typer.Exit(1)
    app_ctx.output.detail(f"gamma deposit on netuid {netuid}", deposit)


@app.command("list")
@with_globals
def list_deposits(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """List every gamma deposit of a coldkey, on every subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return await client.read("gamma_deposits", coldkey_ss58=coldkey)

    deposits = app_ctx.run(_op)
    rows = [
        [
            d["netuid"],
            d["hotkey"],
            d["spender"],
            d["balance_gamma"],
            "none" if d["daily_spend_cap_gamma"] is None else d["daily_spend_cap_gamma"],
            d["spendable_gamma"],
        ]
        for d in deposits
    ]
    app_ctx.output.table(
        "gamma deposits",
        ["netuid", "hotkey", "spender", "balance", "daily_spend_cap", "spendable"],
        rows,
        deposits,
    )


@app.command("price")
@with_globals
def show_price(ctx: typer.Context):
    """Show the TAO/USD price oracle, its last feed, and the accepted price at that feed."""
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("tao_usd_price")

    app_ctx.output.detail("TAO/USD price", app_ctx.run(_op))


@app.command("mint")
@with_tx_globals
def mint(
    ctx: typer.Context,
    netuid: int = _netuid_option("Subnet whose alpha is bought and burned."),
    hotkey: str = _deposit_hotkey_option(),
    amount_tao: str = typer.Option(..., "--amount-tao", help=MintGamma.field_help("amount_tao")),
    rate_tolerance: float = typer.Option(
        0.05, "--rate-tolerance", help=MintGamma.field_help("rate_tolerance")
    ),
    spender: Optional[str] = _spender_option(),
):
    """Mint gamma with TAO into your deposit at an API-key hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        MintGamma(
            netuid=netuid,
            hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", hotkey),
            amount_tao=_money(app_ctx, amount_tao, "--amount-tao"),
            rate_tolerance=rate_tolerance,
            spender_ss58=app_ctx.resolve_address("spender_ss58", spender),
        )
    )


@app.command("mint-with-stake")
@with_tx_globals
def mint_with_stake(
    ctx: typer.Context,
    netuid: int = _netuid_option("Subnet of the stake burned."),
    hotkey: str = _deposit_hotkey_option(),
    amount_alpha: str = typer.Option(
        ..., "--amount-alpha", help=MintGammaWithStake.field_help("amount_alpha")
    ),
    origin_hotkey: Optional[str] = typer.Option(
        None,
        address_cli_name("origin_hotkey_ss58"),
        help=MintGammaWithStake.field_help("origin_hotkey_ss58"),
    ),
    rate_tolerance: float = typer.Option(
        0.05, "--rate-tolerance", help=MintGammaWithStake.field_help("rate_tolerance")
    ),
    spender: Optional[str] = _spender_option(),
):
    """Mint gamma by burning your stake into your deposit at an API-key hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        MintGammaWithStake(
            netuid=netuid,
            hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", hotkey),
            amount_alpha=_money(app_ctx, amount_alpha, "--amount-alpha", allow_all=True),
            origin_hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", origin_hotkey),
            rate_tolerance=rate_tolerance,
            spender_ss58=app_ctx.resolve_address("spender_ss58", spender),
        )
    )


@app.command("move")
@with_tx_globals
def move(
    ctx: typer.Context,
    netuid: int = _netuid_option("Subnet of both deposits."),
    origin_hotkey: str = _hotkey_option(
        "origin_hotkey_ss58", MoveGamma.field_help("origin_hotkey_ss58")
    ),
    destination_hotkey: str = _hotkey_option(
        "destination_hotkey_ss58", MoveGamma.field_help("destination_hotkey_ss58")
    ),
    amount_gamma: str = typer.Option(
        ..., "--amount-gamma", help=MoveGamma.field_help("amount_gamma")
    ),
):
    """Move gamma between two of your deposits on a subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        MoveGamma(
            netuid=netuid,
            origin_hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", origin_hotkey),
            destination_hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", destination_hotkey),
            amount_gamma=_money(app_ctx, amount_gamma, "--amount-gamma", allow_all=True),
        )
    )


@app.command("close")
@with_tx_globals
def close(
    ctx: typer.Context, netuid: int = _netuid_option(), hotkey: str = _deposit_hotkey_option()
):
    """Close your empty gamma deposit, releasing its hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        CloseGammaDeposit(netuid=netuid, hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", hotkey))
    )


@app.command("set-spender")
@with_tx_globals
def set_spender(
    ctx: typer.Context,
    netuid: int = _netuid_option(),
    hotkey: str = _deposit_hotkey_option(),
    spender: str = typer.Option(
        ...,
        address_cli_name("spender_ss58"),
        help=(
            f"{SetGammaDepositSpender.field_help('spender_ss58')} {ss58_param_help('spender_ss58')}"
        ),
    ),
):
    """Set who may spend your gamma deposit."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        SetGammaDepositSpender(
            netuid=netuid,
            hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", hotkey),
            spender_ss58=app_ctx.resolve_address("spender_ss58", spender),
        )
    )


@app.command("set-cap")
@with_tx_globals
def set_cap(
    ctx: typer.Context,
    netuid: int = _netuid_option(),
    hotkey: str = _deposit_hotkey_option(),
    daily_spend_cap: Optional[str] = typer.Option(
        None, "--daily-spend-cap", help="The cap, in gamma."
    ),
    no_cap: bool = typer.Option(False, "--no-cap", help="Remove the cap."),
):
    """Set or remove the daily spend cap of your gamma deposit."""
    app_ctx: AppContext = ctx_of(ctx)
    if (daily_spend_cap is None) != no_cap:
        app_ctx.output.error("pass exactly one of `--daily-spend-cap` or `--no-cap`")
        raise typer.Exit(2)
    app_ctx.submit(
        SetGammaDepositDailySpendCap(
            netuid=netuid,
            hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", hotkey),
            daily_spend_cap_gamma=None
            if daily_spend_cap is None
            else _money(app_ctx, daily_spend_cap, "--daily-spend-cap"),
        )
    )


@app.command("spend")
@with_tx_globals
def spend(
    ctx: typer.Context,
    netuid: int = _netuid_option("Subnet of the deposits."),
    spends: list[str] = typer.Option(
        ...,
        "--spend",
        help="HOTKEY=AMOUNT, the gamma spent from the deposit at HOTKEY. Repeatable.",
    ),
):
    """Spend gamma from the deposits you are the spender of.

    Entries whose hotkey has no deposit, or whose deposit you are not the
    spender of, are skipped. Sign as the spender, or as its `GammaSpend` proxy.
    """
    app_ctx: AppContext = ctx_of(ctx)
    entries = []
    for entry in spends:
        hotkey, separator, amount = entry.rpartition("=")
        if not separator or not hotkey:
            app_ctx.output.error(f"invalid value for `--spend`: {entry!r} is not HOTKEY=AMOUNT")
            raise typer.Exit(2)
        entries.append(
            {
                "hotkey_ss58": app_ctx.resolve_address("hotkey_ss58", hotkey),
                "amount_gamma": _money(app_ctx, amount, "--spend"),
            }
        )
    app_ctx.submit(SpendGamma(netuid=netuid, spends=entries))
