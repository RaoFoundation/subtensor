"""Shared reads and interactive pickers for ``btcli root``.

Root positions are presented in TAO: *staked* is principal on netuid 0,
*accrued* is the TAO your accrued beta would return if redeemed today.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import typer
from rich.console import Console
from rich.text import Text

from .._generated import storage
from ..balance import Balance
from ..basket_index import staker_yield
from ..client import Client
from ..wallets import is_bittensor_address
from .context import AppContext, address_cli_name
from .helpers import (
    DUST_VALUE_TAO,
    chain_identity_names,
    list_coldkeys,
    local_address_names,
)
from .output import (
    CHOICE_INDENT,
    PROMPT_KEY_WIDTH,
    STYLE_COMMAND,
    STYLE_HINT,
    STYLE_KEY,
    STYLE_NAME,
    kv_line,
    prompt_header,
    prompt_hint,
)
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

    @property
    def return_pct(self) -> Optional[float]:
        """Unclaimed yield relative to principal, or ``None`` without principal.

        Beta accrues purely from dividends, so ``accrued`` is money
        made on top of ``staked``. Previously claimed yield is not included:
        claims fold into stake (or leave to free balance), and the chain keeps
        no per-staker lifetime payout history.
        """
        if self.staked.rao <= 0:
            return None
        return self.accrued.rao / self.staked.rao

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
            "return_pct": self.return_pct,
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


def filter_claimable_positions(positions: list[RootPosition]) -> list[RootPosition]:
    """Positions with accrued yield above dust (realize via ``claim_root_with_hotkey``)."""
    return [pos for pos in positions if pos.accrued.rao >= DUST_POSITION_RAO]


def resolve_position_wallet(app_ctx: AppContext, coldkey_ss58: Optional[str]) -> tuple[str, str]:
    """Pick the coldkey whose root positions to act on (claim, unstake, show).

    Uses ``--coldkey`` / ``-w`` when given; otherwise ``resolve_address``
    prompts for the wallet. The prompt is entirely local, so it comes before
    any chain read — the same ordering as ``root list``. (An earlier version
    scanned every wallet's positions first to offer only wallets with a
    position, which put the query spinner ahead of the wallet question; the
    validator picker that follows still filters to non-dust positions.)
    """
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    for name, ss58 in list_coldkeys(app_ctx.wallet_path):
        if ss58 == owner:
            return name, ss58
    return app_ctx.wallet_name, owner


def resolve_validator_selector(app_ctx: AppContext, value: str) -> str:
    """Resolve a root-validator selector to its hotkey ss58.

    Three accepted forms: a hotkey ss58 (used as-is), a root UID (the
    validator's index on netuid 0), or a name — matched case-insensitively
    against local names (wallet hotkeys, address book) first, then against
    the on-chain identity names of validators with a fund. UID and name
    lookups read the chain, so callers must resolve any wallet prompts
    before calling (a prompt under a live query spinner cannot take input).
    """
    if is_bittensor_address(value):
        app_ctx.output.classify_address(value, "hotkey")
        return value

    if value.isdigit():
        uid = int(value)
        with app_ctx.output.activity(f"resolving root UID {uid}…"):
            hotkey = app_ctx.run(lambda c: c.query(storage.SubtensorModule.Keys, [0, uid]))
        if hotkey is None:
            app_ctx.output.error(
                f"no validator at root UID {uid}",
                help="`btcli root list` lists every validator basket",
            )
            raise typer.Exit(1)
        hotkey = str(hotkey)
        app_ctx.output.classify_address(hotkey, "hotkey")
        return hotkey

    wanted = value.casefold()
    for ss58, name in local_address_names(app_ctx.wallet_path).items():
        if name.casefold() == wanted:
            app_ctx.output.name_address(ss58, name)
            app_ctx.output.classify_address(ss58, "hotkey")
            return ss58

    async def _identities(client: Client) -> dict[str, str]:
        summaries = await client.read("root_baskets")
        return await chain_identity_names(client, [row["hotkey"] for row in summaries])

    with app_ctx.output.activity(f"looking up {value!r} among root validators…"):
        identities = app_ctx.run(_identities)
    for ss58, name in identities.items():
        if name.casefold() == wanted:
            app_ctx.output.name_address(ss58, name)
            app_ctx.output.classify_address(ss58, "hotkey")
            return ss58

    app_ctx.output.error(
        f"no root validator named {value!r}",
        help="pass a hotkey ss58, a root UID, an address-book name, or an "
        "on-chain identity name (`btcli root list` lists every fund)",
    )
    raise typer.Exit(1)


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
    prompt: str = "Validator to claim from — answer with a number, name, or hotkey.",
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

    console.print(prompt_header(flag, prompt))

    labels = [_hotkey_label(pos.hotkey, names, identities) for pos in held]
    number_width = len(str(len(held)))
    label_width = max(len(label) for label in labels)
    for index, (pos, label) in enumerate(zip(held, labels), start=1):
        line = Text(" " * CHOICE_INDENT, overflow="ignore", no_wrap=True)
        line.append(str(index).rjust(number_width), style=STYLE_COMMAND)
        line.append("  ")
        line.append(label.ljust(label_width), style=STYLE_NAME)
        line.append(
            f"  total {pos.total}  (staked {pos.staked}, accrued {pos.accrued})",
            style="dim",
        )
        console.print(line, soft_wrap=True)

    if len(held) == 1:
        console.print(prompt_hint("only validator — selected"))
        return held[0]

    matchable = {str(i): pos for i, pos in enumerate(held, start=1)}
    matchable.update({pos.hotkey: pos for pos in held})
    for pos in held:
        label = _hotkey_label(pos.hotkey, names, identities)
        if label not in matchable:
            matchable[label] = pos

    prompt = Text("  ")
    prompt.append(flag.lstrip("-"), style=STYLE_COMMAND)
    prompt.append(" [1]", style=STYLE_HINT)
    prompt.append(": ", style=STYLE_COMMAND)
    while True:
        raw = console.input(prompt).strip()
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
        console.print(prompt_hint("enter a list number or hotkey"))


def pick_fund(
    console: Console,
    app_ctx: AppContext,
    records: list[dict],
    *,
    flag: str,
    prompt: str = "Validator whose fund to allocate to — answer with a number, "
    "name, or hotkey; Enter shows more.",
    exclude: Optional[set[str]] = None,
) -> dict:
    """Numbered picker over all validator baskets; returns the chosen record.

    ``records`` are normalized fund summaries (``root_baskets`` read passed
    through ``normalize_positions``), sorted by the caller. Shown one page at a
    time (Enter reveals more), all the way through the near-zero-NAV tail.
    ``exclude`` drops hotkeys (e.g. the move source) from the list.
    """
    listed = [record for record in records if not exclude or record["hotkey"] not in exclude]
    if not listed:
        app_ctx.output.message("no validator baskets found")
        raise typer.Exit(0)
    names = local_address_names(app_ctx.wallet_path)
    unnamed = [r["hotkey"] for r in listed if r["hotkey"] not in names]
    identities = app_ctx.run(lambda c: chain_identity_names(c, unnamed)) if unnamed else {}

    console.print(prompt_header(flag, prompt))

    number_width = len(str(len(listed)))
    entities = []
    for index, record in enumerate(listed, start=1):
        # Stash the resolved label so the caller can describe the chosen fund
        # (e.g. root allocate's confirmation line) without re-reading names.
        record["name"] = names.get(record["hotkey"]) or identities.get(record["hotkey"])
        cell = Text()
        cell.append(f"{str(index).rjust(number_width)}. ", style=STYLE_KEY)
        cell.append_text(
            app_ctx.output.account_text(record["hotkey"], record["name"], kind="hotkey")
        )
        entities.append(cell)
    rows = [
        [
            f"{record['display_price_tao']:.4f}",
            f"{record['vs_index']:+.2%}",
            f"{record['nav_tao'].tao:,.4f}",
        ]
        for record in listed
    ]

    page_size = 20
    shown = 0

    def show_page() -> None:
        nonlocal shown
        page = slice(shown, shown + page_size)
        app_ctx.output.entity_list(
            "",
            "validator",
            entities[page],
            ["rate (τ/β)", "vs index", "nav (τ)"],
            rows[page],
            listed[page],
        )
        shown = min(shown + page_size, len(listed))
        left = len(listed) - shown
        if left:
            console.print(
                prompt_hint(
                    f"Enter for {min(page_size, left)} more ({shown} of {len(listed)} shown)"
                )
            )

    show_page()

    if len(listed) == 1:
        console.print(prompt_hint("only validator — selected"))
        return listed[0]

    matchable = {str(i): record for i, record in enumerate(listed, start=1)}
    matchable.update({record["hotkey"]: record for record in listed})
    for record in listed:
        label = _hotkey_label(record["hotkey"], names, identities)
        if label not in matchable:
            matchable[label] = record

    ask_line = Text("  ")
    ask_line.append(flag.lstrip("-"), style=STYLE_COMMAND)
    ask_line.append(": ", style=STYLE_COMMAND)
    while True:
        raw = console.input(ask_line).strip()
        if not raw:
            if shown < len(listed):
                show_page()
                continue
            console.print(prompt_hint("end of list — enter a number, name, or hotkey"))
            continue
        if raw in matchable:
            return matchable[raw]
        close = [
            key
            for key, record in matchable.items()
            if key.startswith(raw) and record["hotkey"].startswith(raw)
        ]
        if len(close) == 1:
            return matchable[close[0]]
        console.print(prompt_hint("enter a list number or hotkey"))


def print_command_hint(console: Console, argv_prefix: list[str]) -> None:
    """Replay command as a tabbed `hint` row, aligned with `--hotkey` / `--amount`."""
    command = Text(" ".join(argv_prefix), style=STYLE_COMMAND)
    console.print(kv_line("hint", PROMPT_KEY_WIDTH, command, key_style=STYLE_HINT), soft_wrap=True)


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
        app_ctx.output.entity_list(
            f"weights of {hotkey}",
            "subnet",
            [app_ctx.output.subnet_text(w["netuid"]) for w in weights],
            ["share", "weight (u16)"],
            [[f"{w['share']:.2%}", str(w["weight"])] for w in weights],
            weights,
        )
    else:
        app_ctx.output.message(
            f"no custom root weights on {hotkey}: "
            "dividends accumulate in place on their origin subnet"
        )

    lifetime = summary.get("lifetime_return")
    fund_yield = staker_yield(summary)
    nav_line = (
        f"nav {summary['nav_tao']} (spot {summary['spot_nav_tao']})  ·  "
        f"deposited {summary['deposited_tao']}, redeemed {summary['redeemed_tao']}"
        + (f"  ·  lifetime {lifetime:.4f}x" if lifetime is not None else "")
        + (f"  ·  staker yield {fund_yield:.2%}" if fund_yield is not None else "")
    )
    if holdings:
        app_ctx.output.entity_list(
            f"fund holdings of {hotkey}",
            "subnet",
            [app_ctx.output.subnet_text(entry["netuid"]) for entry in holdings],
            ["holding", "realizable (τ)", "spot (τ)"],
            [
                [
                    str(entry["alpha"]),
                    str(entry["realizable_tao"]),
                    str(entry["spot_tao"]),
                ]
                for entry in holdings
            ],
            holdings,
            footer=f"[dim]{nav_line}[/dim]",
            legend=[
                ("holding", "the fund's alpha balance on that subnet."),
                (
                    "realizable (τ)",
                    "TAO a full redemption of the holding would fetch right now (slippage-aware).",
                ),
                ("spot (τ)", "rate × amount, ignoring slippage."),
                (
                    "lifetime",
                    "money-weighted total return: (NAV + all TAO paid out) ÷ all TAO paid in.",
                ),
                (
                    "staker yield",
                    "β entitlement minted per τ of root stake since the fund's "
                    "first sighting, valued at today's spot rate — what τ1 "
                    "staked over the tracked period earned.",
                ),
            ],
        )
    else:
        app_ctx.output.message(nav_line)


def position_rows(
    positions: list[RootPosition],
    all_wallets: bool,
    *,
    funds: Optional[dict] = None,
) -> list[list[str]]:
    """Value-column rows for root positions (the validator entity cell is
    rendered separately). ``funds`` maps hotkey to a normalized fund summary
    (``display_price_tao``, ``vs_index``, ``index_provisional``).
    """
    funds = funds or {}
    rows = []
    for pos in positions:
        fund = funds.get(pos.hotkey)
        if fund and fund.get("stake_value") is not None:
            rate = f"{fund['stake_value']:.4f}" + ("*" if fund["index_provisional"] else "")
            vs_index = f"{fund['stake_vs_index']:+.2%}"
        else:
            rate, vs_index = "—", "—"
        rows.append(
            ([pos.wallet or "—"] if all_wallets else [])
            + [
                str(pos.staked),
                str(pos.accrued),
                f"{pos.return_pct:+.3%}" if pos.return_pct is not None else "—",
                str(pos.total),
                rate,
                vs_index,
            ]
        )
    return rows


def position_columns(all_wallets: bool) -> list[str]:
    cols = [
        "staked (τ)",
        "accrued (τ)",
        "return",
        "total (τ)",
        "β (τ)",
        "vs index",
    ]
    if all_wallets:
        return ["wallet", *cols]
    return cols
