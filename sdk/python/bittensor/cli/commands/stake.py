"""`btcli stake`: query and manage stake."""

from __future__ import annotations

import json
from typing import Optional

import typer

from ... import config as cfg
from ...balance import Balance
from ...intents import (
    Batch,
    ClaimRoot,
    RemoveStake,
    SetAutoStake,
    SetChildkeyTake,
    SetChildren,
    SetRootClaimType,
)
from ...intents.proxy import ProxyTypeChoice
from ...intents.staking import DEFAULT_RATE_TOLERANCE
from ...reads import StakePosition
from ...settings import tx_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import PANEL_EXECUTION, with_globals, with_tx_globals
from ..helpers import (
    STAKE_LIST_TITLE,
    chain_identity_names,
    dust_note,
    list_coldkeys,
    local_address_names,
    netuid_groups,
    split_dust,
)
from ..prompt import PromptSpec, fill_missing, interactive
from ..stake_picker import stake_source_spec
from ..tx import _parse_money, _resolve_proxy_options, intent_command

app = typer.Typer(no_args_is_help=True, help="Query and manage stake.")

PANEL_MOVE = "Add & move stake"
PANEL_POSITIONS = "Positions"
PANEL_AUTO = "Auto-stake & claims"
PANEL_DELEGATION = "Delegation"

_NETUID_HELP = "Numeric identifier of the subnet the command operates on."
_, _, _REMOVE_STAKE_BODY = RemoveStake.describe().partition("\n")
_REMOVE_STAKE_HELP = (
    "Unstake one or more alpha positions back to the coldkey.\n\n"
    f"{_REMOVE_STAKE_BODY.strip()}\n\n"
    "Use `--hotkey` for one position, `--include-hotkeys` for a comma-separated "
    "selection, or `--all-hotkeys` with optional exclusions. Omit `--netuid` "
    "to unstake matching positions on every subnet. Multiple removals execute "
    f"atomically.\n\nDocs: {tx_docs_url(RemoveStake.op)}"
)

# The btcli-familiar verbs, mounted here as aliases of the generated intent
# commands (`tx add-stake` etc.) so both spellings stay in sync for free.
for _alias, _op in (
    ("add", "add_stake"),
    ("move", "move_stake"),
    ("transfer", "transfer_stake"),
    ("swap", "swap_stake"),
):
    app.command(_alias, rich_help_panel=PANEL_MOVE)(intent_command(_op))


def _hotkey_refs(raw: Optional[str]) -> list[str]:
    """Parse a comma-separated hotkey selector, preserving order and removing duplicates."""
    return list(dict.fromkeys(part.strip() for part in (raw or "").split(",") if part.strip()))


def _resolve_hotkeys(app_ctx: AppContext, refs: list[str]) -> list[str]:
    resolved = [app_ctx.resolve_address("hotkey_ss58", ref) for ref in refs]
    return list(dict.fromkeys(address for address in resolved if address is not None))


def _staked_positions(app_ctx: AppContext, owner: str) -> list[StakePosition]:
    positions = app_ctx.run(lambda client: client.read("stake_for_coldkey", coldkey_ss58=owner))
    return [position for position in positions if position.stake.rao > 0]


def _selection_owner(app_ctx: AppContext, proxy_for: Optional[str]) -> str:
    """Coldkey whose live stake is used by the multi-position selectors."""
    if proxy_for not in (None, "self"):
        owner = app_ctx.resolve_address("proxy_for", proxy_for)
    elif app_ctx.uses_external_signer():
        signer_ref = app_ctx.signer_address or cfg.get("signer_address")
        if signer_ref:
            owner = app_ctx.resolve_address("coldkey_ss58", str(signer_ref))
        elif app_ctx.uses_ledger_signer():
            owner = app_ctx.ledger_signer().ss58_address
        elif app_ctx.uses_vault_signer():
            owner = app_ctx.vault_signer().ss58_address
        else:
            app_ctx.output.error(
                "cannot discover stake positions before selecting the extension account",
                help="pass --signer-address, --proxy-for, or --netuid with explicit hotkeys",
            )
            raise typer.Exit(2)
    else:
        owner = app_ctx.resolve_address("coldkey_ss58", None)
    if owner is None:
        app_ctx.output.error("could not resolve the coldkey whose stake should be removed")
        raise typer.Exit(2)
    return owner


def _position_pairs(positions: list[StakePosition], hotkeys: list[str]) -> list[tuple[str, int]]:
    """Match selected hotkeys to their non-zero live positions, preserving selector order."""
    return [
        (hotkey, position.netuid)
        for hotkey in hotkeys
        for position in positions
        if position.hotkey == hotkey
    ]


@app.command("remove", rich_help_panel=PANEL_MOVE, help=_REMOVE_STAKE_HELP)
@with_tx_globals
def remove_stake(
    ctx: typer.Context,
    netuid: Optional[int] = typer.Option(
        None,
        "--netuid",
        help="Restrict unstaking to this subnet. Omit it to use every matching staked position.",
    ),
    amount_alpha: Optional[str] = typer.Option(
        None,
        "--amount-alpha",
        "--amount",
        "-a",
        help=(
            f"{RemoveStake.field_help('amount_alpha')} Amount in the subnet's alpha. "
            "The same amount is removed from every selected position."
        ),
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
    include_hotkeys: Optional[str] = typer.Option(
        None,
        "--include-hotkeys",
        "-in",
        help="Comma-separated hotkeys to unstake from; each accepts an ss58 address, "
        "address-book name, or local hotkey name.",
    ),
    all_hotkeys: bool = typer.Option(
        False,
        "--all-hotkeys",
        help=(
            "Unstake from every non-zero position owned by the coldkey, restricted "
            "to --netuid when provided."
        ),
    ),
    exclude_hotkeys: Optional[str] = typer.Option(
        None,
        "--exclude-hotkeys",
        "-ex",
        help="Comma-separated hotkeys to skip when using --all-hotkeys.",
    ),
    slippage_protection: bool = typer.Option(
        True,
        "--slippage-protection/--no-slippage-protection",
        help=RemoveStake.field_help("slippage_protection"),
    ),
    rate_tolerance: float = typer.Option(
        DEFAULT_RATE_TOLERANCE,
        "--rate-tolerance",
        help=RemoveStake.field_help("rate_tolerance"),
    ),
    proxy_for: Optional[str] = typer.Option(
        None,
        "--proxy-for",
        help="Dispatch as this account via Proxy.proxy; pass `self` to bypass a "
        "configured default.",
        rich_help_panel=PANEL_EXECUTION,
    ),
    force_proxy_type: Optional[ProxyTypeChoice] = typer.Option(
        None,
        "--force-proxy-type",
        help="Require this exact proxy type to be used (with --proxy-for).",
        rich_help_panel=PANEL_EXECUTION,
    ),
):
    """Adapt the single-position intent to the familiar multi-selector CLI."""
    app_ctx: AppContext = ctx_of(ctx)
    included_hotkeys = _hotkey_refs(include_hotkeys)
    excluded_hotkeys = _hotkey_refs(exclude_hotkeys)
    selected_modes = sum(
        (
            hotkey_ss58 is not None,
            bool(included_hotkeys),
            all_hotkeys,
        )
    )
    if selected_modes > 1:
        app_ctx.output.error(
            "choose only one of `--hotkey`, `--include-hotkeys`, or `--all-hotkeys`"
        )
        raise typer.Exit(2)
    if excluded_hotkeys and not all_hotkeys:
        app_ctx.output.error("`--exclude-hotkeys` requires `--all-hotkeys`")
        raise typer.Exit(2)

    selection_proxy_for = proxy_for
    if selection_proxy_for is None:
        configured_proxy = cfg.get("proxy_for")
        selection_proxy_for = str(configured_proxy) if configured_proxy else None
    if (
        selected_modes == 0
        and not app_ctx.assume_yes
        and not app_ctx.uses_external_signer()
        and interactive(app_ctx)
    ):
        values = {
            "hotkey_ss58": None,
            "netuid": netuid,
            "proxy_for": selection_proxy_for,
        }
        fill_missing(
            app_ctx,
            [stake_source_spec("hotkey_ss58", "netuid")],
            values,
        )
        hotkey_ss58 = values["hotkey_ss58"]
        netuid = values["netuid"]

    amount_values = {"amount_alpha": amount_alpha}
    if amount_alpha is None:
        fill_missing(
            app_ctx,
            [
                PromptSpec(
                    field="amount_alpha",
                    flag="--amount-alpha",
                    help=RemoveStake.field_help("amount_alpha"),
                    parse=lambda _app_ctx, raw: _parse_money(raw, True),
                    placeholder="number or all",
                )
            ],
            amount_values,
        )
        amount_alpha = amount_values["amount_alpha"]

    assert amount_alpha is not None
    try:
        amount = _parse_money(amount_alpha, True)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount-alpha`: {error}")
        raise typer.Exit(2)

    resolved_proxy_for, resolved_proxy_type = _resolve_proxy_options(
        app_ctx, proxy_for, force_proxy_type
    )

    live_positions: Optional[list[StakePosition]] = None
    if all_hotkeys or netuid is None:
        owner = _selection_owner(app_ctx, selection_proxy_for)
        live_positions = _staked_positions(app_ctx, owner)

    selected_pairs: list[tuple[str, int]]
    if all_hotkeys:
        assert live_positions is not None
        excluded = set(_resolve_hotkeys(app_ctx, excluded_hotkeys))
        selected_pairs = [
            (position.hotkey, position.netuid)
            for position in live_positions
            if position.hotkey not in excluded and (netuid is None or position.netuid == netuid)
        ]
    elif included_hotkeys:
        selected = _resolve_hotkeys(app_ctx, included_hotkeys)
        if netuid is None:
            assert live_positions is not None
            selected_pairs = _position_pairs(live_positions, selected)
        else:
            selected_pairs = [(hotkey, netuid) for hotkey in selected]
    else:
        resolved = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        if resolved is None:
            selected_pairs = []
        elif netuid is None and live_positions is not None:
            selected_pairs = _position_pairs(live_positions, [resolved])
        else:
            assert netuid is not None
            selected_pairs = [(resolved, netuid)]

    selected_pairs = list(dict.fromkeys(selected_pairs))
    if not selected_pairs:
        where = f" on netuid {netuid}" if netuid is not None else ""
        app_ctx.output.error(
            f"no selected hotkeys have stake{where}",
            help="`btcli stake list` shows every position",
        )
        raise typer.Exit(1)

    removals = [
        RemoveStake(
            hotkey_ss58=hotkey,
            netuid=position_netuid,
            amount_alpha=amount,
            slippage_protection=slippage_protection,
            rate_tolerance=rate_tolerance,
        )
        for hotkey, position_netuid in selected_pairs
    ]
    intent = removals[0] if len(removals) == 1 else Batch(intents=removals)
    app_ctx.submit(
        intent,
        proxy_for=resolved_proxy_for,
        force_proxy_type=resolved_proxy_type,
    )


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
