"""Shared reads and interactive pickers for ``btcli root``.

Root positions are presented in TAO: *staked* is principal on netuid 0,
*accrued* is the TAO your fund shares would return if redeemed today.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import typer
from rich.console import Console
from rich.text import Text

from ..balance import Balance
from ..client import Client
from .context import AppContext, address_cli_name
from .helpers import (
    DUST_VALUE_TAO,
    chain_identity_names,
    list_coldkeys,
    local_address_names,
)
from .output import STYLE_COMMAND, STYLE_HINT, STYLE_KEY, STYLE_NAME
from .prompt import PromptSpec

# Same dust cutoff as ``btcli stake list`` (τ0.001).
DUST_POSITION_RAO = int(DUST_VALUE_TAO * 1_000_000_000)


@dataclass
class RootPosition:
    """One validator's root position for a coldkey, in TAO."""

    hotkey: str
    staked: Balance  # principal on netuid 0
    accrued: Balance  # fund entitlement (realizable TAO quote)
    wallet: Optional[str] = None
    coldkey: Optional[str] = None

    @property
    def total(self) -> Balance:
        return Balance.from_rao(self.staked.rao + self.accrued.rao)

    def as_record(self) -> dict:
        total = self.total
        return {
            "hotkey": self.hotkey,
            "wallet": self.wallet,
            "coldkey": self.coldkey,
            "staked": self.staked,
            "staked_tao": self.staked.tao,
            "accrued": self.accrued,
            "accrued_tao": self.accrued.tao,
            "total": total,
            "total_tao": total.tao,
            "value_tao": total.tao,
        }


def is_dust_position(position: RootPosition) -> bool:
    return position.total.rao < DUST_POSITION_RAO


def filter_dust_positions(positions: list[RootPosition]) -> list[RootPosition]:
    return [pos for pos in positions if not is_dust_position(pos)]


async def fetch_root_positions(client: Client, coldkey_ss58: str) -> list[RootPosition]:
    snapshot = await client.at()
    stakes = await snapshot.read("stake_for_coldkey", coldkey_ss58=coldkey_ss58)
    staked_by_hotkey = {
        pos.hotkey: pos.stake for pos in stakes if pos.netuid == 0 and pos.stake.rao > 0
    }
    accrued_rows = await snapshot.read("root_basket_owed_breakdown", coldkey_ss58=coldkey_ss58)
    accrued_by_hotkey = {row["hotkey"]: row["owed_tao"] for row in accrued_rows}
    hotkeys = sorted(set(staked_by_hotkey) | set(accrued_by_hotkey))
    return [
        RootPosition(
            hotkey=hotkey,
            staked=staked_by_hotkey.get(hotkey, Balance.from_rao(0)),
            accrued=accrued_by_hotkey.get(hotkey, Balance.from_rao(0)),
        )
        for hotkey in hotkeys
    ]


async def fetch_all_root_positions(
    client: Client, coldkeys: list[tuple[str, str]]
) -> list[RootPosition]:
    positions: list[RootPosition] = []
    for name, ss58 in coldkeys:
        for pos in await fetch_root_positions(client, ss58):
            positions.append(
                RootPosition(
                    hotkey=pos.hotkey,
                    staked=pos.staked,
                    accrued=pos.accrued,
                    wallet=name,
                    coldkey=ss58,
                )
            )
    positions.sort(key=lambda p: -p.total.rao)
    return positions


def _hotkey_label(
    hotkey: str,
    names: dict[str, str],
    identities: dict[str, str],
) -> str:
    return names.get(hotkey) or identities.get(hotkey) or hotkey


async def _wallets_with_positions(
    client: Client, coldkeys: list[tuple[str, str]]
) -> list[tuple[str, str, Balance, int]]:
    """Wallets with a non-dust root position; returns (name, ss58, total τ, validator count)."""
    held: list[tuple[str, str, Balance, int]] = []
    for name, ss58 in coldkeys:
        positions = filter_dust_positions(await fetch_root_positions(client, ss58))
        if not positions:
            continue
        total = Balance(sum(pos.total.rao for pos in positions))
        held.append((name, ss58, total, len(positions)))
    held.sort(key=lambda row: -row[2].rao)
    return held


def filter_claimable_positions(positions: list[RootPosition]) -> list[RootPosition]:
    """Positions with accrued yield above dust (realize via ``claim_root_with_hotkey``)."""
    return [pos for pos in positions if pos.accrued.rao >= DUST_POSITION_RAO]


def resolve_claim_wallet(
    console: Console,
    app_ctx: AppContext,
    coldkey_ss58: Optional[str],
    *,
    interactive: bool,
) -> tuple[str, str]:
    """Pick the coldkey whose root position to claim / withdraw.

    Uses ``--coldkey`` / ``-w`` when given; otherwise prompts among wallets that
    hold a non-dust root position.
    """
    if coldkey_ss58:
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        for name, ss58 in list_coldkeys(app_ctx.wallet_path):
            if ss58 == owner:
                return name, ss58
        return app_ctx.wallet_name, owner

    if app_ctx.wallet_given or not interactive:
        owner = app_ctx.resolve_address("coldkey_ss58", None)
        return app_ctx.wallet_name, owner

    all_wallets = list_coldkeys(app_ctx.wallet_path)
    if not all_wallets:
        app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
        raise typer.Exit(1)

    with_positions = app_ctx.run(lambda c: _wallets_with_positions(c, all_wallets))
    if not with_positions:
        app_ctx.output.message(
            f"no root position above dust (τ{DUST_VALUE_TAO}) in any wallet under "
            f"{app_ctx.wallet_path}"
        )
        raise typer.Exit(0)

    if len(with_positions) == 1:
        name, ss58, total, _ = with_positions[0]
        app_ctx.output.message(f"wallet {name} ({ss58}) — {total}")
        return name, ss58

    hint = Text("  ")
    hint.append("Select wallet", style=STYLE_KEY)
    hint.append("  ", style=STYLE_HINT)
    hint.append("(only wallets with a root position above dust).", style=STYLE_HINT)
    console.print(hint)
    default = next(
        (row for row in with_positions if row[0] == app_ctx.wallet_name),
        with_positions[0],
    )
    for index, (name, _ss58, total, count) in enumerate(with_positions, start=1):
        line = Text("    ", overflow="ignore", no_wrap=True)
        line.append(f"{index}. ", style=STYLE_KEY)
        line.append(name, style=STYLE_NAME)
        line.append(f"  {total}  ({count} validators)", style="dim")
        console.print(line)

    while True:
        raw = console.input("  > ").strip()
        if not raw:
            return default[0], default[1]
        if raw.isdigit():
            idx = int(raw)
            if 1 <= idx <= len(with_positions):
                row = with_positions[idx - 1]
                return row[0], row[1]
        for name, ss58, _, _ in with_positions:
            if raw in (name, ss58):
                return name, ss58
        console.print("  [dim]enter a number, wallet name, or coldkey[/dim]")


def claim_root_source_spec(hotkey_field: str = "hotkey_ss58") -> PromptSpec:
    """PromptSpec whose custom flow picks a validator from root positions."""

    def _pick(console: Console, app_ctx: AppContext, kwargs: dict) -> list[str]:
        owner = app_ctx.resolve_address("coldkey_ss58", None)
        positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
        chosen = pick_claim_hotkey(console, app_ctx, positions, flag=address_cli_name(hotkey_field))
        kwargs[hotkey_field] = chosen.hotkey
        return [address_cli_name(hotkey_field), chosen.hotkey]

    return PromptSpec(
        field=hotkey_field,
        flag=address_cli_name(hotkey_field),
        help=None,
        parse=lambda _app_ctx, raw: raw,
        custom=_pick,
    )


def pick_claim_hotkey(
    console: Console,
    app_ctx: AppContext,
    positions: list[RootPosition],
    *,
    flag: str,
) -> RootPosition:
    """Numbered picker over root positions; returns the chosen position."""
    held = filter_dust_positions(positions)
    if not held:
        app_ctx.output.message(f"no root position above dust (τ{DUST_VALUE_TAO}) for this wallet")
        raise typer.Exit(0)

    held = sorted(held, key=lambda p: -p.total.rao)
    names = local_address_names(app_ctx.wallet_path)
    unnamed = [pos.hotkey for pos in held if pos.hotkey not in names]
    identities = app_ctx.run(lambda c: chain_identity_names(c, unnamed)) if unnamed else {}

    hint = Text("  ")
    hint.append(flag, style=STYLE_COMMAND)
    hint.append("  ", style=STYLE_HINT)
    hint.append(
        "Validator to claim from — answer with a number, name, or hotkey.",
        style=STYLE_HINT,
    )
    console.print(hint)

    for index, pos in enumerate(held, start=1):
        label = _hotkey_label(pos.hotkey, names, identities)
        line = Text("    ", overflow="ignore", no_wrap=True)
        line.append(f"{index}. ", style=STYLE_KEY)
        line.append(label, style=STYLE_NAME)
        line.append(
            f"  total {pos.total}  (staked {pos.staked}, accrued {pos.accrued})",
            style="dim",
        )
        console.print(line)

    if len(held) == 1:
        console.print("  [dim]only validator — selected[/dim]")
        return held[0]

    matchable = {str(i): pos for i, pos in enumerate(held, start=1)}
    matchable.update({pos.hotkey: pos for pos in held})
    for pos in held:
        label = _hotkey_label(pos.hotkey, names, identities)
        if label not in matchable:
            matchable[label] = pos

    while True:
        raw = console.input("  > ").strip()
        if not raw:
            return held[0]
        if raw in matchable:
            return matchable[raw]
        close = [
            key
            for key, pos in matchable.items()
            if key.startswith(raw) and pos.hotkey.startswith(raw)
        ]
        if len(close) == 1:
            return matchable[close[0]]
        console.print("  [dim]enter a list number or hotkey[/dim]")


def prompt_claim_amount(console: Console, position: RootPosition) -> Optional[str]:
    """Ask how much to withdraw; ``None`` means claim accrued into stake only."""
    can_compound = position.accrued.rao >= DUST_POSITION_RAO
    hint = Text("  ")
    hint.append("--amount", style=STYLE_COMMAND)
    hint.append("  ", style=STYLE_HINT)
    if can_compound:
        hint.append(
            f"Withdraw to free balance (total {position.total}). "
            "`all`, a τ amount, or Enter to only claim accrued into stake.",
            style=STYLE_HINT,
        )
    else:
        hint.append(
            f"Withdraw to free balance (total {position.total}). "
            "`all` or a τ amount (no accrued yield to claim into stake).",
            style=STYLE_HINT,
        )
    console.print(hint)
    while True:
        raw = console.input("  > ").strip()
        if not raw:
            if can_compound:
                return None
            return "all"
        if raw.lower() == "all":
            return "all"
        try:
            if Balance.from_tao(raw).rao <= 0:
                console.print("  [dim]amount must be positive[/dim]")
                continue
            return raw
        except Exception:
            console.print("  [dim]enter `all`, a τ amount, or press Enter[/dim]")


def resolve_show_wallet(
    console: Console,
    app_ctx: AppContext,
    coldkey_ss58: Optional[str],
    *,
    interactive: bool,
) -> tuple[str, str]:
    """Pick the coldkey whose root positions to inspect.

    Uses ``--coldkey`` or ``-w`` when given; otherwise prompts only among wallets
    that hold more than dust on root (not every wallet on disk).
    """
    if coldkey_ss58:
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        for name, ss58 in list_coldkeys(app_ctx.wallet_path):
            if ss58 == owner:
                return name, ss58
        return app_ctx.wallet_name, owner

    if app_ctx.wallet_given or not interactive:
        owner = app_ctx.resolve_address("coldkey_ss58", None)
        return app_ctx.wallet_name, owner

    all_wallets = list_coldkeys(app_ctx.wallet_path)
    if not all_wallets:
        app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
        raise typer.Exit(1)

    with_positions = app_ctx.run(lambda c: _wallets_with_positions(c, all_wallets))
    if not with_positions:
        app_ctx.output.message(
            f"no root position above dust (τ{DUST_VALUE_TAO}) in any wallet under "
            f"{app_ctx.wallet_path}"
        )
        raise typer.Exit(0)

    if len(with_positions) == 1:
        name, ss58, total, _ = with_positions[0]
        app_ctx.output.message(f"wallet {name} ({ss58}) — {total}")
        return name, ss58

    hint = Text("  ")
    hint.append("Select wallet", style=STYLE_KEY)
    hint.append("  ", style=STYLE_HINT)
    hint.append("(only wallets with a root position above dust).", style=STYLE_HINT)
    console.print(hint)
    default = next(
        (row for row in with_positions if row[0] == app_ctx.wallet_name),
        with_positions[0],
    )
    for index, (name, _ss58, total, count) in enumerate(with_positions, start=1):
        line = Text("    ", overflow="ignore", no_wrap=True)
        line.append(f"{index}. ", style=STYLE_KEY)
        line.append(name, style=STYLE_NAME)
        line.append(f"  {total}  ({count} validators)", style="dim")
        console.print(line)

    while True:
        raw = console.input("  > ").strip()
        if not raw:
            return default[0], default[1]
        if raw.isdigit():
            idx = int(raw)
            if 1 <= idx <= len(with_positions):
                row = with_positions[idx - 1]
                return row[0], row[1]
        for name, ss58, _, _ in with_positions:
            if raw in (name, ss58):
                return name, ss58
        console.print("  [dim]enter a number, wallet name, or coldkey[/dim]")


def pick_validator(
    console: Console,
    app_ctx: AppContext,
    rows: list[dict],
    *,
    flag: str,
) -> dict:
    """Numbered picker over validator summary rows; returns the chosen row."""
    if not rows:
        app_ctx.output.message(
            f"no root position above dust (τ{DUST_VALUE_TAO}) on any validator for this wallet"
        )
        raise typer.Exit(0)

    names = local_address_names(app_ctx.wallet_path)
    unnamed = [row["hotkey"] for row in rows if row["hotkey"] not in names]
    identities = app_ctx.run(lambda c: chain_identity_names(c, unnamed)) if unnamed else {}

    hint = Text("  ")
    hint.append(flag, style=STYLE_COMMAND)
    hint.append("  ", style=STYLE_HINT)
    hint.append("Validator to inspect — answer with a number, name, or hotkey.", style=STYLE_HINT)
    console.print(hint)

    for index, row in enumerate(rows, start=1):
        label = _hotkey_label(row["hotkey"], names, identities)
        yours = row.get("your_tao")
        yours_text = f"  yours τ{yours:.4f}" if yours is not None and yours > 0 else ""
        perf = row.get("lifetime_return")
        perf_text = f"  {perf:.2f}x" if perf is not None else ""
        line = Text("    ", overflow="ignore", no_wrap=True)
        line.append(f"{index}. ", style=STYLE_KEY)
        line.append(label, style=STYLE_NAME)
        line.append(
            f"  nav {row['nav_tao']}  weights {row['weight_count']}{yours_text}{perf_text}",
            style="dim",
        )
        console.print(line)

    if len(rows) == 1:
        console.print("  [dim]only validator — selected[/dim]")
        return rows[0]

    matchable = {str(i): row for i, row in enumerate(rows, start=1)}
    matchable.update({row["hotkey"]: row for row in rows})
    for row in rows:
        label = _hotkey_label(row["hotkey"], names, identities)
        if label not in matchable:
            matchable[label] = row

    while True:
        raw = console.input("  > ").strip()
        if not raw:
            return rows[0]
        if raw in matchable:
            return matchable[raw]
        close = [
            key
            for key, row in matchable.items()
            if key.startswith(raw) and row["hotkey"].startswith(raw)
        ]
        if len(close) == 1:
            return matchable[close[0]]
        console.print("  [dim]enter a list number or hotkey[/dim]")


def print_command_hint(console: Console, argv_prefix: list[str]) -> None:
    line = Text("  ")
    line.append("hint:".rjust(7), style=STYLE_HINT)
    line.append(" ")
    line.append(" ".join(argv_prefix), style=STYLE_COMMAND)
    console.print(line)


def render_validator_detail(
    app_ctx: AppContext, summary: dict, yours: Optional[RootPosition]
) -> None:
    if app_ctx.output.json_mode:
        app_ctx.output.value(summary)
        return

    hotkey = summary["hotkey"]
    weights = summary.get("weights") or []
    holdings = summary.get("holdings") or []

    if yours and yours.total.rao > 0:
        record = yours.as_record()
        app_ctx.output.message(
            f"your position on {hotkey}: {record['total']} "
            f"(staked {record['staked']}, accrued {record['accrued']})"
        )

    if weights:
        weight_rows = [[w["netuid"], f"{w['share']:.2%}", w["weight"]] for w in weights]
        app_ctx.output.table(
            f"weights of {hotkey}",
            ["netuid", "share", "weight (u16)"],
            weight_rows,
        )
    else:
        app_ctx.output.message(
            f"no custom root weights on {hotkey}: "
            "dividends accumulate in place on their origin subnet"
        )

    if holdings:
        table_rows = [
            [
                entry["netuid"],
                str(entry["alpha"]),
                str(entry["realizable_tao"]),
                str(entry["spot_tao"]),
            ]
            for entry in holdings
        ]
        app_ctx.output.table(
            f"fund holdings of {hotkey}",
            ["netuid", "holding", "realizable", "spot"],
            table_rows,
        )

    lifetime = summary.get("lifetime_return")
    app_ctx.output.message(
        f"fund nav: {summary['nav_tao']} (spot {summary['spot_nav_tao']}) | "
        f"subscribed {summary['deposited_tao']}, redeemed {summary['redeemed_tao']}"
        + (f" | lifetime {lifetime:.4f}x" if lifetime is not None else "")
    )


def position_rows(positions: list[RootPosition], all_wallets: bool) -> list[list[str]]:
    return [
        ([pos.wallet or "—"] if all_wallets else [])
        + [
            pos.hotkey,
            str(pos.staked),
            str(pos.accrued),
            str(pos.total),
            f"{pos.total.tao:.6f}",
        ]
        for pos in positions
    ]


def position_columns(all_wallets: bool) -> list[str]:
    cols = ["validator hotkey", "staked (τ)", "accrued (τ)", "total (τ)", "value (τ)"]
    if all_wallets:
        return ["wallet", *cols]
    return cols
