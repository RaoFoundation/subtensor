"""``btcli root``: allocate to, claim from, move, and curate root validator baskets.

Everything is denominated in TAO. Allocating deploys τ from your free
balance into a validator's basket and credits β immediately (assets in).
Claiming sells that β and folds the TAO into root stake (assets out).
``move`` claims any accrued yield on a source validator and restakes the
whole root position (principal + that yield) onto a destination validator.
``list`` is the fund leaderboard (one fund in detail with a validator
argument, your own positions with ``--mine``), ``register`` joins the root
network, and the ``weights`` sub-group curates a validator's dividend
basket. ``show`` remains as a hidden, deprecated alias of ``list``.
``subscribe`` remains as a hidden, deprecated alias of ``allocate``.
"""

from __future__ import annotations

from typing import Optional

import typer
from rich.console import Console

from ..._generated import storage
from ...balance import Balance
from ...basket_index import age_days, index_level, normalize_position
from ...intents import ALL, ClaimRootWithHotkey, RootRegister, StakeIntoBasket
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import (
    DUST_VALUE_TAO,
    chain_identity_names,
    dust_note,
    list_coldkeys,
    local_address_names,
)
from ..prompt import confirm_wallet, interactive, record_answers
from ..root_helpers import (
    fetch_all_root_positions,
    fetch_root_positions,
    filter_dust_positions,
    is_dust_position,
    pick_claim_hotkey,
    pick_fund,
    position_columns,
    position_rows,
    print_command_hint,
    render_validator_detail,
    resolve_position_wallet,
    resolve_validator_selector,
)
from ..tx import resolve_all_amount
from . import root_move, root_weights

app = typer.Typer(
    no_args_is_help=True,
    help="Root network: validator baskets, dividend weights, and your TAO positions."
    f"\n\nGuide: {guide_docs_url('root-reborn')}",
)


def _fund_display_context(app_ctx: AppContext, hotkeys: list[str]) -> tuple[dict, dict]:
    """Fund rates and validator names for the given hotkeys.

    Returns ``(funds, names)``: normalized fund summary per hotkey (index-spliced
    display rate, ``vs_index``) and the best available label per hotkey (local
    wallet / address book first, then on-chain identity).
    """
    if not hotkeys:
        return {}, {}
    wanted = set(hotkeys)
    summaries = app_ctx.run(lambda c: c.read("root_baskets"))
    funds = {s["hotkey"]: normalize_position(s) for s in summaries if s["hotkey"] in wanted}
    local = local_address_names(app_ctx.wallet_path)
    unnamed = [hk for hk in hotkeys if hk not in local]
    identities = app_ctx.run(lambda c: chain_identity_names(c, unnamed)) if unnamed else {}
    names = {hk: local.get(hk) or identities.get(hk) for hk in hotkeys}
    return funds, names


def _enrich_position_records(records: list[dict], funds: dict, names: dict) -> list[dict]:
    for record in records:
        fund = funds.get(record["hotkey"])
        record["name"] = names.get(record["hotkey"])
        record["price_tao"] = fund["display_price_tao"] if fund else None
        record["vs_index"] = fund["vs_index"] if fund else None
        record["index_provisional"] = fund["index_provisional"] if fund else None
    return records


def _render_positions(
    app_ctx: AppContext,
    *,
    all_wallets: bool,
    coldkey_ss58: Optional[str],
    show_dust: bool,
) -> None:
    """Your root positions: staked principal and accrued yield per validator.

    Each row shows the validator's name, staked τ (principal on netuid 0),
    accrued τ (fund yield, realizable quote), the total value, and the fund's
    index-spliced beta rate with its performance vs the basket index. JSON
    records use ``staked_tao``, ``accrued_tao``, ``total_tao``, ``price_tao``,
    and ``vs_index``.
    """
    # Resolve wallets before starting the spinner: resolve_address may prompt
    # interactively, and a prompt under a live spinner cannot take input.
    if all_wallets:
        coldkeys = list_coldkeys(app_ctx.wallet_path)
        if not coldkeys:
            app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
            raise typer.Exit(1)
        owner = None
        title = "root positions (all wallets)"
    else:
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        title = f"root positions of {owner}"

    with app_ctx.output.activity("reading root positions…") as update:
        if all_wallets:
            positions = app_ctx.run(lambda c: fetch_all_root_positions(c, coldkeys))
        else:
            positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))

        update("comparing funds against the index…")
        funds, names = _fund_display_context(app_ctx, sorted({pos.hotkey for pos in positions}))
    records = _enrich_position_records([pos.as_record() for pos in positions], funds, names)
    if app_ctx.output.json_mode:
        app_ctx.output.value(records)
        return

    if not positions:
        app_ctx.output.message(f"{title}: none")
        return

    if show_dust:
        shown = positions
        dust: list = []
    else:
        shown = filter_dust_positions(positions)
        dust = [pos for pos in positions if is_dust_position(pos)]

    if not shown:
        app_ctx.output.message(f"{title}: none above dust (τ0.001)")
        if dust:
            app_ctx.output.message(dust_note([pos.as_record() for pos in dust]))
        return

    shown_records = _enrich_position_records([pos.as_record() for pos in shown], funds, names)
    total = Balance(sum(pos.total.rao for pos in shown))
    entities = [
        app_ctx.output.account_text(pos.hotkey, names.get(pos.hotkey), kind="hotkey")
        for pos in shown
    ]
    app_ctx.output.entity_list(
        title,
        "validator",
        entities,
        position_columns(all_wallets),
        position_rows(shown, all_wallets, funds=funds),
        shown_records,
        footer=f"[dim]total {total}  ·  basket index {index_level():.4f}[/dim]",
        legend=[
            (
                "staked (τ)",
                "your principal: TAO staked on root (netuid 0) with this validator.",
            ),
            (
                "accrued (τ)",
                "unclaimed fund yield: the TAO your accrued beta would pay if "
                "claimed right now (realizable quote).",
            ),
            (
                "return",
                "accrued over staked — yield earned since your last claim "
                "(claims fold yield into stake and reset this meter).",
            ),
            ("total (τ)", "staked + accrued: the full position value."),
            (
                "rate (τ/β)",
                "TAO per beta, index-spliced: starts at the index level of the "
                "fund's launch, so higher = better lifetime performance.",
            ),
            (
                "vs index",
                "the fund's cumulative out/under-performance vs the average "
                "basket; 0% = market-average.",
            ),
        ],
    )
    if dust:
        app_ctx.output.message(dust_note([pos.as_record() for pos in dust]))


def _render_leaderboard(app_ctx: AppContext, show_dust: bool) -> None:
    """The fund leaderboard: every validator basket measured against the index."""

    async def _fetch(client):
        snapshot = await client.at()
        records = await snapshot.read("root_baskets")
        stakes = (
            await snapshot.query_batch(
                storage.SubtensorModule.TotalHotkeyAlpha,
                [[record["hotkey"], 0] for record in records],
            )
            if records
            else []
        )
        return records, stakes, snapshot.block

    with app_ctx.output.activity("loading validators…") as update:
        records, stakes, current_block = app_ctx.run(_fetch)
        level = index_level()
        for record, stake in zip(records, stakes):
            record["stake_tao"] = Balance.from_rao(int(stake or 0))
            normalize_position(record)
            age = age_days(record["index_first_block"], current_block)
            record["age_days"] = age
            spot = record["spot_nav_tao"].tao
            record["redemption_slippage"] = 1 - record["nav_tao"].tao / spot if spot > 0 else None
        records.sort(key=lambda entry: -entry["nav_tao"].rao)

        shown = records if show_dust else [r for r in records if r["nav_tao"].tao >= DUST_VALUE_TAO]
        hidden = len(records) - len(shown)

        update("resolving validator identities…")
        names = local_address_names(app_ctx.wallet_path)
        unnamed = [r["hotkey"] for r in shown if r["hotkey"] not in names]
        identities = app_ctx.run(lambda c: chain_identity_names(c, unnamed)) if unnamed else {}
        for record in shown:
            record["name"] = names.get(record["hotkey"]) or identities.get(record["hotkey"])

    if app_ctx.output.json_mode:
        app_ctx.output.value({"basket_index": level, "funds": records})
        return

    if not shown:
        app_ctx.output.message("no funds above dust (τ0.001); pass --dust to show all")
        return

    entities = [
        app_ctx.output.account_text(record["hotkey"], record.get("name"), kind="hotkey")
        for record in shown
    ]
    columns = ["stake (τ)", "rate (τ/β)", "vs index", "nav (τ)", "slippage", "age (days)"]
    rows = [
        [
            f"{record['stake_tao'].tao:,.4f}",
            f"{record['display_price_tao']:.4f}" + ("*" if record["index_provisional"] else ""),
            f"{record['vs_index']:+.2%}",
            f"{record['nav_tao'].tao:,.4f}",
            f"{record['redemption_slippage']:.2%}"
            if record["redemption_slippage"] is not None
            else "—",
            f"{record['age_days']:.1f}" if record["age_days"] is not None else "—",
        ]
        for record in shown
    ]
    summary = f"basket index {level:.4f}  ·  above index = beating the average basket"
    if hidden:
        summary += f"  ·  {hidden} dust funds hidden (--dust to show)"
    summary += "  ·  your positions: btcli root list --mine"
    app_ctx.output.entity_list(
        "validator baskets vs basket index",
        "validator",
        entities,
        columns,
        rows,
        shown,
        footer=f"[dim]{summary}[/dim]",
        legend=[
            (
                "stake (τ)",
                "total TAO staked to this validator on root (netuid 0).",
            ),
            (
                "rate (τ/β)",
                "TAO per beta. Index-spliced: every fund starts at the index level "
                "of its launch, so higher = better lifetime performance at any age.",
            ),
            (
                "vs index",
                "cumulative out/under-performance vs the average basket "
                "(the index); 0% = market-average.",
            ),
            (
                "nav (τ)",
                "net asset value: the TAO a full redemption of the fund's holdings "
                "would fetch right now (slippage-aware).",
            ),
            (
                "slippage",
                "the haircut between the fund's spot value and its realizable NAV "
                "— what redeeming the whole fund would cost at current pool depth.",
            ),
            ("age (days)", "days since value first entered the fund."),
        ],
    )
    if any(record["index_provisional"] for record in shown):
        app_ctx.output.message(
            "* fund not yet in the frozen baseline table; shown at the index level "
            "until the table is rebuilt"
        )


@app.command("list")
@with_globals
def root_list(
    ctx: typer.Context,
    validator: Optional[str] = typer.Argument(
        None,
        help="Validator to inspect: a hotkey ss58, a root UID, or a name "
        "(address book or on-chain identity, case-insensitive). "
        "Omit for the full fund leaderboard.",
        show_default=False,
    ),
    mine: bool = typer.Option(
        False,
        "--mine",
        "-m",
        help="Your root positions instead: staked principal, accrued yield, "
        "and return per validator.",
    ),
    all_wallets: bool = typer.Option(
        False,
        "--all",
        "-a",
        help="With --mine (implied): positions for every wallet (unified view).",
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show funds (or positions) below τ0.001. JSON always includes all.",
    ),
):
    """The fund leaderboard, one fund in detail, or your own positions.

    Without arguments: every validator basket measured against the basket
    index, sorted by NAV. Rates are index-spliced: each fund's rate starts
    at the index level of its launch, so a mediocre fund sits on the index
    whether it is three days or three years old; `vs index` is the fund's
    out/under-performance against the average basket.

    With a validator (hotkey ss58, root UID, or name): that fund's weights,
    holdings, and performance — plus your position on it, if any.

    With ``--mine``: your root positions per validator — staked τ (principal
    on netuid 0), accrued τ (unclaimed fund yield), and the return since your
    last claim.
    """
    app_ctx: AppContext = ctx_of(ctx)

    if all_wallets:
        mine = True

    if mine:
        if validator is not None:
            app_ctx.output.error(
                "--mine lists your positions across validators; drop the "
                "validator argument, or drop --mine for that fund's detail view"
            )
            raise typer.Exit(2)
        _render_positions(
            app_ctx,
            all_wallets=all_wallets,
            coldkey_ss58=coldkey_ss58,
            show_dust=show_dust,
        )
        return

    if validator is None:
        _render_leaderboard(app_ctx, show_dust)
        return

    # Resolve the wallet before any chain read: the resolver may prompt, and a
    # prompt under a live query spinner cannot take input (same as --mine).
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    hotkey = resolve_validator_selector(app_ctx, validator)

    with app_ctx.output.activity("reading the validator's fund…"):
        your_rows = app_ctx.run(lambda c: fetch_root_positions(c, owner))
        yours = next((p for p in your_rows if p.hotkey == hotkey), None)
        summary = app_ctx.run(lambda c: c.read("validator_basket_summary", hotkey_ss58=hotkey))
    render_validator_detail(app_ctx, summary, yours)

    if validator != hotkey and not app_ctx.output.json_mode:
        console = Console(stderr=True, highlight=False)
        print_command_hint(console, ["btcli", "root", "list", hotkey])


# Deprecated spelling, kept as a hidden alias so scripts and muscle memory
# keep working (same convention as the hidden group aliases in main.py).
app.command("show", hidden=True)(root_list)


def _claim_position(
    app_ctx: AppContext,
    *,
    hotkey: str,
    owner: str,
    accrued: Optional[Balance] = None,
) -> None:
    """Redeem accrued beta into root stake, showing what the claim sells.

    A claim only converts the fund entitlement (beta) into root stake — it
    never touches staked principal. Withdrawing τ to free balance is a
    separate, ordinary unstake: ``btcli stake remove --netuid 0``. The claim
    facts (accrued, rate, redeem estimate, destination) render as the
    "Claim" stage of the pre-sign review card.
    """
    with app_ctx.output.activity("reading your position…"):
        if accrued is None:
            positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
            row = next((p for p in positions if p.hotkey == hotkey), None)
            accrued = row.accrued if row else Balance.from_rao(0)
        summary = (
            app_ctx.run(lambda c: c.read("validator_basket_summary", hotkey_ss58=hotkey))
            if accrued.rao > 0
            else None
        )
        name = local_address_names(app_ctx.wallet_path).get(hotkey)
        if name is None and accrued.rao > 0:
            name = app_ctx.run(lambda c: chain_identity_names(c, [hotkey])).get(hotkey)
    if accrued.rao <= 0:
        app_ctx.output.error(f"no accrued yield to claim on {hotkey}")
        raise typer.Exit(1)

    app_ctx.output.name_address(hotkey, name)
    label = name or f"{hotkey[:8]}…"
    rate = (summary or {}).get("beta_price_tao") or 0.0
    rows: list[tuple] = [
        ("wallet", owner),
        ("validator", hotkey),
        ("accrued", str(accrued)),
    ]
    if rate > 0:
        rows.append(("rate", f"{rate:.4f} τ/β"))
        rows.append(("redeem", f"~{accrued.tao / rate:,.4f} β"))
    rows.append(("destination", f"root stake · {label}"))
    rows.append(
        (
            "note",
            "principal stays staked — withdraw τ with btcli stake remove (netuid 0)",
            "dim",
        )
    )
    app_ctx.submit(
        ClaimRootWithHotkey(hotkey_ss58=hotkey),
        summary=f"claim {accrued} into {label}'s root stake",
        card_sections=[("Claim", rows)],
    )


@app.command("claim")
@with_tx_globals
def root_claim(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help="Validator to claim from. Omit on a terminal to pick from your root positions.",
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=ss58_param_help("coldkey_ss58"),
    ),
):
    """Redeem accrued beta from a validator's root fund into root stake.

    A claim sells your accrued beta back to the fund and folds its TAO
    value into your root stake on that validator — principal is never
    touched. To withdraw τ to free balance afterwards, unstake normally:
    ``btcli stake remove --netuid 0``.

    Interactively: pick a wallet, then a validator (staked + accrued shown).

    ``--dry-run`` (and the confirm step) estimates the reserved inclusion
    fee versus the fee that will settle, compares that spent fee to
    accrued yield, warns when the claim loses money, and refuses when
    free TAO cannot cover the reserve.
    """
    app_ctx: AppContext = ctx_of(ctx)
    console = Console()

    if hotkey_ss58 is not None:
        hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        _claim_position(app_ctx, hotkey=hotkey, owner=owner)
        return

    if not interactive(app_ctx):
        app_ctx.output.error(
            "missing required option: `--hotkey`",
            help="pass `--hotkey`, or run on a terminal to pick a root position",
        )
        raise typer.Exit(2)

    # Resolve the wallet before any chain read: the resolver may prompt, and a
    # prompt under a live query spinner cannot take input (same as root list).
    wallet_name, owner = resolve_position_wallet(app_ctx, coldkey_ss58)
    app_ctx.wallet_name = wallet_name
    app_ctx.wallet_given = True

    with app_ctx.output.activity("reading root positions…"):
        positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
    chosen = pick_claim_hotkey(console, app_ctx, positions, flag="--hotkey")
    record_answers(["--hotkey", chosen.hotkey])
    console.print()  # one blank line between the picker and what follows

    _claim_position(app_ctx, hotkey=chosen.hotkey, owner=owner, accrued=chosen.accrued)


def _allocate_review(
    app_ctx: AppContext,
    intent: StakeIntoBasket,
    *,
    name: Optional[str],
    beta_price_tao: Optional[float],
) -> tuple[str, list[tuple]]:
    """Fund-aware confirm line and the Allocate stage of the review card.

    The intent's own summary talks in hotkeys; an allocation is better
    described in fund terms — the fund's name, its raw beta rate (NAV over
    beta supply), and roughly how many beta the TAO buys. The beta estimate
    is approximate: the mint is priced on the NAV the deposit actually
    adds, so the depositor bears entry slippage and swap fees.
    """
    app_ctx.output.name_address(intent.hotkey_ss58, name)
    label = name or f"{intent.hotkey_ss58[:8]}…"
    amount = "ALL free TAO" if intent.amount_tao == ALL else str(intent.amount_tao)
    line = f"allocate {amount} into {label}'s fund"
    rows: list[tuple] = []
    owner = app_ctx.review_account()
    if owner:
        rows.append(("wallet", owner))
    rows.append(("validator", intent.hotkey_ss58))
    rows.append(("amount", amount))
    if beta_price_tao and beta_price_tao > 0:
        rows.append(("rate", f"{beta_price_tao:.4f} τ/β"))
        if intent.amount_tao != ALL:
            received = intent.amount_tao.tao / beta_price_tao
            rows.append(("receive", f"~{received:,.4f} β"))
            line += f" at {beta_price_tao:.4f} τ/β for ~{received:,.4f} β"
        else:
            line += f" at {beta_price_tao:.4f} τ/β"
    rows.append(
        (
            "note",
            "deploys this TAO into the fund's holdings and credits β at the value "
            "actually added — you bear entry slippage and swap fees",
            "dim",
        )
    )
    return line, rows


@app.command("allocate")
@with_tx_globals
def root_allocate(
    ctx: typer.Context,
    amount: Optional[str] = typer.Option(
        None,
        "--amount",
        help="TAO to deploy into the validator's basket. You receive β immediately. "
        "Pass `all` for the entire free balance minus the existential deposit and fee headroom.",
    ),
    all_amount: bool = typer.Option(
        False,
        "--all",
        help="Allocate the entire free balance minus the existential deposit and fee headroom "
        "(same as `--amount all`).",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help="Validator whose fund to allocate to. Omit on a terminal to pick "
        "from all validator baskets.",
    ),
):
    """Buy shares in a validator's basket with free TAO."""
    app_ctx: AppContext = ctx_of(ctx)

    def _submit_allocation(
        hotkey: str, resolved: str, *, name: Optional[str], beta_price_tao: Optional[float]
    ) -> None:
        # Confirm the wallet before submit so a saved multisig (e.g. MULTIX)
        # is wrapped and planned as signatory rounds — same path as claim.
        confirm_wallet(
            app_ctx,
            help_text="Wallet whose coldkey signs this transaction.",
        )
        intent = StakeIntoBasket(hotkey_ss58=hotkey, amount_tao=resolved)
        summary, rows = _allocate_review(app_ctx, intent, name=name, beta_price_tao=beta_price_tao)
        app_ctx.submit(intent, summary=summary, card_sections=[("Allocate", rows)])

    if hotkey_ss58 is not None:
        hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        resolved = resolve_all_amount(app_ctx, amount, all_amount, flag="--amount")

        async def _fund_context(client) -> tuple[Optional[str], Optional[float]]:
            summary = await client.read("validator_basket_summary", hotkey_ss58=hotkey)
            name = local_address_names(app_ctx.wallet_path).get(hotkey)
            if name is None:
                name = (await chain_identity_names(client, [hotkey])).get(hotkey)
            return name, summary["beta_price_tao"]

        try:
            with app_ctx.output.activity("quoting the fund…"):
                name, beta_price_tao = app_ctx.run(_fund_context)
        except Exception:
            # Display-only context; a pricing hiccup must not block the buy.
            name, beta_price_tao = None, None
        _submit_allocation(hotkey, resolved, name=name, beta_price_tao=beta_price_tao)
        return

    if not interactive(app_ctx):
        app_ctx.output.error(
            "missing required option: `--hotkey`",
            help="pass `--hotkey`, or run on a terminal to pick a validator basket",
        )
        raise typer.Exit(2)

    console = Console()
    with app_ctx.output.activity("fetching validator baskets…"):
        records = app_ctx.run(lambda c: c.read("root_baskets"))
        for record in records:
            normalize_position(record)
        records.sort(key=lambda record: -record["nav_tao"].rao)

    chosen = pick_fund(console, app_ctx, records, flag=address_cli_name("hotkey_ss58"))
    record_answers(["--hotkey", chosen["hotkey"]])
    console.print()  # one blank line between the picker and what follows
    resolved = resolve_all_amount(app_ctx, amount, all_amount, flag="--amount")
    _submit_allocation(
        chosen["hotkey"],
        resolved,
        name=chosen.get("name"),
        beta_price_tao=chosen.get("beta_price_tao"),
    )


# Deprecated spelling, kept as a hidden alias so scripts and muscle memory
# keep working (same convention as the hidden `show` alias of `list`).
app.command("subscribe", hidden=True)(root_allocate)
app.command("move")(root_move.root_move)


@app.command("register")
@with_tx_globals
def root_register(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Register a hotkey on the root network (netuid 0).

    The same flow as `btcli subnets register --netuid 0`: the coldkey pays
    the current root burn price (fully recycled). No prior stake is needed,
    and a full root network prunes its lowest-staked non-immune member.
    A new seat is immune for the current root immunity period so it can
    attract stake before the next registration can evict it. Registration
    is what lets the hotkey receive root stake and curate its dividend
    basket (`btcli root weights`).
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(RootRegister(hotkey_ss58=hotkey))


app.add_typer(root_weights.app, name="weights")
