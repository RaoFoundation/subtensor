"""`btcli lock`: stake-lock and conviction commands."""

from __future__ import annotations

import asyncio
from typing import Optional

import typer

from ...balance import Balance
from ...intents import LockStake, MoveLock, SetPerpetualLock
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import chain_identity_names, dust_note, local_address_names, split_dust
from ..tx import _parse_money

app = typer.Typer(no_args_is_help=True, help="Stake-lock and conviction.")

LOCK_LIST_TITLE = "locks (per-subnet currency: TAO on netuid 0, alpha elsewhere)"


@app.command("list")
@with_globals
def list_locks(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    netuid: Optional[int] = typer.Option(
        None, "--netuid", help="Only show the lock on this subnet."
    ),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show dust locks (spot value < τ0.001). JSON always includes every lock.",
    ),
):
    """List locks per subnet for a coldkey (optionally filtered by netuid).

    Each subnet shows the locked amount, its spot TAO value, and the hotkey
    the lock targets; JSON output carries the flat per-lock records.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    hotkey_names = local_address_names(app_ctx.wallet_path)

    async def _op(client):
        locks, prices = await asyncio.gather(
            client.read("locks_for_coldkey", coldkey_ss58=owner),
            client.read("alpha_prices"),
        )
        unnamed = [lk["hotkey"] for lk in locks if lk["hotkey"] not in hotkey_names]
        return locks, prices, await chain_identity_names(client, unnamed)

    locks, prices, identity_names = app_ctx.run(_op)
    if netuid is not None:
        locks = [lk for lk in locks if lk["netuid"] == netuid]

    def _spot(locked: Balance) -> Balance:
        if locked.netuid == 0:
            return Balance.from_rao(locked.rao)
        return Balance.from_rao(int(locked.rao * prices.get(locked.netuid, 0.0)))

    records = []
    groups = []
    total_rao = 0
    for lock in locks:
        locked: Balance = lock["locked_alpha"]
        value = _spot(locked)
        total_rao += value.rao
        hotkey = lock["hotkey"]
        records.append(
            {
                "netuid": lock["netuid"],
                "hotkey": hotkey,
                "locked": str(locked),
                "locked_amount": locked.amount,
                "locked_unit": "TAO" if lock["netuid"] == 0 else f"alpha (netuid {lock['netuid']})",
                "value": str(value),
                "value_tao": value.tao,
                "is_perpetual": lock["is_perpetual"],
            }
        )
        groups.append(
            {
                "netuid": lock["netuid"],
                "note": "perpetual" if lock["is_perpetual"] else None,
                "stake": str(locked),
                "value": str(value),
                "value_tao": value.tao,
                "positions": [
                    {
                        "stake": str(locked),
                        "value_tao": value.tao,
                        "hotkey": hotkey,
                        "label": hotkey_names.get(hotkey) or identity_names.get(hotkey, hotkey),
                        "named": hotkey in hotkey_names,
                        "identity": hotkey not in hotkey_names and hotkey in identity_names,
                    }
                ],
            }
        )
    shown, dust = (groups, []) if show_dust else split_dust(groups)
    app_ctx.output.stake_list(LOCK_LIST_TITLE, shown, records, Balance(total_rao))
    if dust:
        app_ctx.output.message(dust_note(dust))


@app.command("show")
@with_globals
def show_lock(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet whose lock to inspect."),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Show detailed lock state for one subnet.

    Includes the raw lock record plus the conviction accrued by the hotkey
    the lock targets.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        lock = await client.read("coldkey_lock", coldkey_ss58=owner, netuid=netuid)
        if not lock:
            return {"lock": None, "conviction": None}
        conviction = await client.read(
            "hotkey_conviction", hotkey_ss58=lock["hotkey"], netuid=netuid
        )
        return {"lock": lock, "conviction": conviction}

    data = app_ctx.run(_op)
    app_ctx.output.detail(f"lock on netuid {netuid}", data)


@app.command("add")
@with_tx_globals
def add_lock(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", help=LockStake.field_help("netuid") or "Subnet to lock stake on."
    ),
    amount_alpha: str = typer.Option(
        ...,
        "--amount-alpha",
        "--amount",
        help=LockStake.field_help("amount_alpha")
        or "Amount to lock, in this subnet's alpha (TAO if netuid is 0).",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
    perpetual: bool = typer.Option(False, "--perpetual", help="Enable perpetual lock mode first."),
):
    """Lock alpha stake on a subnet hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(amount_alpha, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount-alpha`: {error}")
        raise typer.Exit(2)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    if perpetual:
        app_ctx.submit(SetPerpetualLock(netuid=netuid, enabled=True))
    app_ctx.submit(LockStake(netuid=netuid, amount_alpha=amount, hotkey_ss58=hotkey))


@app.command("mode")
@with_tx_globals
def lock_mode(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ...,
        "--netuid",
        help=SetPerpetualLock.field_help("netuid") or "Subnet whose lock mode to change.",
    ),
    perpetual: bool = typer.Option(
        ...,
        "--perpetual/--decaying",
        help=SetPerpetualLock.field_help("enabled")
        or "Whether the lock is perpetual (never decays) or decays over time.",
    ),
):
    """Set perpetual or decaying lock mode for a subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(SetPerpetualLock(netuid=netuid, enabled=perpetual))


@app.command("move")
@with_tx_globals
def move_lock(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", help=MoveLock.field_help("netuid") or "Subnet the lock lives on."
    ),
    destination_hotkey_ss58: str = typer.Option(
        ...,
        address_cli_name("destination_hotkey_ss58"),
        help=ss58_param_help("destination_hotkey_ss58"),
    ),
):
    """Move an existing lock to a different hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    dest = app_ctx.resolve_address("hotkey_ss58", destination_hotkey_ss58)
    app_ctx.submit(MoveLock(netuid=netuid, destination_hotkey_ss58=dest))
