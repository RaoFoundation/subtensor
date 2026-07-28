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
    coldkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=(
            f"{ss58_param_help('coldkey_ss58')} Defaults to the hotkey owner "
            "(collateral is keyed by hotkey + coldkey)."
        ),
    ),
):
    """Show a `(hotkey, coldkey)` position's collateral on a subnet.

    Includes the locked amount (non-withdrawable stake released through
    earned emission), the self-maintained floor, the drain-ratio snapshot,
    and the subnet's collateral policy. Omit `--coldkey` to use the hotkey
    owner.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    # Omit → read looks up the hotkey owner. Only resolve when the user
    # explicitly passed a coldkey (address-book name, wallet, or ss58).
    coldkey = (
        app_ctx.resolve_address("coldkey_ss58", coldkey_ss58) if coldkey_ss58 is not None else None
    )

    async def _op(client):
        collateral, policy = await asyncio.gather(
            client.read(
                "miner_collateral",
                netuid=netuid,
                hotkey_ss58=hotkey,
                coldkey_ss58=coldkey,
            ),
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
    """List every `(hotkey, coldkey)` position with standing collateral on a subnet.

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
    amount_alpha: str = typer.Option(
        ...,
        "--amount-alpha",
        "--amount",
        help=AddCollateral.field_help("amount_alpha")
        or "Alpha to lock as collateral (free stake first, then buy shortfall).",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
    rate_tolerance: float = typer.Option(
        0.05,
        "--rate-tolerance",
        help=AddCollateral.field_help("rate_tolerance")
        or "Max price move (fraction) for any TAO→alpha shortfall buy.",
    ),
):
    """Lock additional miner collateral (free stake first, then buy shortfall).

    Always MEV-shielded: the shortfall buy is fill-or-kill against spot ×
    (1 + rate tolerance). ``--no-mev-shield`` is refused.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(amount_alpha, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount-alpha`: {error}")
        raise typer.Exit(2)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(
        AddCollateral(
            netuid=netuid,
            amount_alpha=amount,
            hotkey_ss58=hotkey,
            rate_tolerance=rate_tolerance,
        )
    )


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

    The drain never releases the lock below the floor, and earned emission
    fills any shortfall — no more re-locking drained funds to track a
    validator-published requirement.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(min_alpha, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--min-alpha`: {error}")
        raise typer.Exit(2)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(SetMinCollateral(netuid=netuid, min_alpha=amount, hotkey_ss58=hotkey))
