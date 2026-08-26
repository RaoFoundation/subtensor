"""``btcli root weights``: curate a validator's root dividend basket.

The CLI always submits equal weights: every chosen destination subnet gets
exactly 1/N of the validator's root dividends (netuid 0 holds its share as
TAO / root stake). ``set`` replaces the whole allocation; ``add`` and
``remove`` edit the current non-zero set and renormalize. The chain enforces
a diversity floor and a concentration cap (1/16 at launch), so an equal
split needs at least 16 destinations on mainnet.
"""

from __future__ import annotations

from typing import Optional

import typer
from rich.console import Console
from rich.text import Text

from ... import wallets
from ..._generated import storage
from ...basket_index import normalize_positions
from ...intents import SetRootWeights
from ...intents.weights import DEFAULT_ROOT_WEIGHTS_CAP
from ...settings import U16_MAX, guide_docs_url
from ..context import AppContext, ctx_of
from ..globals import with_globals, with_tx_globals
from ..output import (
    CHOICE_INDENT,
    STYLE_COMMAND,
    STYLE_NAME,
    prompt_header,
    prompt_hint,
)
from ..prompt import PromptSpec, ask, confirm_wallet, interactive, record_answers
from ..root_helpers import pick_fund, print_command_hint, resolve_validator_selector

app = typer.Typer(
    no_args_is_help=True,
    help="Root dividend weights: an equal 1/N allocation across chosen subnets."
    f"\n\nGuide: {guide_docs_url('root-reborn')}",
)

# Chain-side diversity floor (`MIN_ROOT_BASKET_WEIGHTS` in the subtensor
# pallet): the minimum number of positive destinations `set_root_weights`
# accepts, softened when fewer networks exist.
MIN_ROOT_BASKET_WEIGHTS = 8

NETUIDS_HELP = (
    "Comma-separated destination subnets (e.g. 4,8,23). Each gets an equal "
    "1/N share of root dividends; netuid 0 holds its share as TAO (root stake)."
)


def _parse_netuids(raw: str) -> list[int]:
    """Comma-separated netuids to an ordered, duplicate-free list."""
    out: list[int] = []
    for part in raw.split(","):
        part = part.strip()
        if not part:
            continue
        try:
            netuid = int(part)
        except ValueError:
            raise ValueError(f"expected comma-separated netuids like '4,8,23', got {part!r}")
        if netuid < 0:
            raise ValueError(f"netuid must be non-negative, got {netuid}")
        if netuid in out:
            raise ValueError(f"duplicate netuid {netuid}")
        out.append(netuid)
    if not out:
        raise ValueError("no netuids given")
    return out


def _netuids_from_flag(app_ctx: AppContext, raw: str, flag: str) -> list[int]:
    try:
        return _parse_netuids(raw)
    except ValueError as error:
        app_ctx.output.error(f"invalid {flag}: {error}")
        raise typer.Exit(2) from error


def _prompt_netuids(app_ctx: AppContext, *, flag: str, help_text: str) -> list[int]:
    """Ask for the netuid list interactively; script-safe error otherwise."""
    if not interactive(app_ctx):
        app_ctx.output.error(
            f"missing required option: `{flag}`",
            help="pass the option explicitly, or run on a terminal to be prompted",
        )
        raise typer.Exit(2)
    console = Console(stderr=True, highlight=False)
    spec = PromptSpec(
        field="netuids",
        flag=flag,
        help=help_text,
        parse=lambda _app_ctx, raw: _parse_netuids(raw),
    )
    value, raw = ask(console, app_ctx, spec)
    record_answers([flag, raw])
    return value


def _require_root_hotkey(app_ctx: AppContext) -> str:
    """The signing hotkey, verified registered on root (netuid 0) — fail fast.

    With ``--wallet-hotkey`` the named hotkey is used directly (and its root
    registration verified). Otherwise the hotkey is discovered: the wallet's
    local hotkeys are checked against root registration on-chain, a single
    match is picked automatically, and several offer an indexed choice. The
    discovered choice is recorded so the skip-the-prompts hint shows
    ``--wallet-hotkey`` for next time. The wallet confirmation always runs
    before any chain read.
    """
    confirm_wallet(
        app_ctx,
        help_text="Wallet containing the signing hotkey.",
        require_coldkey=False,
    )

    if app_ctx.hotkey_given:
        try:
            hotkey = app_ctx.wallet().hotkey.ss58_address
        except Exception as error:
            app_ctx.output.error(
                f"wallet {app_ctx.wallet_name!r} has no usable hotkey "
                f"{app_ctx.hotkey_name!r}: {error}"
            )
            raise typer.Exit(1) from error
        app_ctx.output.name_address(hotkey, f"{app_ctx.wallet_name}/{app_ctx.hotkey_name}")
        app_ctx.output.classify_address(hotkey, "hotkey")
        with app_ctx.output.activity("checking root registration…"):
            uid = app_ctx.run(lambda c: c.read("uid", netuid=0, hotkey_ss58=hotkey))
        if uid is None:
            app_ctx.output.error(
                f"hotkey {hotkey} is not registered on the root network (netuid 0)",
                help="register it first: `btcli root register`",
            )
            raise typer.Exit(1)
        return hotkey

    wallet_info = next(
        (
            ck
            for ck in wallets.list_wallets_detailed(app_ctx.wallet_path)
            if ck.name == app_ctx.wallet_name
        ),
        None,
    )
    candidates = [
        (hk.name, hk.ss58) for hk in (wallet_info.hotkeys if wallet_info else []) if hk.ss58
    ]
    if not candidates:
        app_ctx.output.error(
            f"wallet {app_ctx.wallet_name!r} has no local hotkeys to sign with",
            help="pass `--wallet-hotkey <name>` of a wallet that holds the root-registered hotkey",
        )
        raise typer.Exit(1)

    async def _registered(client):
        out = []
        for name, ss58 in candidates:
            uid = await client.read("uid", netuid=0, hotkey_ss58=ss58)
            if uid is not None:
                out.append((name, ss58, uid))
        return out

    with app_ctx.output.activity("finding root-registered hotkeys…"):
        registered = app_ctx.run(_registered)

    if not registered:
        app_ctx.output.error(
            f"no hotkey in wallet {app_ctx.wallet_name!r} is registered on the "
            "root network (netuid 0)",
            help="register one first: `btcli root register`",
        )
        raise typer.Exit(1)

    if len(registered) == 1:
        name, hotkey, uid = registered[0]
    else:
        if not interactive(app_ctx):
            listing = ", ".join(name for name, _ss58, _uid in registered)
            app_ctx.output.error(
                f"wallet {app_ctx.wallet_name!r} has several root-registered hotkeys ({listing})",
                help="pass `--wallet-hotkey <name>` to choose one",
            )
            raise typer.Exit(2)
        console = Console(stderr=True, highlight=False)
        console.print(
            prompt_header(
                "--wallet-hotkey",
                f"Root-registered hotkey in wallet {app_ctx.wallet_name!r} that signs "
                "this transaction — answer with a number or name.",
            )
        )
        number_width = len(str(len(registered)))
        name_width = max(len(name) for name, _ss58, _uid in registered)
        for index, (name, ss58, uid) in enumerate(registered, 1):
            line = Text(" " * CHOICE_INDENT, overflow="ignore", no_wrap=True)
            line.append(str(index).rjust(number_width), style=STYLE_COMMAND)
            line.append("  ")
            line.append(name.ljust(name_width), style=STYLE_NAME)
            line.append(f"  {ss58} · uid {uid}", style="dim")
            console.print(line, soft_wrap=True)
        by_answer = {str(i): entry for i, entry in enumerate(registered, 1)}
        by_answer.update({entry[0]: entry for entry in registered})
        ask_line = Text("  ")
        ask_line.append("wallet-hotkey", style=STYLE_COMMAND)
        ask_line.append(": ", style=STYLE_COMMAND)
        while True:
            raw = console.input(ask_line).strip()
            if raw in by_answer:
                name, hotkey, uid = by_answer[raw]
                break
            console.print(prompt_hint("enter a list number or hotkey name"))

    # Adopt the discovery so submit signs with this hotkey without re-asking,
    # and the skip-the-prompts hint teaches the explicit flag.
    app_ctx.hotkey_name = name
    app_ctx.hotkey_given = True
    record_answers(["--wallet-hotkey", name])
    app_ctx.output.name_address(hotkey, f"{app_ctx.wallet_name}/{name}")
    app_ctx.output.classify_address(hotkey, "hotkey")
    app_ctx.output.message(f"[dim]signing hotkey: {name} ({hotkey}) — uid {uid} on root[/dim]")
    return hotkey


def _current_netuids(app_ctx: AppContext, hotkey: str) -> list[int]:
    """The validator's current non-zero destination set, sorted."""
    with app_ctx.output.activity("reading current root weights…"):
        rows = app_ctx.run(lambda c: c.read("validator_root_weights", hotkey_ss58=hotkey))
    return sorted({int(r["netuid"]) for r in rows if int(r["weight"]) > 0})


def _validate_destinations(app_ctx: AppContext, netuids: list[int]) -> None:
    """Mirror the chain's destination checks before the confirm prompt.

    ``set_root_weights`` requires every destination to be root (0) or an
    existing subnet, at least ``min(MIN_ROOT_BASKET_WEIGHTS, available)``
    positive destinations, and no destination above the ``RootWeightsCap``
    share of the vector. For an equal 1/N split the cap means
    N >= ceil(U16_MAX / cap) — 16 at the launch cap — once at least that
    many networks exist (the same softening rule the chain applies).
    """

    async def _fetch(client):
        networks = await client.query_map(storage.SubtensorModule.NetworksAdded)
        try:
            # Not in the generated storage catalog yet (spec 500 addition), so
            # queried by name; pre-500 chains have no such item — fall back to
            # the launch default (weight setting is gated off there anyway).
            cap_raw = await client.query(("SubtensorModule", "RootWeightsCap"), [0])
        except Exception:
            cap_raw = None
        return cap_raw, networks

    with app_ctx.output.activity("checking chain weight limits…"):
        cap_raw, networks = app_ctx.run(_fetch)

    existing = {int(netuid) for netuid, added in networks if added}
    unknown = sorted(n for n in netuids if n != 0 and n not in existing)
    if unknown:
        app_ctx.output.error(
            "unknown subnet netuid(s): " + ", ".join(str(n) for n in unknown),
            help="`btcli subnets list` shows every subnet; netuid 0 holds the share as TAO",
        )
        raise typer.Exit(2)

    available = len(existing | {0})
    cap = int(cap_raw) if cap_raw is not None else DEFAULT_ROOT_WEIGHTS_CAP
    minimum = min(MIN_ROOT_BASKET_WEIGHTS, available)
    cap_minimum = -(-U16_MAX // max(cap, 1))  # ceil: smallest N whose 1/N share fits the cap
    if available >= cap_minimum:
        minimum = max(minimum, cap_minimum)
    if len(netuids) < minimum:
        app_ctx.output.error(
            f"{len(netuids)} destination{'s' if len(netuids) != 1 else ''} is below "
            f"the chain minimum of {minimum} for an equal split (diversity floor "
            f"{MIN_ROOT_BASKET_WEIGHTS}; concentration cap ≈ {cap / U16_MAX:.4g} "
            "per destination)",
            help="choose more subnets so each equal 1/N share fits under the cap",
        )
        raise typer.Exit(2)


def _confirm_and_submit(app_ctx: AppContext, netuids: list[int]) -> None:
    """Render the resulting equal allocation, then hand off to ``submit``
    (which shows the fee and asks for confirmation before signing)."""
    ordered = sorted(netuids)
    share = 1 / len(ordered)
    if not app_ctx.output.json_mode:
        rows = [[app_ctx.output.with_subnets(f"netuid {n}"), f"{share:.2%}"] for n in ordered]
        app_ctx.output.table(
            f"resulting allocation ({len(ordered)} destinations, equal weights)",
            ["destination", "share"],
            rows,
        )
    app_ctx.submit(SetRootWeights(netuids=ordered, weights=[1.0] * len(ordered)))


@app.command("show")
@with_globals
def weights_show(
    ctx: typer.Context,
    validator: Optional[str] = typer.Argument(
        None,
        help="Validator whose weights to show: a hotkey ss58, a root UID, or a "
        "name (address book or on-chain identity). Omit to use the wallet "
        "hotkey — or, on a terminal, to pick from all root validators.",
        show_default=False,
    ),
):
    """Show a validator's non-zero root dividend weights (fund allocation)."""
    app_ctx: AppContext = ctx_of(ctx)
    console = Console(stderr=True, highlight=False)

    picked = False
    if validator is not None:
        hotkey = resolve_validator_selector(app_ctx, validator)
    elif app_ctx.wallet_given or app_ctx.hotkey_given or not interactive(app_ctx):
        hotkey = app_ctx.resolve_address("hotkey_ss58", None)
    else:
        with app_ctx.output.activity("fetching root validators…"):
            records = app_ctx.run(lambda c: c.read("root_baskets"))
            normalize_positions(records)
            records.sort(key=lambda record: -record["nav_tao"].rao)
        chosen = pick_fund(
            console,
            app_ctx,
            records,
            flag="validator",
            prompt="Validator whose weights to show — answer with a number, "
            "name, or hotkey; Enter shows more.",
        )
        hotkey = chosen["hotkey"]
        picked = True
        console.print()  # one blank line between the picker and what follows

    with app_ctx.output.activity("reading root weights…"):
        rows = app_ctx.run(lambda c: c.read("validator_root_weights", hotkey_ss58=hotkey))

    if not rows:
        app_ctx.output.detail("root weights", {"hotkey": hotkey, "weights": []})
        app_ctx.output.message(
            "no custom weights set: dividends accumulate in place on their origin subnet"
        )
    else:
        table_rows = [
            [
                app_ctx.output.with_subnets(f"netuid {r['netuid']}"),
                f"{r['share']:.2%}",
                r["weight"],
            ]
            for r in rows
        ]
        app_ctx.output.table(
            f"root weights of {hotkey}",
            ["destination", "share", "weight (u16)"],
            table_rows,
            rows,
        )

    hint_worthy = picked or (validator is not None and validator != hotkey)
    if hint_worthy and not app_ctx.output.json_mode:
        print_command_hint(console, ["btcli", "root", "weights", "show", hotkey])


@app.command("set")
@with_tx_globals
def weights_set(
    ctx: typer.Context,
    netuids: Optional[str] = typer.Option(None, "--netuids", help=NETUIDS_HELP, show_default=False),
):
    """Replace the allocation: equal 1/N weights across the given subnets.

    The signing hotkey must be registered on root (netuid 0) — checked
    before anything else. The resulting allocation is shown and confirmed
    before submission.
    """
    app_ctx: AppContext = ctx_of(ctx)
    _require_root_hotkey(app_ctx)
    if netuids is not None:
        chosen = _netuids_from_flag(app_ctx, netuids, "--netuids")
    else:
        chosen = _prompt_netuids(app_ctx, flag="--netuids", help_text=NETUIDS_HELP)
    _validate_destinations(app_ctx, chosen)
    _confirm_and_submit(app_ctx, chosen)


@app.command("add")
@with_tx_globals
def weights_add(
    ctx: typer.Context,
    netuid: Optional[str] = typer.Option(
        None,
        "--netuid",
        help="Subnet(s) to add to the allocation: one netuid, or several comma-separated.",
        show_default=False,
    ),
):
    """Add subnet(s) to the allocation and renormalize to equal weights.

    Fetches the current non-zero set, adds the new netuid(s), and resubmits
    the whole vector at 1/(new N) each. The signing hotkey must be
    registered on root; the result is shown and confirmed before submission.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = _require_root_hotkey(app_ctx)
    current = _current_netuids(app_ctx, hotkey)
    if current:
        app_ctx.output.message(
            "current destinations: "
            + app_ctx.output.with_subnets(", ".join(f"netuid {n}" for n in current))
        )
    else:
        app_ctx.output.message("no current root weights — this starts a new allocation")

    if netuid is not None:
        added = _netuids_from_flag(app_ctx, netuid, "--netuid")
    else:
        added = _prompt_netuids(
            app_ctx,
            flag="--netuid",
            help_text="Subnet(s) to add: one netuid, or several comma-separated.",
        )

    dupes = sorted(n for n in added if n in current)
    if dupes:
        app_ctx.output.error(
            "already in the allocation: " + ", ".join(f"netuid {n}" for n in dupes),
            help="`btcli root weights show` prints the current set",
        )
        raise typer.Exit(2)

    combined = sorted(set(current) | set(added))
    _validate_destinations(app_ctx, combined)
    _confirm_and_submit(app_ctx, combined)


@app.command("remove")
@with_tx_globals
def weights_remove(
    ctx: typer.Context,
    netuid: Optional[str] = typer.Option(
        None,
        "--netuid",
        help="Subnet(s) to remove from the allocation: one netuid, or several comma-separated.",
        show_default=False,
    ),
):
    """Remove subnet(s) from the allocation and renormalize to equal weights.

    Fetches the current non-zero set, drops the netuid(s), and resubmits the
    rest at equal 1/N. Errors before submission if the result would fall
    below the chain's minimum destination count — or empty the basket.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = _require_root_hotkey(app_ctx)
    current = _current_netuids(app_ctx, hotkey)
    if not current:
        app_ctx.output.error(
            f"{hotkey} has no root weights set — nothing to remove",
            help="`btcli root weights set --netuids ...` creates an allocation",
        )
        raise typer.Exit(1)
    app_ctx.output.message(
        "current destinations: "
        + app_ctx.output.with_subnets(", ".join(f"netuid {n}" for n in current))
    )

    if netuid is not None:
        removed = _netuids_from_flag(app_ctx, netuid, "--netuid")
    else:
        removed = _prompt_netuids(
            app_ctx,
            flag="--netuid",
            help_text="Subnet(s) to remove: one netuid, or several comma-separated.",
        )

    missing = sorted(n for n in removed if n not in current)
    if missing:
        app_ctx.output.error(
            "not in the allocation: " + ", ".join(f"netuid {n}" for n in missing),
            help="`btcli root weights show` prints the current set",
        )
        raise typer.Exit(2)

    remaining = [n for n in current if n not in removed]
    if not remaining:
        app_ctx.output.error(
            "removing every destination would submit an empty vector, which the chain rejects",
            help="set a new allocation instead: `btcli root weights set --netuids ...`",
        )
        raise typer.Exit(2)

    _validate_destinations(app_ctx, remaining)
    _confirm_and_submit(app_ctx, remaining)
