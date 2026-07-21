"""`btcli collateral`: miner registration-collateral commands."""

from __future__ import annotations

import asyncio
from typing import Optional

import typer

from ...intents import AddCollateral, SetMinCollateral
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import _parse_money

app = typer.Typer(no_args_is_help=True, help="Miner registration collateral.")


@app.command("show")
@with_globals
def show_collateral(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to query."),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Show a miner hotkey's collateral on a subnet.

    Includes the locked amount (non-withdrawable stake released through
    earned incentive), the self-maintained floor, the drain-ratio snapshot,
    and the subnet's collateral policy.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)

    async def _op(client):
        collateral, policy = await asyncio.gather(
            client.read("miner_collateral", netuid=netuid, hotkey_ss58=hotkey),
            client.read("collateral_policy", netuid=netuid),
        )
        return {"collateral": collateral, "policy": policy}

    data = app_ctx.run(_op)
    app_ctx.output.detail(f"collateral on netuid {netuid}", data)


@app.command("list")
@with_globals
def list_collateral(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to query."),
):
    """List every miner hotkey with standing collateral on a subnet.

    The same view validator code uses to enforce a per-machine collateral
    requirement, sorted by locked amount.
    """
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("subnet_collateral", netuid=netuid)

    data = app_ctx.run(_op)
    app_ctx.output.detail(f"collateral on netuid {netuid}", data)


@app.command("add")
@with_tx_globals
def add_collateral(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", help=AddCollateral.field_help("netuid") or "Subnet to lock collateral on."
    ),
    amount_tao: str = typer.Option(
        ...,
        "--amount-tao",
        "--amount",
        help=AddCollateral.field_help("amount_tao") or "TAO to stake and lock as collateral.",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Stake TAO to your hotkey and lock it as additional miner collateral."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(amount_tao, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount-tao`: {error}")
        raise typer.Exit(2)
    app_ctx.submit(AddCollateral(netuid=netuid, amount_tao=amount, hotkey_ss58=hotkey_ss58))


@app.command("set-min")
@with_tx_globals
def set_min_collateral(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ...,
        "--netuid",
        help=SetMinCollateral.field_help("netuid") or "Subnet the floor applies to.",
    ),
    min_alpha: str = typer.Option(
        ...,
        "--min-alpha",
        "--amount",
        help=SetMinCollateral.field_help("min_alpha")
        or "The floor, in the subnet's alpha. Zero clears it.",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Set the self-maintaining collateral floor for your hotkey.

    The drain never releases the lock below the floor, and earned incentive
    fills any shortfall — no more re-locking drained funds to track a
    validator-published requirement.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(min_alpha, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--min-alpha`: {error}")
        raise typer.Exit(2)
    app_ctx.submit(SetMinCollateral(netuid=netuid, min_alpha=amount, hotkey_ss58=hotkey_ss58))
