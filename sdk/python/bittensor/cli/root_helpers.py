"""Shared reads and interactive pickers for ``btcli root``.

Root staking uses **beta** as the user-facing unit: staked beta is principal on
netuid 0 (1 β = 1 τ), accrued beta is basket entitlement (quoted in τ).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import typer
from rich.console import Console
from rich.text import Text

from ..balance import Balance
from ..client import Client
from .context import AppContext
from .helpers import (
    DUST_VALUE_TAO,
    chain_identity_names,
    list_coldkeys,
    local_address_names,
)
from .output import STYLE_COMMAND, STYLE_HINT, STYLE_KEY, STYLE_NAME

# Same dust cutoff as ``btcli stake list`` (τ0.001).
DUST_BETA_RAO = int(DUST_VALUE_TAO * 1_000_000_000)


@dataclass
class BetaPosition:
    """One validator's beta exposure for a coldkey."""

    hotkey: str
    staked_beta: Balance  # principal on netuid 0
    accrued_beta: Balance  # basket entitlement (realizable τ quote)
    wallet: Optional[str] = None
    coldkey: Optional[str] = None

    @property
    def total_beta(self) -> Balance:
        return Balance.from_rao(self.staked_beta.rao + self.accrued_beta.rao)

    def as_record(self) -> dict:
        total = self.total_beta
        return {
            "hotkey": self.hotkey,
            "wallet": self.wallet,
            "coldkey": self.coldkey,
            "staked_beta": self.staked_beta,
            "staked_beta_tao": self.staked_beta.tao,
            "accrued_beta": self.accrued_beta,
            "accrued_beta_tao": self.accrued_beta.tao,
            "beta": total,
            "beta_tao": total.tao,
            "value_tao": total.tao,
        }


def is_dust_beta(position: BetaPosition) -> bool:
    return position.total_beta.rao < DUST_BETA_RAO


def filter_dust_beta(positions: list[BetaPosition]) -> list[BetaPosition]:
    return [pos for pos in positions if not is_dust_beta(pos)]


async def fetch_beta_positions(client: Client, coldkey_ss58: str) -> list[BetaPosition]:
    snapshot = await client.at()
    stakes = await snapshot.read("stake_for_coldkey", coldkey_ss58=coldkey_ss58)
    staked_by_hotkey = {
        pos.hotkey: pos.stake for pos in stakes if pos.netuid == 0 and pos.stake.rao > 0
    }
    accrued_rows = await snapshot.read("root_basket_owed_breakdown", coldkey_ss58=coldkey_ss58)
    accrued_by_hotkey = {row["hotkey"]: row["owed_tao"] for row in accrued_rows}
    hotkeys = sorted(set(staked_by_hotkey) | set(accrued_by_hotkey))
    return [
        BetaPosition(
            hotkey=hotkey,
            staked_beta=staked_by_hotkey.get(hotkey, Balance.from_rao(0)),
            accrued_beta=accrued_by_hotkey.get(hotkey, Balance.from_rao(0)),
        )
        for hotkey in hotkeys
    ]


async def fetch_all_beta_positions(
    client: Client, coldkeys: list[tuple[str, str]]
) -> list[BetaPosition]:
    positions: list[BetaPosition] = []
    for name, ss58 in coldkeys:
        for pos in await fetch_beta_positions(client, ss58):
            positions.append(
                BetaPosition(
                    hotkey=pos.hotkey,
                    staked_beta=pos.staked_beta,
                    accrued_beta=pos.accrued_beta,
                    wallet=name,
                    coldkey=ss58,
                )
            )
    positions.sort(key=lambda p: -p.total_beta.rao)
    return positions


def _hotkey_label(
    hotkey: str,
    names: dict[str, str],
    identities: dict[str, str],
) -> str:
    return names.get(hotkey) or identities.get(hotkey) or hotkey


async def _wallets_holding_beta(
    client: Client, coldkeys: list[tuple[str, str]]
) -> list[tuple[str, str, Balance, int]]:
    """Wallets with non-dust β somewhere; returns (name, ss58, total β, validator count)."""
    held: list[tuple[str, str, Balance, int]] = []
    for name, ss58 in coldkeys:
        positions = filter_dust_beta(await fetch_beta_positions(client, ss58))
        if not positions:
            continue
        total = Balance(sum(pos.total_beta.rao for pos in positions))
        held.append((name, ss58, total, len(positions)))
    held.sort(key=lambda row: -row[2].rao)
    return held


def resolve_show_wallet(
    console: Console,
    app_ctx: AppContext,
    coldkey_ss58: Optional[str],
    *,
    interactive: bool,
) -> tuple[str, str]:
    """Pick the coldkey whose β to inspect.

    Uses ``--coldkey`` or ``-w`` when given; otherwise prompts only among wallets
    that hold more than dust β (not every wallet on disk).
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

    with_beta = app_ctx.run(lambda c: _wallets_holding_beta(c, all_wallets))
    if not with_beta:
        app_ctx.output.message(
            f"no beta above dust (τ{DUST_VALUE_TAO}) in any wallet under {app_ctx.wallet_path}"
        )
        raise typer.Exit(0)

    if len(with_beta) == 1:
        name, ss58, total, _ = with_beta[0]
        app_ctx.output.message(f"wallet {name} ({ss58}) — {total} β")
        return name, ss58

    hint = Text("  ")
    hint.append("Select wallet", style=STYLE_KEY)
    hint.append("  ", style=STYLE_HINT)
    hint.append("(only wallets with β above dust).", style=STYLE_HINT)
    console.print(hint)
    default = next(
        (row for row in with_beta if row[0] == app_ctx.wallet_name),
        with_beta[0],
    )
    for index, (name, ss58, total, count) in enumerate(with_beta, start=1):
        line = Text("    ", overflow="ignore", no_wrap=True)
        line.append(f"{index}. ", style=STYLE_KEY)
        line.append(name, style=STYLE_NAME)
        line.append(f"  {total} β  ({count} validators)", style="dim")
        console.print(line)

    while True:
        raw = console.input("  > ").strip()
        if not raw:
            return default[0], default[1]
        if raw.isdigit():
            idx = int(raw)
            if 1 <= idx <= len(with_beta):
                row = with_beta[idx - 1]
                return row[0], row[1]
        for name, ss58, _, _ in with_beta:
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
            f"no beta above dust (τ{DUST_VALUE_TAO}) on any validator for this wallet"
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
        yours = row.get("your_beta_tao")
        yours_text = f"  your β {yours:.4f} τ" if yours is not None and yours > 0 else ""
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


def render_validator_detail(app_ctx: AppContext, summary: dict, your_beta: Optional[BetaPosition]) -> None:
    hotkey = summary["hotkey"]
    weights = summary.get("weights") or []
    holdings = summary.get("holdings") or []

    if your_beta and your_beta.total_beta.rao > 0:
        record = your_beta.as_record()
        app_ctx.output.message(
            f"your beta on {hotkey}: {record['beta']} "
            f"(staked {record['staked_beta']}, accrued {record['accrued_beta']})"
        )

    if weights:
        weight_rows = [[w["netuid"], f"{w['share']:.2%}", w["weight"]] for w in weights]
        app_ctx.output.table(
            f"weights of {hotkey}",
            ["netuid", "share", "weight (u16)"],
            weight_rows,
            weights,
        )
    else:
        app_ctx.output.message(
            f"no custom root weights on {hotkey}: dividends default to 100% root (TAO in the basket)"
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
            summary,
        )

    lifetime = summary.get("lifetime_return")
    app_ctx.output.message(
        f"fund nav: {summary['nav_tao']} (spot {summary['spot_nav_tao']}) | "
        f"price {summary['share_price']:.4f} τ/β | "
        f"deposited {summary['deposited_tao']}, redeemed {summary['redeemed_tao']}"
        + (f" | lifetime {lifetime:.4f}x" if lifetime is not None else "")
    )


def beta_list_rows(positions: list[BetaPosition]) -> list[list[str]]:
    return [
        [
            pos.wallet or "—",
            pos.hotkey,
            str(pos.staked_beta),
            str(pos.accrued_beta),
            str(pos.total_beta),
            f"{pos.total_beta.tao:.6f}",
        ]
        for pos in positions
    ]


def beta_list_columns(all_wallets: bool) -> list[str]:
    cols = ["validator hotkey", "staked β", "accrued β", "total β", "value (τ)"]
    if all_wallets:
        return ["wallet"] + cols
    return cols
