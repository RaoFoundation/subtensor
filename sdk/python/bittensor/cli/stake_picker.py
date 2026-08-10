"""Interactive stake-source selection for unstake-style tx commands.

When a command that draws from existing stake (remove/move/swap/transfer,
unstake-all) is missing its source hotkey, blindly defaulting to the wallet's
own hotkey is wrong — the stake usually sits on other people's hotkeys, and the
wallet may not even have a hotkey file. Instead (btcli-style, but numbered and
valued) the coldkey's live positions are listed and the answer picks one. For
netuid-scoped ops the subnet is asked first — only the subnets actually holding
stake are offered — so the position list covers just that subnet.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Callable, Optional

import typer
from rich.console import Console
from rich.text import Text

from ..balance import Balance
from ..reads import StakePosition, StakeValuation
from .context import AppContext, address_cli_name
from .helpers import chain_identity_names, local_address_names
from .output import STYLE_COMMAND, STYLE_HINT, STYLE_KEY, STYLE_NAME, Output
from .prompt import PromptSpec

# Positions whose spot value is below this are dust: hidden from the pickers
# (unless everything is dust) but still selectable by typing the netuid/hotkey.
_DUST_RAO = 1_000_000  # τ0.001

# op -> (hotkey field the stake comes from, netuid field naming its subnet).
# A None netuid field means the op spans every subnet the hotkey is staked on.
STAKE_SOURCE_FIELDS: dict[str, tuple[str, Optional[str]]] = {
    "remove_stake": ("hotkey_ss58", "netuid"),
    "remove_stake_limit": ("hotkey_ss58", "netuid"),
    "unstake_all": ("hotkey_ss58", None),
    "unstake_all_alpha": ("hotkey_ss58", None),
    "swap_stake": ("hotkey_ss58", "origin_netuid"),
    "transfer_stake": ("hotkey_ss58", "origin_netuid"),
    "move_stake": ("origin_hotkey_ss58", "origin_netuid"),
}


def stake_source_spec(hotkey_field: str, netuid_field: Optional[str]) -> PromptSpec:
    """A PromptSpec whose custom flow picks the source hotkey from live stake."""
    return PromptSpec(
        field=hotkey_field,
        flag=address_cli_name(hotkey_field),
        help=None,
        parse=lambda _app_ctx, raw: raw,  # unused: the custom flow does everything
        custom=lambda console, app_ctx, kwargs: _pick(
            console, app_ctx, kwargs, hotkey_field, netuid_field
        ),
    )


@dataclass
class _Row:
    hotkey: str
    netuid: Optional[int]  # None: aggregate across subnets (unstake-all ops)
    where: str  # "netuid 19" or "3 subnets"
    stake: str  # amount in the position's own currency (or spot total)
    value: Balance  # spot TAO, for sorting and the ≈ column
    show_value: bool  # alpha positions show their spot TAO next to the amount
    note: str = ""  # e.g. lock-on-other-hotkey hint


def _pick(
    console: Console,
    app_ctx: AppContext,
    kwargs: dict,
    hotkey_field: str,
    netuid_field: Optional[str],
) -> list[str]:
    """List the owner's stake, ask which position, and fill the answer into kwargs.

    For netuid-scoped ops the subnet is asked first (offering only the subnets
    that actually hold stake), so the position list covers just that subnet.
    Returns the argv tokens for the skip-the-prompts hint.
    """
    owner, owner_label = _stake_owner(app_ctx, kwargs)
    netuid = kwargs.get(netuid_field) if netuid_field else None

    async def _fetch(client):
        valuation = await client.read("stake_value_for_coldkey", coldkey_ss58=owner)
        positions = [p for p in valuation.positions if p.stake.rao > 0]
        names = local_address_names(app_ctx.wallet_path)
        unnamed = [p.hotkey for p in positions if p.hotkey not in names]
        identities = await chain_identity_names(client, unnamed) if unnamed else {}
        return valuation, positions, names, identities

    with console.status("[dim]loading stake positions…[/dim]"):
        valuation, positions, names, identities = app_ctx.run(_fetch)

    entered: list[str] = []
    if netuid_field is not None and netuid is None and positions:
        netuid = _ask_netuid(console, app_ctx, netuid_field, positions, valuation)
        kwargs[netuid_field] = netuid
        entered += ["--" + netuid_field.replace("_", "-"), str(netuid)]
        console.print()
    if netuid is not None:
        positions = [p for p in positions if p.netuid == netuid]

    if not positions:
        where = f" on netuid {netuid}" if netuid is not None else ""
        app_ctx.output.error(
            f"{owner_label} has no stake{where}",
            help="`btcli stake list` shows every position",
        )
        raise typer.Exit(1)

    lock_info: Optional[tuple[Optional[dict], dict]] = None
    if netuid is not None:

        async def _lock(client):
            return await asyncio.gather(
                client.read("coldkey_lock", coldkey_ss58=owner, netuid=netuid),
                client.read("stake_availability", coldkey_ss58=owner, netuid=netuid),
            )

        lock, availability = app_ctx.run(_lock)
        lock_info = (lock, availability)
        if lock and lock["hotkey"] not in names and lock["hotkey"] not in identities:
            extra = app_ctx.run(lambda c: chain_identity_names(c, [lock["hotkey"]]))
            identities = {**identities, **extra}

    rows = _rows(
        positions,
        valuation,
        per_position=netuid_field is not None,
        subnet_ref=app_ctx.output.with_subnets,
        lock_info=lock_info,
        names=names,
        identities=identities,
    )
    visible = [r for r in rows if r.value.rao >= _DUST_RAO] or rows
    hidden = len(rows) - len(visible)
    flag = address_cli_name(hotkey_field)
    _print_rows(console, app_ctx.output, flag, visible, names, identities)
    if lock_info is not None:
        lock, availability = lock_info
        if lock and availability["locked"].rao > 0:
            lock_label = (
                names.get(lock["hotkey"]) or identities.get(lock["hotkey"]) or lock["hotkey"]
            )
            console.print(
                f"  [dim]netuid {netuid}: {availability['locked']} locked · "
                f"{availability['available']} free · lock → {lock_label}[/dim]"
            )
    if hidden:
        plural = "s" if hidden > 1 else ""
        console.print(
            f"  [dim]+ {hidden} dust position{plural} hidden (under τ0.001)"
            " — answer with a hotkey to use one[/dim]"
        )
    if len(rows) == 1:
        row = rows[0]
        console.print("  [dim]only staked position — selected[/dim]")
    else:
        row = _ask_row(console, app_ctx, flag, visible, hotkey_field, matchable=rows)

    name = names.get(row.hotkey) or identities.get(row.hotkey)
    if name:
        app_ctx.output.name_address(row.hotkey, name)
    kwargs[hotkey_field] = row.hotkey
    entered += [flag, row.hotkey]
    return entered


def _stake_owner(app_ctx: AppContext, kwargs: dict) -> tuple[str, str]:
    """The coldkey whose stake is on offer: the proxied account with --proxy-for,
    otherwise the signing wallet's coldkey."""
    proxied = kwargs.get("proxy_for")
    if proxied:
        owner = app_ctx.resolve_address("proxy_for", proxied)
        return owner, str(proxied)
    try:
        owner = app_ctx.wallet().coldkeypub.ss58_address
    except Exception as error:
        app_ctx.output.error(f"wallet {app_ctx.wallet_name!r} has no usable coldkey: {error}")
        raise typer.Exit(1)
    return owner, f"wallet {app_ctx.wallet_name!r}"


def _ask_netuid(
    console: Console,
    app_ctx: AppContext,
    netuid_field: str,
    positions: list[StakePosition],
    valuation: StakeValuation,
) -> int:
    """List the subnets holding stake and ask which one; returns the netuid.

    The answer is the netuid itself, not a row number — row numbers would be
    ambiguous with small netuids. Enter takes the highest-valued subnet; a
    netuid with no stake re-asks (unstaking there is a guaranteed no-op).
    """
    output = app_ctx.output
    flag = "--" + netuid_field.replace("_", "-")
    value_rao: dict[int, int] = {}
    count: dict[int, int] = {}
    for p in positions:
        value_rao[p.netuid] = value_rao.get(p.netuid, 0) + valuation.spot_value(p.stake).rao
        count[p.netuid] = count.get(p.netuid, 0) + 1
    netuids = sorted(value_rao, key=lambda n: -value_rao[n])
    visible = [n for n in netuids if value_rao[n] >= _DUST_RAO] or netuids
    hidden = len(netuids) - len(visible)

    hint = Text("  ")
    hint.append(flag, style=STYLE_COMMAND)
    hint.append("  ")
    hint.append("Subnet the stake comes from — answer with a netuid below.", style=STYLE_HINT)
    console.print(hint)

    labels = {n: output.with_subnets(f"netuid {n}") for n in visible}
    label_width = max(len(label) for label in labels.values())
    counts = {n: f"{count[n]} position" + ("s" if count[n] > 1 else "") for n in visible}
    count_width = max(len(text) for text in counts.values())
    for n in visible:
        line = Text("    ", overflow="ignore", no_wrap=True)
        line.append_text(output.linked_prose(labels[n].ljust(label_width), STYLE_KEY))
        line.append("  ")
        line.append(counts[n].ljust(count_width), style=STYLE_HINT)
        line.append(f"  ≈ {Balance(value_rao[n])}", style="dim")
        console.print(line, soft_wrap=True)
    if hidden:
        plural = "s" if hidden > 1 else ""
        console.print(
            f"  [dim]+ {hidden} subnet{plural} holding only dust hidden (under τ0.001)"
            " — answering with a hidden netuid still works[/dim]"
        )

    if len(netuids) == 1:
        console.print("  [dim]only staked subnet — selected[/dim]")
        return netuids[0]

    prompt = Text("  ")
    prompt.append(flag.lstrip("-"), style=STYLE_COMMAND)
    prompt.append(f" [{netuids[0]}]", style=STYLE_HINT)
    prompt.append(": ", style=STYLE_COMMAND)
    while True:
        try:
            raw = console.input(prompt).strip()
        except (KeyboardInterrupt, EOFError):
            console.print()
            output.message("aborted.")
            raise typer.Exit(130)
        if not raw:
            return netuids[0]
        try:
            picked = int(raw)
        except ValueError:
            console.print("  answer with one of the netuids above", style=STYLE_HINT)
            continue
        if picked in value_rao:
            return picked
        console.print(
            f"  no stake on netuid {picked} — pick one of the subnets above",
            style=STYLE_HINT,
        )


def _rows(
    positions: list[StakePosition],
    valuation: StakeValuation,
    *,
    per_position: bool,
    subnet_ref: Callable[[str], str] = lambda text: text,
    lock_info: Optional[tuple[Optional[dict], dict]] = None,
    names: Optional[dict[str, str]] = None,
    identities: Optional[dict[str, str]] = None,
) -> list[_Row]:
    """One row per position — or per hotkey (spot total) for ops without a netuid.

    Per-position rows carry no subnet label: the netuid was already pinned (asked
    or flagged) before this list renders, so every row would repeat it.
    ``subnet_ref`` rewrites "netuid N" labels to the canonical named form
    ("netuid N (Targon)") when names are known.
    """
    names = names or {}
    identities = identities or {}
    lock_hotkey = None
    locked_rao = 0
    if lock_info is not None:
        lock, availability = lock_info
        locked_rao = int(availability["locked"].rao) if availability else 0
        if lock and locked_rao > 0:
            lock_hotkey = lock["hotkey"]
    lock_label = (
        names.get(lock_hotkey) or identities.get(lock_hotkey) or lock_hotkey
        if lock_hotkey
        else None
    )

    rows: list[_Row] = []
    if per_position:
        for p in positions:
            value = valuation.spot_value(p.stake)
            note = ""
            if lock_hotkey and p.hotkey != lock_hotkey:
                note = f"lock on {lock_label}"
            rows.append(_Row(p.hotkey, p.netuid, "", str(p.stake), value, p.netuid != 0, note))
    else:
        by_hotkey: dict[str, list[StakePosition]] = {}
        for p in positions:
            by_hotkey.setdefault(p.hotkey, []).append(p)
        for hotkey, group in by_hotkey.items():
            value = Balance(sum(valuation.spot_value(p.stake).rao for p in group))
            note = ""
            if lock_hotkey and hotkey != lock_hotkey:
                note = f"lock on {lock_label}"
            if len(group) == 1:
                only = group[0]
                rows.append(
                    _Row(
                        hotkey,
                        only.netuid,
                        subnet_ref(f"netuid {only.netuid}"),
                        str(only.stake),
                        value,
                        only.netuid != 0,
                        note,
                    )
                )
            else:
                rows.append(
                    _Row(hotkey, None, f"{len(group)} subnets", f"≈ {value}", value, False, note)
                )
    rows.sort(key=lambda r: -r.value.rao)
    return rows


def _print_rows(
    console: Console,
    output: Output,
    flag: str,
    rows: list[_Row],
    names: dict[str, str],
    identities: dict[str, str],
) -> None:
    hint = Text("  ")
    hint.append(flag, style=STYLE_COMMAND)
    hint.append("  ")
    hint.append("Where the stake comes from — pick a position below.", style=STYLE_HINT)
    console.print(hint)

    def label(hotkey: str) -> tuple[str, str]:
        if hotkey in names:
            return names[hotkey], STYLE_NAME
        if hotkey in identities:
            # On-chain identity name: informative but unverified (no local-name accent).
            return identities[hotkey], "dim italic"
        return hotkey, ""

    number_width = len(str(len(rows)))
    label_width = max(len(label(r.hotkey)[0]) for r in rows)
    where_width = max(len(r.where) for r in rows)
    stake_width = max(len(r.stake) for r in rows)
    for index, row in enumerate(rows, start=1):
        text, style = label(row.hotkey)
        line = Text("  ", overflow="ignore", no_wrap=True)
        line.append(str(index).rjust(number_width), style=STYLE_COMMAND)
        line.append("  ")
        line.append(text.ljust(label_width), style=style)
        line.append("  ")
        if where_width:
            line.append_text(output.linked_prose(row.where.ljust(where_width), STYLE_KEY))
            line.append("  ")
        line.append(row.stake.rjust(stake_width))
        if row.show_value:
            line.append(f"  ≈ {row.value}", style="dim")
        if row.note:
            line.append(f"  {row.note}", style="dim italic")
        console.print(line, soft_wrap=True)


def _ask_row(
    console: Console,
    app_ctx: AppContext,
    flag: str,
    rows: list[_Row],
    hotkey_field: str,
    *,
    matchable: Optional[list[_Row]] = None,
) -> _Row:
    """Ask until a row is chosen: a number picks from the list; anything else
    resolves like the flag would (hotkey name, address-book name, ss58).

    ``matchable`` widens the name/address lookup beyond the displayed rows so
    a dust position hidden from the listing can still be picked by name."""
    matchable = matchable if matchable is not None else rows
    prompt = Text("  ")
    prompt.append(flag.lstrip("-"), style=STYLE_COMMAND)
    prompt.append(" [1]", style=STYLE_HINT)
    prompt.append(": ", style=STYLE_COMMAND)
    while True:
        try:
            raw = console.input(prompt).strip()
        except (KeyboardInterrupt, EOFError):
            console.print()
            app_ctx.output.message("aborted.")
            raise typer.Exit(130)
        if not raw:
            raw = "1"
        if raw.isdigit():
            index = int(raw)
            if 1 <= index <= len(rows):
                return rows[index - 1]
            console.print(f"  enter a number between 1 and {len(rows)}", style=STYLE_HINT)
            continue
        try:
            address = app_ctx.resolve_address(hotkey_field, raw)
        except typer.Exit:
            continue  # the resolver already printed its own error
        matches = [row for row in matchable if row.hotkey == address]
        if matches:
            return matches[0]  # rows are sorted by value; take the largest
        console.print("  [dim]note: no stake found on that hotkey — using it anyway[/dim]")
        return _Row(address, None, "", "", Balance(0), False)
