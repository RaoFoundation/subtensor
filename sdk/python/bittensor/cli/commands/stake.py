"""`btcli stake`: query and manage stake."""

from __future__ import annotations

import json
from typing import Optional

import typer

from ...balance import Balance
from ...intents import ClaimRoot, SetAutoStake, SetChildkeyTake, SetChildren, SetRootClaimType
from ...reads import StakePosition
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import (
    STAKE_LIST_TITLE,
    chain_identity_names,
    dust_note,
    list_coldkeys,
    local_address_names,
    netuid_groups,
    split_dust,
)
from ..tx import intent_command

app = typer.Typer(no_args_is_help=True, help="Query and manage stake.")

PANEL_MOVE = "Add & move stake"
PANEL_POSITIONS = "Positions"
PANEL_AUTO = "Auto-stake & claims"
PANEL_DELEGATION = "Delegation"

_NETUID_HELP = "Numeric identifier of the subnet the command operates on."

# The btcli-familiar verbs, mounted here as aliases of the generated intent
# commands (`tx add-stake` etc.) so both spellings stay in sync for free.
for _alias, _op in (
    ("add", "add_stake"),
    ("remove", "remove_stake"),
    ("move", "move_stake"),
    ("transfer", "transfer_stake"),
    ("swap", "swap_stake"),
):
    app.command(_alias, rich_help_panel=PANEL_MOVE)(intent_command(_op))


@app.command(rich_help_panel=PANEL_POSITIONS)
@with_globals
def show(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Show stake held by a coldkey on a hotkey within a subnet (in the subnet's own currency)."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    stake = app_ctx.run(lambda c: c.staking.get(owner, hotkey, netuid))
    app_ctx.output.detail(
        None,
        {
            "coldkey": owner,
            "hotkey": hotkey,
            "netuid": netuid,
            "stake": stake,
            "stake_amount": stake.amount,
            "stake_unit": "TAO" if netuid == 0 else f"alpha (netuid {netuid})",
        },
    )


def _position_record(pos, valuation, extra: Optional[dict] = None) -> dict:
    value = valuation.spot_value(pos.stake)
    return {
        **(extra or {}),
        "netuid": pos.netuid,
        "hotkey": pos.hotkey,
        "stake": str(pos.stake),
        "stake_amount": pos.stake.amount,
        "stake_unit": "TAO" if pos.netuid == 0 else f"alpha (netuid {pos.netuid})",
        "value": str(value),
        "value_tao": value.tao,
        "registered": pos.is_registered,
    }


@app.command("list", rich_help_panel=PANEL_POSITIONS)
@with_globals
def stake_list(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    all_wallets: bool = typer.Option(False, "--all", "-a", help="List stake for every wallet."),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show dust subnets and positions (spot value < τ0.001). "
        "JSON always includes every position.",
    ),
):
    """List stake per subnet for a coldkey (or all wallets with --all).

    Each subnet shows its total with the per-hotkey breakdown beneath it;
    JSON output carries the flat per-position records (with registration
    status).
    """
    app_ctx: AppContext = ctx_of(ctx)
    title = STAKE_LIST_TITLE
    hotkey_names = local_address_names(app_ctx.wallet_path)

    def _unnamed(positions: list[StakePosition]) -> list[str]:
        return [p.hotkey for p in positions if p.hotkey not in hotkey_names]

    if all_wallets:
        coldkeys = list_coldkeys(app_ctx.wallet_path)

        async def _all(client):
            ss58s = [ss58 for _, ss58 in coldkeys]
            valuations = await client.read("stake_value_for_coldkeys", coldkey_ss58s=ss58s)
            unnamed = [hk for v in valuations.values() for hk in _unnamed(v.positions)]
            return valuations, await chain_identity_names(client, unnamed)

        valuations, identity_names = app_ctx.run(_all)
        records = [
            _position_record(pos, valuations[ss58], {"wallet": name, "coldkey": ss58})
            for name, ss58 in coldkeys
            for pos in valuations[ss58].positions
        ]
        groups = [
            group
            for name, ss58 in coldkeys
            for group in netuid_groups(
                valuations[ss58].positions,
                valuations[ss58],
                hotkey_names,
                identity_names,
                {"wallet": name},
            )
        ]
        shown, dust = (groups, []) if show_dust else split_dust(groups)
        grand_total = Balance(sum(valuations[ss58].stake_value.rao for _, ss58 in coldkeys))
        app_ctx.output.stake_list(title, shown, records, grand_total)
        if dust:
            app_ctx.output.message(dust_note(dust))
        return

    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _one(client):
        valuation = await client.read("stake_value_for_coldkey", coldkey_ss58=owner)
        return valuation, await chain_identity_names(client, _unnamed(valuation.positions))

    valuation, identity_names = app_ctx.run(_one)
    records = [_position_record(pos, valuation) for pos in valuation.positions]
    groups = netuid_groups(valuation.positions, valuation, hotkey_names, identity_names)
    shown, dust = (groups, []) if show_dust else split_dust(groups)
    app_ctx.output.stake_list(title, shown, records, valuation.stake_value)
    if dust:
        app_ctx.output.message(dust_note(dust))


@app.command("auto", rich_help_panel=PANEL_AUTO)
@with_globals
def auto_stake_list(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """List auto-stake destinations for a coldkey."""
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    known_names = local_address_names(app_ctx.wallet_path)

    async def _fetch(client):
        rows = await client.read("auto_stake_all", coldkey_ss58=owner)
        unnamed = [r["hotkey"] for r in rows if r["hotkey"] not in known_names]
        return rows, await chain_identity_names(client, unnamed)

    rows, identity_names = app_ctx.run(_fetch)
    if not rows:
        app_ctx.output.detail("auto-stake", {"destinations": []})
        return
    labels = {**identity_names, **known_names}
    table_rows = [[r["netuid"], labels.get(r["hotkey"], r["hotkey"])] for r in rows]
    app_ctx.output.table("auto-stake destinations", ["netuid", "hotkey"], table_rows, rows)


@app.command("set-auto", rich_help_panel=PANEL_AUTO)
@with_tx_globals
def set_auto_stake(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetAutoStake.field_help("netuid")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Set auto-stake destination for a subnet.

    All future rewards the wallet coldkey earns on the subnet are then
    staked automatically to the chosen hotkey (the wallet's own hotkey by
    default) instead of accruing unstaked.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(SetAutoStake(netuid=netuid, hotkey_ss58=hotkey_ss58))


@app.command("set-claim", rich_help_panel=PANEL_AUTO)
@with_tx_globals
def set_claim_type(
    ctx: typer.Context,
    claim_type: str = typer.Option(
        ..., "--claim-type", help=SetRootClaimType.field_help("claim_type")
    ),
    subnets: Optional[str] = typer.Option(
        None,
        "--subnets",
        help="Comma-separated netuids to keep alpha on; required only when "
        "--claim-type is KeepSubnets.",
    ),
):
    """Set root claim type for the wallet coldkey.

    Controls how the coldkey's root-stake dividends are paid out: swapped
    to TAO (Swap, the default), kept as subnet alpha (Keep), or kept as
    alpha only on the listed subnets (KeepSubnets).
    """
    app_ctx: AppContext = ctx_of(ctx)
    subnet_list = None
    if subnets:
        subnet_list = [int(part.strip()) for part in subnets.split(",") if part.strip()]
    app_ctx.submit(SetRootClaimType(claim_type=claim_type, subnets=subnet_list))


@app.command("process-claim", rich_help_panel=PANEL_AUTO)
@with_tx_globals
def process_claim(
    ctx: typer.Context,
    subnets: str = typer.Option(
        ...,
        "--subnets",
        help="Comma-separated netuids to claim accumulated root dividends from.",
    ),
):
    """Claim accumulated root dividends from subnets.

    Pays out the dividends accrued to the wallet coldkey on each listed
    subnet, applying the coldkey's root claim type (see
    `btcli stake set-claim`).
    """
    app_ctx: AppContext = ctx_of(ctx)
    subnet_list = [int(part.strip()) for part in subnets.split(",") if part.strip()]
    app_ctx.submit(ClaimRoot(subnets=subnet_list))


child_app = typer.Typer(no_args_is_help=True, help="Child hotkey delegation.")
app.add_typer(child_app, name="child", rich_help_panel=PANEL_DELEGATION)
app.add_typer(child_app, name="children", hidden=True)


@child_app.command("get")
@with_globals
def child_get(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Show child hotkeys assigned to a parent hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    children = app_ctx.run(lambda c: c.read("children", hotkey_ss58=hotkey, netuid=netuid))
    app_ctx.output.detail("children", {"hotkey": hotkey, "netuid": netuid, "children": children})


@child_app.command("set")
@with_tx_globals
def child_set(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetChildren.field_help("netuid")),
    children: str = typer.Option(
        ...,
        "--children",
        help="JSON list of proportion/hotkey pairs: each entry is a two-element "
        "array holding a proportion (0..1 fraction like 0.5, or the raw u64 "
        "share of u64 max) and the child hotkey's ss58 address.",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Assign child hotkeys with proportions.

    Delegates the given shares of the parent hotkey's stake weight to the
    child hotkeys on the subnet, replacing any existing child list. The
    chain rate-limits how often the child list can change.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        parsed = json.loads(children)
    except json.JSONDecodeError as error:
        app_ctx.output.error(f"invalid --children JSON: {error}")
        raise typer.Exit(1)
    try:
        intent = SetChildren(netuid=netuid, children=parsed, hotkey_ss58=hotkey_ss58)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)
    app_ctx.submit(intent)


@child_app.command("revoke")
@with_tx_globals
def child_revoke(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetChildren.field_help("netuid")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Revoke all child hotkeys on a subnet.

    Clears the parent hotkey's child list so it regains its full stake
    weight. Subject to the same rate limit as `btcli stake child set`.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(SetChildren(netuid=netuid, children=[], hotkey_ss58=hotkey_ss58))


@child_app.command("take")
@with_tx_globals
def child_take(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetChildkeyTake.field_help("netuid")),
    take: str = typer.Option(..., "--take", help=SetChildkeyTake.field_help("take")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Set childkey take for a hotkey.

    The take is the fraction of rewards the child hotkey keeps from stake
    weight delegated to it by parents: a 0..1 fraction with a decimal point
    (e.g. 0.09) or the raw u16 proportion (0 to 65535).
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        intent = SetChildkeyTake(netuid=netuid, take=take, hotkey_ss58=hotkey_ss58)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)
    app_ctx.submit(intent)


@app.command("wizard", rich_help_panel=PANEL_MOVE)
@with_globals
def stake_wizard(ctx: typer.Context):
    """Interactive stake movement guide (use `btcli stake move`, `transfer`, or `swap`)."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.detail(
        "stake wizard",
        {
            "move": "btcli stake move --origin-netuid N --destination-netuid M --amount-alpha X",
            "transfer": "btcli stake transfer --origin-hotkey H1 --destination-hotkey H2 ...",
            "swap": "btcli stake swap --origin-netuid N --destination-netuid M --amount-alpha X",
            "units": "amounts are in the ORIGIN subnet's alpha (TAO when origin netuid is 0)",
        },
    )
