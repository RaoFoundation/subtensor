"""`btcli subnets`: list subnets and inspect one."""

from __future__ import annotations

import asyncio
import math
from typing import Optional

import typer

from ..._generated import storage
from ...balance import Balance
from ...intents import BurnedRegister, RegisterSubnet, RootRegister
from ...settings import BLOCKTIME, guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import chain_identity_names, list_coldkeys, local_address_names
from ..hyperparams_view import fetch_hyperparameters, show_hyperparameters
from ..metagraph_view import show_metagraph
from ..prompt import record_sdk_hint

app = typer.Typer(
    no_args_is_help=True,
    help=f"Inspect subnets.\n\nGuide: {guide_docs_url('subnets')}",
)

PANEL_INSPECT = "Inspect"
PANEL_REGISTER = "Registration"

_NETUID_HELP = "Numeric identifier of the subnet."

# SubnetIdentitiesV3 field -> short label. ``subnet_name`` is the overview
# ``name`` row, not repeated here.
_IDENTITY_LINKS = (
    ("github_repo", "github"),
    ("subnet_url", "url"),
    ("discord", "discord"),
    ("subnet_contact", "contact"),
    ("description", "description"),
    ("logo_url", "logo"),
    ("additional", "additional"),
)


@app.command("list", rich_help_panel=PANEL_INSPECT)
@with_globals
def list_subnets(ctx: typer.Context):
    """List all subnets, highest emission first: name, spot alpha price,
    emission, and burn. JSON records also carry tempo and neuron slots."""
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        infos, names, prices, max_uids, emissions, tao_flows = await asyncio.gather(
            client.subnets.all(),
            client.read("subnet_names"),
            client.read("alpha_prices"),
            client.query_map(storage.SubtensorModule.MaxAllowedUids),
            client.query_map(storage.SubtensorModule.SubnetTaoInEmission),
            client.read("subnet_tao_flows"),
        )
        return (
            infos,
            names,
            prices,
            {int(k): int(v) for k, v in max_uids},
            {int(k): int(v) for k, v in emissions},
            tao_flows,
        )

    infos, names, prices, max_uids, emissions, tao_flows = app_ctx.run(_op)
    app_ctx.output.update_subnet_names(names)

    blocks_per_day = round(86400 / BLOCKTIME)
    infos = sorted(infos, key=lambda i: (-emissions.get(i.netuid, 0), i.netuid))
    rows = []
    records = []
    for i in infos:
        price = prices.get(i.netuid)
        max_n = max_uids.get(i.netuid)
        emission_per_day = Balance.from_rao(emissions.get(i.netuid, 0) * blocks_per_day).tao
        flow = tao_flows.get(i.netuid)
        flow_per_day = flow * blocks_per_day / 10**9 if flow is not None else None
        rows.append(
            {
                "netuid": i.netuid,
                # Root has no alpha pool; its "price" is identically 1 TAO
                # and no TAO is injected into it.
                "price": f"{price:.6f}" if i.netuid != 0 and price is not None else "—",
                "emission": f"{emission_per_day:,.2f}" if i.netuid != 0 else "—",
                "flow": f"{flow_per_day:+,.2f}"
                if i.netuid != 0 and flow_per_day is not None
                else "—",
                "burn": i.burn,
            }
        )
        records.append(
            {
                "netuid": i.netuid,
                "name": names.get(i.netuid),
                "symbol": app_ctx.output.unit(i.netuid),
                "price_tao_per_alpha": price if i.netuid != 0 else None,
                "emission_tao_per_day": emission_per_day if i.netuid != 0 else None,
                "tao_flow_tao_per_day": flow_per_day if i.netuid != 0 else None,
                "tempo": i.tempo,
                "burn_tao": i.burn.tao,
                "neurons": i.neuron_count,
                "max_neurons": max_n,
                "url": app_ctx.output.subnet_url(i.netuid),
            }
        )
    total_neurons = sum(i.neuron_count for i in infos)
    footer = f"[dim]{len(infos)} subnets  ·  {total_neurons:,} neurons  ·  sorted by emission[/dim]"
    app_ctx.output.subnet_list(
        "subnets",
        rows,
        records,
        footer=footer,
        legend=[
            (
                "netuid",
                "clickable — opens the subnet's page on taomarketcap.com."
                if app_ctx.output.hyperlinks
                else "Cmd+double-click the URL to open the subnet's page.",
            ),
            ("price (τ)", "spot price of one alpha token in TAO (root has no alpha pool)."),
            ("emission (τ/day)", "TAO injected into the subnet per day at the current rate."),
            (
                "flow (τ/day)",
                "smoothed net TAO flow from staking: positive means TAO is "
                "entering the subnet, negative means it is leaving.",
            ),
            ("burn", "TAO burned to register a neuron on the subnet right now."),
            ("trailing", "the subnet's alpha token symbol."),
        ],
    )


@app.command(rich_help_panel=PANEL_INSPECT)
@with_globals
def show(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
):
    """Show one subnet: owner, registration, identity, tempo, burn, and neurons."""
    app_ctx: AppContext = ctx_of(ctx)
    local_names = local_address_names(app_ctx.wallet_path)
    for wallet_name, ss58 in list_coldkeys(app_ctx.wallet_path):
        local_names.setdefault(ss58, wallet_name)

    async def _op(client):
        (
            exists,
            info,
            identity,
            owner,
            owner_hotkey,
            registered_at,
            max_uids,
            block,
        ) = await asyncio.gather(
            client.query(storage.SubtensorModule.NetworksAdded, [netuid]),
            client.subnets.info(netuid),
            client.read("subnet_identity", netuid=netuid),
            client.query(storage.SubtensorModule.SubnetOwner, [netuid]),
            client.query(storage.SubtensorModule.SubnetOwnerHotkey, [netuid]),
            client.query(storage.SubtensorModule.NetworkRegisteredAt, [netuid]),
            client.query(storage.SubtensorModule.MaxAllowedUids, [netuid]),
            client.block(),
        )
        if not exists:
            return None
        return {
            "info": info,
            "identity": identity if isinstance(identity, dict) else None,
            "owner": str(owner) if owner else None,
            "owner_hotkey": str(owner_hotkey) if owner_hotkey else None,
            "registered_at": int(registered_at or 0),
            "max_uids": int(max_uids) if max_uids else None,
            "block": int(block),
        }

    data = app_ctx.run(_op)
    if data is None:
        app_ctx.output.error(f"subnet {netuid} does not exist")
        raise typer.Exit(1)

    info = data["info"]
    identity = data["identity"]
    owner = data["owner"]
    owner_hotkey = data["owner_hotkey"]
    registered_at = data["registered_at"]
    name = (identity or {}).get("subnet_name") or None
    if name:
        app_ctx.output.update_subnet_names({netuid: name})
    if owner:
        app_ctx.output.classify_address(owner, "coldkey")
        if owner in local_names:
            app_ctx.output.name_address(owner, local_names[owner])
    if owner_hotkey:
        app_ctx.output.classify_address(owner_hotkey, "hotkey")
        if owner_hotkey in local_names:
            app_ctx.output.name_address(owner_hotkey, local_names[owner_hotkey])

    neurons = (
        f"{info.neuron_count}/{data['max_uids']}" if data["max_uids"] else str(info.neuron_count)
    )
    overview: list[tuple[str, str, Optional[str]]] = []
    if name:
        overview.append(("name", name, None))
    overview += [
        ("owner", owner or "—", local_names.get(owner)),
        ("owner hotkey", owner_hotkey or "—", local_names.get(owner_hotkey)),
        ("registered", f"block {registered_at:,}", _registered_note(registered_at, data["block"])),
        ("tempo", str(info.tempo), None),
        ("burn", str(info.burn), None),
        ("neurons", neurons, None),
    ]
    links = [
        (label, str(identity[key]), None)
        for key, label in _IDENTITY_LINKS
        if identity and identity.get(key)
    ]
    app_ctx.output.kv_sections(
        f"subnet {netuid}",
        [
            (None, overview),
            ("identity", links),
        ],
        {
            "netuid": netuid,
            "name": name,
            "symbol": app_ctx.output.unit(netuid),
            "owner_coldkey": owner,
            "owner_hotkey": owner_hotkey,
            "registered_at": registered_at,
            "tempo": info.tempo,
            "burn_tao": info.burn.tao,
            "neurons": info.neuron_count,
            "max_neurons": data["max_uids"],
            "identity": identity,
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


# Hidden short alias, matching the hidden group aliases in main.py.
app.command("hparams", hidden=True)(hyperparameters)


@app.command("burn-cost", rich_help_panel=PANEL_REGISTER)
@with_globals
def burn_cost(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
):
    """Show the current cost to register a hotkey on a subnet by burning TAO.

    Netuid 0 shows the root network's registration price.
    """
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
    record_sdk_hint(f"sub.prices.alpha_price(netuid={netuid})")
    app_ctx.output.detail(None, price)


def _registered_note(registered_at: int, block: int) -> Optional[str]:
    """Age of a registration block, or None when the chain has not moved past it."""
    if block <= registered_at:
        return None
    seconds = int((block - registered_at) * BLOCKTIME)
    if seconds >= 365 * 86400:
        return f"~{seconds / (365 * 86400):.1f}y ago"
    if seconds >= 86400:
        return f"~{seconds / 86400:.0f}d ago"
    if seconds >= 3600:
        return f"~{seconds / 3600:.0f}h ago"
    if seconds >= 60:
        return f"~{max(1, seconds // 60)}m ago"
    return "just now"


def _conviction_eta(blocks: Optional[int]) -> str:
    """Human reading of a blocks-until-18%-gate estimate for one hotkey."""
    if blocks is None:
        return "won't clear 18%"
    if blocks == 0:
        return "above 18%"
    seconds = int(blocks * BLOCKTIME)
    if seconds >= 86400:
        return f"18% in ~{seconds / 86400:.0f}d"
    if seconds >= 3600:
        return f"18% in ~{seconds / 3600:.0f}h"
    return f"18% in ~{max(1, seconds // 60)}m"


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
    alpha, conviction, and the estimated time until its own conviction exceeds
    18% of the subnet's eligible alpha — the single-hotkey gate that reassigns
    subnet ownership.
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
        "leader_hotkey": data["leader_hotkey"],
        "leader_blocks_to_threshold": data["leader_blocks_to_threshold"],
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

    leader_hotkey = data["leader_hotkey"]
    leader_label = (
        hotkey_names.get(leader_hotkey) or identity_names.get(leader_hotkey, leader_hotkey)
        if leader_hotkey
        else None
    )
    leader_summary = (
        f"leader {leader_label}: {_conviction_eta(data['leader_blocks_to_threshold'])}"
        if leader_hotkey
        else "no locks yet"
    )
    total = {
        "locked": str(data["total_locked_alpha"]),
        "conviction": str(data["total_conviction_alpha"]),
        "summary": f"context only — the gate is per-hotkey · {leader_summary}",
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
            "18% of eligible alpha — one hotkey's own conviction must exceed this "
            "before ownership can change",
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
        f"Lock alpha to a hotkey with `btcli conviction add --netuid {netuid} --amount "
        "<alpha>` to earn conviction — the score that gates subnet ownership. The "
        "times shown are estimates that assume the rates and alpha accounting stay "
        f"constant. Full mechanics: {guide_docs_url('conviction')}",
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
    through earned emission). The confirm prompt shows the current burn
    and lock split. Check the current cost with
    `btcli subnets burn-cost`.

    Netuid 0 registers on the root network: the cost is fully recycled, no
    prior stake is needed, and a full root network prunes its lowest-staked
    member to make room.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    if netuid == 0:
        app_ctx.submit(RootRegister(hotkey_ss58=hotkey))
    else:
        app_ctx.submit(BurnedRegister(netuid=netuid, hotkey_ss58=hotkey))


@app.command("create", rich_help_panel=PANEL_REGISTER)
@with_tx_globals
def create_subnet(ctx: typer.Context):
    """Register a new subnet.

    Creates a new subnet owned by the wallet coldkey, charging the current
    subnet registration cost in TAO (see `btcli subnets create-cost`).
    The subnet does not emit until its owner activates it with
    `btcli hparams start`.
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
