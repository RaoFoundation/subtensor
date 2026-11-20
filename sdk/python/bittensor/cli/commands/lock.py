"""`btcli conviction`: stake-lock and conviction commands."""

from __future__ import annotations

import asyncio
from typing import Optional

import typer

from ...balance import Balance
from ...intents import LockStake, MoveLock, SetPerpetualLock, SetRejectLockedAlpha
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import chain_identity_names, dust_note, local_address_names, split_dust
from ..tx import resolve_all_amount

app = typer.Typer(
    no_args_is_help=True,
    help=f"Stake-lock and conviction.\n\nGuide: {guide_docs_url('conviction')}",
)

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
        locks, prices, accepts = await asyncio.gather(
            client.read("locks_for_coldkey", coldkey_ss58=owner),
            client.read("alpha_prices"),
            client.read("accepts_locked_alpha", coldkey_ss58=owner),
        )
        unnamed = [lk["hotkey"] for lk in locks if lk["hotkey"] not in hotkey_names]
        return locks, prices, accepts, await chain_identity_names(client, unnamed)

    locks, prices, accepts_locked, identity_names = app_ctx.run(_op)
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
    app_ctx.output.message(
        "incoming locked alpha: accepted (`conviction accept --reject` to opt out)"
        if accepts_locked
        else "incoming locked alpha: rejected (chain default; "
        "`conviction accept --allow` to opt in)"
    )


@app.command("targeting")
@with_globals
def locks_targeting(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to query."),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """List every coldkey's lock targeting a hotkey on a subnet.

    The reverse of ``list``: instead of the locks one coldkey created, this
    shows which coldkeys point conviction at the hotkey — the target-side
    view a subnet owner needs to see who is building conviction toward
    their keys. Also reports the hotkey's total conviction.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    coldkey_names = local_address_names(app_ctx.wallet_path)

    async def _op(client):
        locks, prices, conviction = await asyncio.gather(
            client.read("locks_for_hotkey", hotkey_ss58=hotkey, netuid=netuid),
            client.read("alpha_prices"),
            client.read("hotkey_conviction", hotkey_ss58=hotkey, netuid=netuid),
        )
        unnamed = sorted({lk["coldkey"] for lk in locks} - coldkey_names.keys())
        identities = await asyncio.gather(
            *[client.read("identity", coldkey_ss58=coldkey) for coldkey in unnamed]
        )
        identity_names = {
            coldkey: str(identity["name"])
            for coldkey, identity in zip(unnamed, identities)
            if identity and identity.get("name")
        }
        return locks, prices, conviction, identity_names

    locks, prices, conviction, identity_names = app_ctx.run(_op)

    def _spot_rao(locked: Balance) -> int:
        if netuid == 0:
            return locked.rao
        return int(locked.rao * prices.get(netuid, 0.0))

    locks.sort(key=lambda lk: -lk["locked_alpha"].rao)
    records = []
    rows = []
    total_rao = 0
    for lock in locks:
        locked: Balance = lock["locked_alpha"]
        value = Balance.from_rao(_spot_rao(locked))
        total_rao += value.rao
        coldkey = lock["coldkey"]
        name = coldkey_names.get(coldkey) or identity_names.get(coldkey) or "—"
        records.append(
            {
                "coldkey": coldkey,
                "name": None if name == "—" else name,
                "locked": str(locked),
                "locked_amount": locked.amount,
                "value": str(value),
                "value_tao": value.tao,
                "is_perpetual": lock["is_perpetual"],
            }
        )
        rows.append(
            [
                name,
                coldkey,
                str(locked),
                str(value),
                "perpetual" if lock["is_perpetual"] else "decaying",
            ]
        )
    app_ctx.output.table(
        f"locks targeting {hotkey} on netuid {netuid}",
        ["name", "coldkey", "locked", "value", "mode"],
        rows,
        records,
    )
    conviction_alpha = (conviction or {}).get("conviction_alpha")
    summary = f"total locked value {Balance(total_rao)}"
    if conviction_alpha is not None:
        summary += f" · hotkey conviction {conviction_alpha}"
    app_ctx.output.message(summary)


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
    amount_alpha: Optional[str] = typer.Option(
        None,
        "--amount-alpha",
        "--amount",
        help=LockStake.field_help("amount_alpha")
        or "Amount to lock, in this subnet's alpha (TAO if netuid is 0).",
    ),
    all_amount: bool = typer.Option(
        False, "--all", help="Lock every unlocked alpha on the subnet (same as `--amount all`)."
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
    perpetual: bool = typer.Option(False, "--perpetual", help="Enable perpetual lock mode first."),
):
    """Lock alpha stake on a subnet hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    amount = resolve_all_amount(app_ctx, amount_alpha, all_amount, flag="--amount")
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


@app.command("accept")
@with_tx_globals
def accept_locked_alpha(
    ctx: typer.Context,
    allow: bool = typer.Option(
        ...,
        "--allow/--reject",
        help=(
            "Whether this coldkey accepts incoming locked alpha (transfers of "
            "conviction-locked stake and coldkey swaps that carry locks). "
            "Coldkeys reject locked alpha by default."
        ),
    ),
):
    """Opt this coldkey in or out of receiving locked alpha transfers.

    A locked-stake transfer to a coldkey that has not opted in fails with
    ``AccountRejectsLockedAlpha``; the receiver runs this once with
    ``--allow`` before the sender transfers. ``conviction list`` shows the
    current setting.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(SetRejectLockedAlpha(enabled=not allow))


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
