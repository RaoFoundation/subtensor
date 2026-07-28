"""`btcli subnets`: list subnets and inspect one."""

from __future__ import annotations

import asyncio
import math
from typing import Optional

import typer

from ..._generated import storage
from ...balance import Balance
from ...intents import BurnedRegister, RegisterSubnet
from ...settings import BLOCKTIME
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import chain_identity_names, local_address_names
from ..hyperparams_view import fetch_hyperparameters, show_hyperparameters
from ..metagraph_view import show_metagraph

app = typer.Typer(no_args_is_help=True, help="Inspect subnets.")

PANEL_INSPECT = "Inspect"
PANEL_REGISTER = "Registration"

_NETUID_HELP = "Numeric identifier of the subnet."


@app.command("list", rich_help_panel=PANEL_INSPECT)
@with_globals
def list_subnets(ctx: typer.Context):
    """List all subnets: name, spot alpha price, tempo, burn, and neuron slots."""
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        infos, names, prices, max_uids = await asyncio.gather(
            client.subnets.all(),
            client.read("subnet_names"),
            client.read("alpha_prices"),
            client.query_map(storage.SubtensorModule.MaxAllowedUids),
        )
        return infos, names, prices, {int(k): int(v) for k, v in max_uids}

    infos, names, prices, max_uids = app_ctx.run(_op)
    app_ctx.output.update_subnet_names(names)

    rows = []
    records = []
    for i in infos:
        price = prices.get(i.netuid)
        max_n = max_uids.get(i.netuid)
        rows.append(
            {
                "netuid": i.netuid,
                # Root has no alpha pool; its "price" is identically 1 TAO.
                "price": f"{price:.6f}" if i.netuid != 0 and price is not None else "—",
                "tempo": i.tempo,
                "burn": i.burn,
                "neurons": f"{i.neuron_count}/{max_n}" if max_n else i.neuron_count,
            }
        )
        records.append(
            {
                "netuid": i.netuid,
                "name": names.get(i.netuid),
                "symbol": app_ctx.output.unit(i.netuid),
                "price_tao_per_alpha": price if i.netuid != 0 else None,
                "tempo": i.tempo,
                "burn_tao": i.burn.tao,
                "neurons": i.neuron_count,
                "max_neurons": max_n,
            }
        )
    total_neurons = sum(i.neuron_count for i in infos)
    footer = (
        f"[dim]{len(infos)} subnets  ·  {total_neurons:,} neurons  ·  "
        f"price is TAO per alpha (spot)[/dim]"
    )
    app_ctx.output.subnet_list("subnets", rows, records, footer=footer)


@app.command(rich_help_panel=PANEL_INSPECT)
@with_globals
def show(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
):
    """Show details for a single subnet."""
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        if not await client.query(storage.SubtensorModule.NetworksAdded, [netuid]):
            return None
        return await client.subnets.info(netuid)

    info = app_ctx.run(_op)
    if info is None:
        app_ctx.output.error(f"subnet {netuid} does not exist")
        raise typer.Exit(1)
    app_ctx.output.detail(
        f"subnet {info.netuid}",
        {
            "tempo": info.tempo,
            "burn": info.burn,
            "neurons": info.neuron_count,
        },
    )


@app.command("hyperparameters", rich_help_panel=PANEL_INSPECT)
@with_globals
def hyperparameters(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
    name: Optional[str] = typer.Option(
        None,
        "--name",
        help="Explain a single hyperparameter in detail, including the command that changes it.",
    ),
):
    """Show subnet hyperparameters."""
    app_ctx: AppContext = ctx_of(ctx)
    params = app_ctx.run(lambda c: fetch_hyperparameters(c, netuid))
    show_hyperparameters(app_ctx, netuid, params, name)


@app.command("burn-cost", rich_help_panel=PANEL_REGISTER)
@with_globals
def burn_cost(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
):
    """Show the current cost to register a hotkey on a subnet by burning TAO."""
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        if not await client.query(storage.SubtensorModule.NetworksAdded, [netuid]):
            return None
        return await client.subnets.burn(netuid)

    cost = app_ctx.run(_op)
    if cost is None:
        app_ctx.output.error(f"subnet {netuid} does not exist")
        raise typer.Exit(1)
    app_ctx.output.detail(None, {"burn_cost": cost, "tao": cost.tao})


@app.command("create-cost", rich_help_panel=PANEL_REGISTER)
@with_globals
def create_cost(ctx: typer.Context):
    """Show the current cost to register a new subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    cost = app_ctx.run(lambda c: c.read("subnet_registration_cost"))
    app_ctx.output.detail(None, {"create_cost": cost, "tao": cost.tao})


@app.command("price", rich_help_panel=PANEL_INSPECT)
@with_globals
def subnet_price(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
):
    """Show current spot alpha price for a subnet (TAO per alpha)."""
    app_ctx: AppContext = ctx_of(ctx)
    price = app_ctx.run(lambda c: c.read("alpha_price", netuid=netuid))
    app_ctx.output.detail(None, price)


def _conviction_eta(blocks: Optional[int]) -> str:
    """Human reading of a blocks-until-10%-threshold estimate."""
    if blocks is None:
        return "won't reach 10%"
    if blocks == 0:
        return "10% reached"
    seconds = int(blocks * BLOCKTIME)
    if seconds >= 86400:
        return f"10% in ~{seconds / 86400:.0f}d"
    if seconds >= 3600:
        return f"10% in ~{seconds / 3600:.0f}h"
    return f"10% in ~{max(1, seconds // 60)}m"


@app.command("conviction", rich_help_panel=PANEL_INSPECT)
@with_globals
def subnet_conviction(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help="Show conviction for this hotkey only: an ss58 address, address-book "
        "name, or a local hotkey name. Omit to list every locked hotkey.",
    ),
    show_all: bool = typer.Option(
        False, "--all", help="Also show negligible locks. JSON always includes every lock."
    ),
):
    """Show conviction for one hotkey, or every locked hotkey on a subnet.

    Without ``--hotkey``, lists each hotkey holding locked stake: its locked
    alpha, conviction, and the estimated time until its conviction reaches 10%
    of the subnet's outstanding alpha (the subnet-ownership threshold).
    """
    app_ctx: AppContext = ctx_of(ctx)
    if hotkey_ss58:
        hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        data = app_ctx.run(lambda c: c.read("hotkey_conviction", hotkey_ss58=hotkey, netuid=netuid))
        app_ctx.output.detail(f"conviction netuid {netuid}", data)
        return

    hotkey_names = local_address_names(app_ctx.wallet_path)

    async def _op(client):
        data = await client.read("subnet_convictions", netuid=netuid)
        unnamed = [
            entry["hotkey"] for entry in data["hotkeys"] if entry["hotkey"] not in hotkey_names
        ]
        return data, await chain_identity_names(client, unnamed)

    data, identity_names = app_ctx.run(_op)
    threshold: Balance = data["threshold_alpha"]
    records = [
        {
            "hotkey": entry["hotkey"],
            "is_owner": entry["is_owner"],
            "locked": str(entry["locked_alpha"]),
            "locked_alpha": entry["locked_alpha"].amount,
            "conviction": str(entry["conviction_alpha"]),
            "conviction_alpha": entry["conviction_alpha"].amount,
            "pct_of_threshold": entry["pct_of_threshold"],
            "blocks_to_threshold": entry["blocks_to_threshold"],
        }
        for entry in data["hotkeys"]
    ]
    json_payload = {
        "netuid": data["netuid"],
        "block": data["block"],
        "alpha_out": data["alpha_out"].amount,
        "threshold_alpha": threshold.amount,
        "total_locked_alpha": data["total_locked_alpha"].amount,
        "total_conviction_alpha": data["total_conviction_alpha"].amount,
        "total_blocks_to_threshold": data["total_blocks_to_threshold"],
        "unlock_rate": data["unlock_rate"],
        "maturity_rate": data["maturity_rate"],
        "owner_hotkey": data["owner_hotkey"],
        "registered_at": data["registered_at"],
        "ownership_changeable_at_block": data["ownership_changeable_at_block"],
        "hotkeys": records,
    }

    # Negligible: contributes under 0.01% of the ownership threshold (with an
    # absolute dust floor so tiny subnets still hide true dust).
    cutoff = max(threshold.rao // 10_000, 1_000_000)
    entries = data["hotkeys"]
    shown = (
        entries
        if show_all
        else [
            entry
            for entry in entries
            if entry["locked_alpha"].rao >= cutoff or entry["conviction_alpha"].rao >= cutoff
        ]
    )

    def _position(entry: dict) -> dict:
        hotkey = entry["hotkey"]
        pct = (
            f"= {entry['pct_of_threshold']:.2%} of threshold"
            if entry["pct_of_threshold"] is not None
            else "no threshold"
        )
        return {
            "hotkey": hotkey,
            "label": hotkey_names.get(hotkey) or identity_names.get(hotkey, hotkey),
            "named": hotkey in hotkey_names,
            "identity": hotkey not in hotkey_names and hotkey in identity_names,
            "note": "owner" if entry["is_owner"] else None,
            "locked": str(entry["locked_alpha"]),
            "conviction": str(entry["conviction_alpha"]),
            "detail": f"{pct} · {_conviction_eta(entry['blocks_to_threshold'])}",
        }

    total_pct = (
        f"= {data['total_conviction_alpha'].rao / threshold.rao:.1%} of threshold · "
        if threshold.rao
        else ""
    )
    total = {
        "locked": str(data["total_locked_alpha"]),
        "conviction": str(data["total_conviction_alpha"]),
        "summary": f"{total_pct}{_conviction_eta(data['total_blocks_to_threshold'])}",
    }

    def _halflife(rate: int) -> str:
        return f"{rate * math.log(2) * BLOCKTIME / 86400:.0f}d"

    now = data["block"]
    changeable_at = data["ownership_changeable_at_block"]
    if now >= changeable_at:
        age_value = "open"
        age_note = (
            f"registered at block {data['registered_at']:,} — the one-year age gate has passed"
        )
    else:
        days_away = (changeable_at - now) * BLOCKTIME / 86400
        age_value = f"block {changeable_at:,}"
        age_note = (
            f"~{days_away:.0f}d away — a subnet must be one year old before ownership can change"
        )
    parameters = [
        (
            "alpha out",
            str(data["alpha_out"]),
            "alpha in circulation on this subnet (grows with emissions)",
        ),
        (
            "takeover threshold",
            str(threshold),
            "10% of alpha out — total conviction must reach this before ownership can change",
        ),
        (
            "unlock rate",
            f"{data['unlock_rate']:,} blocks",
            f"a decaying lock's mass halves every ~{_halflife(data['unlock_rate'])}",
        ),
        (
            "maturity rate",
            f"{data['maturity_rate']:,} blocks",
            "conviction closes half its gap to the locked mass every "
            f"~{_halflife(data['maturity_rate'])}",
        ),
        ("age gate", age_value, age_note),
        (
            "owner hotkey",
            data["owner_hotkey"] or "—",
            "its lock always counts conviction equal to its full locked mass",
        ),
    ]

    explanation = [
        f"Anyone can lock alpha to a hotkey with `btcli lock add --netuid {netuid} "
        "--amount <alpha>`. Locked alpha cannot be unstaked, and it earns conviction: "
        "a score that grows toward the locked amount at the maturity rate. A default "
        "(decaying) lock also unlocks itself at the unlock rate, so its conviction "
        f"rises and then fades away; a perpetual lock (`btcli lock mode --netuid {netuid} "
        "--perpetual`) keeps the full amount locked forever and its conviction keeps "
        "climbing toward 100% of it.",
        "Once the subnet is a year old and the total conviction of all locks reaches "
        "the takeover threshold (10% of alpha out), the hotkey with the most conviction "
        "becomes the subnet owner hotkey and its owning coldkey takes over the subnet. "
        "The sitting owner's own lock always counts conviction equal to its full locked "
        "mass, so an owner defends by keeping more alpha locked than any challenger "
        "can mature.",
        "The times shown are estimates from current locks: they assume the rates and "
        "alpha out stay constant, while emissions actually keep increasing alpha out "
        "and push the threshold up over time.",
    ]

    app_ctx.output.conviction_list(
        f"conviction netuid {netuid}",
        total,
        [_position(entry) for entry in shown],
        parameters,
        explanation,
        json_payload,
    )
    hidden = len(entries) - len(shown)
    if hidden:
        app_ctx.output.message(
            f"{hidden} negligible lock{'s' if hidden > 1 else ''} hidden (pass `--all` to show)"
        )


@app.command("register", rich_help_panel=PANEL_REGISTER)
@with_tx_globals
def register_subnet(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=BurnedRegister.field_help("netuid")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Register a hotkey on a subnet by paying the registration cost.

    Pays the subnet's current floating registration cost from the wallet
    coldkey for a neuron slot (UID). When the subnet's collateral lock
    share is zero the full cost is burned/recycled; when it is positive,
    that share is staked and locked as miner collateral (released only
    through earned emission). Check the current cost with
    `btcli subnets burn-cost`.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(BurnedRegister(netuid=netuid, hotkey_ss58=hotkey_ss58))


@app.command("create", rich_help_panel=PANEL_REGISTER)
@with_tx_globals
def create_subnet(ctx: typer.Context):
    """Register a new subnet.

    Creates a new subnet owned by the wallet coldkey, charging the current
    subnet registration cost in TAO (see `btcli subnets create-cost`).
    The subnet does not emit until its owner activates it with
    `btcli sudo start`.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(RegisterSubnet())


@app.command("metagraph", rich_help_panel=PANEL_INSPECT)
@with_globals
def metagraph(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
):
    """Show metagraph data for a subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    graph = app_ctx.run(lambda c: c.read("metagraph", netuid=netuid))
    show_metagraph(app_ctx, netuid, graph)
