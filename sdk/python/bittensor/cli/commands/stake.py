"""`btcli stake`: query and manage stake."""

from __future__ import annotations

import asyncio
import json
from typing import Optional

import typer

from ...balance import Balance
from ...intents import SetAutoStake, SetChildkeyTake, SetChildren
from ...reads import StakePosition
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import (
    STAKE_LIST_TITLE,
    annotate_stake_groups_with_locks,
    chain_identity_names,
    coldkey_lock_context,
    combine_root_yields,
    dust_note,
    enrich_stake_records_with_locks,
    enrich_stake_records_with_root_yield,
    fetch_root_basket_owed_for_coldkeys,
    list_coldkeys,
    local_address_names,
    netuid_groups,
    root_staked,
    root_yield_record,
    split_dust,
)
from ..prompt import fill_missing, interactive
from ..stake_exit import mount_stake_exit_commands
from ..stake_picker import stake_source_spec
from ..tx import intent_command

app = typer.Typer(
    no_args_is_help=True,
    help=f"Query and manage stake.\n\nGuide: {guide_docs_url('staking')}",
)

PANEL_MOVE = "Add & move stake"
PANEL_POSITIONS = "Positions"
PANEL_AUTO = "Auto-stake"
PANEL_DELEGATION = "Delegation"

_NETUID_HELP = "Numeric identifier of the subnet the command operates on."

# The btcli-familiar verbs, mounted here as aliases of the generated intent
# commands (`tx add-stake` etc.) so both spellings stay in sync for free.
for _alias, _op in (
    ("add", "add_stake"),
    ("remove", "remove_stake"),
    ("move", "move_stake"),
    ("move-swap", "move_swap_stake"),
    ("transfer", "transfer_stake"),
    ("swap", "swap_stake"),
    ("burn", "stake_burn"),
):
    app.command(_alias, rich_help_panel=PANEL_MOVE)(intent_command(_op))

mount_stake_exit_commands(app, PANEL_MOVE)


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
    # The coldkey resolves first so a missing --hotkey can be picked from that
    # coldkey's live stake positions — the staked-to hotkey is usually someone
    # else's validator, not a local hotkey file (and a multisig owner has none).
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    if hotkey_ss58 is None and interactive(app_ctx):
        owner_label = coldkey_ss58 or f"wallet {app_ctx.wallet_name!r}"
        values: dict = {"hotkey_ss58": None, "netuid": netuid}
        fill_missing(
            app_ctx,
            [stake_source_spec("hotkey_ss58", "netuid", owner=(owner, owner_label))],
            values,
        )
        hotkey_ss58 = values["hotkey_ss58"]
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
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

    One table row per validator position — netuid, stake, locked, spot
    value, validator, hotkey — sorted by value, largest first. When a
    conviction lock is active, the subnet's locked mass shows on its
    largest position. JSON carries the same lock fields on each row.

    Root positions still list principal only. Accrued basket yield is a
    separate line under the total (``auto-claim is off → btcli root claim``).
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
            contexts, owed_by_ss58 = await asyncio.gather(
                asyncio.gather(
                    *[
                        coldkey_lock_context(client, ss58, valuations[ss58].positions)
                        for _, ss58 in coldkeys
                    ]
                ),
                fetch_root_basket_owed_for_coldkeys(client, ss58s),
            )
            lock_ctx = {ss58: ctx for (_, ss58), ctx in zip(coldkeys, contexts)}
            for locks_by_netuid, _ in lock_ctx.values():
                for lock in locks_by_netuid.values():
                    if lock["hotkey"] not in hotkey_names:
                        unnamed.append(lock["hotkey"])
            return (
                valuations,
                await chain_identity_names(client, unnamed),
                lock_ctx,
                owed_by_ss58,
            )

        valuations, identity_names, lock_ctx, owed_by_ss58 = app_ctx.run(_all)
        records = [
            _position_record(pos, valuations[ss58], {"wallet": name, "coldkey": ss58})
            for name, ss58 in coldkeys
            for pos in valuations[ss58].positions
        ]
        staked_by_ss58 = {ss58: root_staked(valuations[ss58].positions) for _, ss58 in coldkeys}
        groups = []
        for name, ss58 in coldkeys:
            locks_by_netuid, availability_by_netuid = lock_ctx[ss58]
            wallet_groups = netuid_groups(
                valuations[ss58].positions,
                valuations[ss58],
                hotkey_names,
                identity_names,
                {"wallet": name},
            )
            annotate_stake_groups_with_locks(
                wallet_groups,
                locks_by_netuid,
                availability_by_netuid,
                hotkey_names,
                identity_names,
            )
            wallet_records = [r for r in records if r.get("coldkey") == ss58]
            enrich_stake_records_with_locks(
                wallet_records,
                locks_by_netuid,
                availability_by_netuid,
            )
            groups.extend(wallet_groups)
        enrich_stake_records_with_root_yield(records, owed_by_ss58, staked_by_ss58)
        shown, dust = (groups, []) if show_dust else split_dust(groups)
        grand_total = Balance(sum(valuations[ss58].stake_value.rao for _, ss58 in coldkeys))
        app_ctx.output.stake_table(
            title,
            shown,
            records,
            grand_total,
            root_yield=combine_root_yields(
                [
                    (owed_by_ss58.get(ss58, Balance.from_rao(0)), staked_by_ss58[ss58])
                    for _, ss58 in coldkeys
                ]
            ),
        )
        if dust:
            app_ctx.output.message(dust_note(dust))
        return

    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _one(client):
        valuation = await client.read("stake_value_for_coldkey", coldkey_ss58=owner)
        unnamed = _unnamed(valuation.positions)
        (locks_by_netuid, availability_by_netuid), owed_by_ss58 = await asyncio.gather(
            coldkey_lock_context(client, owner, valuation.positions),
            fetch_root_basket_owed_for_coldkeys(client, [owner]),
        )
        for lock in locks_by_netuid.values():
            if lock["hotkey"] not in hotkey_names:
                unnamed.append(lock["hotkey"])
        return (
            valuation,
            await chain_identity_names(client, unnamed),
            locks_by_netuid,
            availability_by_netuid,
            owed_by_ss58,
        )

    valuation, identity_names, locks_by_netuid, availability_by_netuid, owed_by_ss58 = app_ctx.run(
        _one
    )
    records = [_position_record(pos, valuation) for pos in valuation.positions]
    staked = root_staked(valuation.positions)
    accrued = owed_by_ss58.get(owner, Balance.from_rao(0))
    groups = netuid_groups(valuation.positions, valuation, hotkey_names, identity_names)
    annotate_stake_groups_with_locks(
        groups, locks_by_netuid, availability_by_netuid, hotkey_names, identity_names
    )
    enrich_stake_records_with_locks(records, locks_by_netuid, availability_by_netuid)
    enrich_stake_records_with_root_yield(records, owed_by_ss58, {owner: staked}, default_ss58=owner)
    shown, dust = (groups, []) if show_dust else split_dust(groups)
    app_ctx.output.stake_table(
        title,
        shown,
        records,
        valuation.stake_value,
        root_yield=root_yield_record(accrued, staked),
    )
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
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(SetAutoStake(netuid=netuid, hotkey_ss58=hotkey))


child_app = typer.Typer(
    no_args_is_help=True,
    help=f"Child hotkey delegation.\n\nGuide: {guide_docs_url('staking')}",
)
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
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    try:
        parsed = json.loads(children)
    except json.JSONDecodeError as error:
        app_ctx.output.error(f"invalid --children JSON: {error}")
        raise typer.Exit(1)
    try:
        intent = SetChildren(netuid=netuid, children=parsed, hotkey_ss58=hotkey)
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
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(SetChildren(netuid=netuid, children=[], hotkey_ss58=hotkey))


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
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    try:
        intent = SetChildkeyTake(netuid=netuid, take=take, hotkey_ss58=hotkey)
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
            "move": "btcli stake move --origin-netuid N --destination-netuid M --all",
            "move-swap": "btcli stake move-swap --origin-netuid N --dest-netuid M --all",
            "transfer": "btcli stake transfer --origin-hotkey H1 --destination-hotkey H2 --all",
            "swap": "btcli stake swap --origin-netuid N --destination-netuid M --all",
            "units": "amounts are in the ORIGIN subnet's alpha (TAO when origin netuid is 0); "
            "pass `--all` or `--amount all` to move the whole position",
        },
    )
