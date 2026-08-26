"""``btcli root move``: claim yield on a source and restake onto a destination."""

from __future__ import annotations

from typing import Optional

import typer
from rich.console import Console

from ...balance import Balance
from ...basket_index import normalize_positions
from ...intents import ALL, MoveStake
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_tx_globals
from ..helpers import chain_identity_names, local_address_names
from ..prompt import confirm_wallet, interactive, record_answers
from ..root_helpers import (
    fetch_root_positions,
    pick_claim_hotkey,
    pick_fund,
    resolve_position_wallet,
    resolve_validator_selector,
)


def _move_review(
    app_ctx: AppContext,
    *,
    owner: str,
    origin: str,
    dest: str,
    origin_name: Optional[str],
    dest_name: Optional[str],
    staked: Balance,
    accrued: Balance,
) -> tuple[str, list[tuple]]:
    """Confirm line and the Move stage of the review card."""
    app_ctx.output.name_address(origin, origin_name)
    app_ctx.output.name_address(dest, dest_name)
    origin_label = origin_name or f"{origin[:8]}…"
    dest_label = dest_name or f"{dest[:8]}…"
    total = Balance.from_rao(staked.rao + accrued.rao)
    line = f"move {total} from {origin_label} to {dest_label} on root"
    if accrued.rao > 0:
        line = f"claim {accrued} then " + line
    rows: list[tuple] = [
        ("wallet", owner),
        ("from", origin),
        ("to", dest),
        ("staked", str(staked)),
        ("accrued", str(accrued)),
        ("total", str(total)),
        (
            "note",
            "claims yield into root stake on the source, then moves the "
            "whole position to the destination — future yield accrues there",
            "dim",
        ),
    ]
    return line, rows


@with_tx_globals
def root_move(
    ctx: typer.Context,
    origin: Optional[str] = typer.Option(
        None,
        "--from",
        help="Source validator: a hotkey ss58, a root UID, or a name. "
        "Omit on a terminal to pick from your root positions.",
        show_default=False,
    ),
    dest: Optional[str] = typer.Option(
        None,
        "--to",
        help="Destination validator: a hotkey ss58, a root UID, or a name. "
        "Omit on a terminal to pick from all validator baskets.",
        show_default=False,
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=ss58_param_help("coldkey_ss58"),
    ),
):
    """Move your whole root position from one validator to another.

    Claims any accrued basket yield on the source (that TAO folds into
    root stake there), then moves all root stake — principal plus the
    just-claimed yield — onto the destination. Future dividends accrue
    on the destination's basket. The position never passes through your
    free balance.

    This is not ``btcli root allocate``: allocate buys basket shares (β)
    with free TAO. A move keeps the value as root stake on a new
    validator.

    Interactively: pick a wallet, a source position, then a destination
    fund.
    """
    app_ctx: AppContext = ctx_of(ctx)
    console = Console()

    if origin is None or dest is None:
        if not interactive(app_ctx):
            missing = []
            if origin is None:
                missing.append("`--from`")
            if dest is None:
                missing.append("`--to`")
            app_ctx.output.error(
                "missing required option: " + " and ".join(missing),
                help="pass `--from` and `--to`, or run on a terminal to pick validators",
            )
            raise typer.Exit(2)

        wallet_name, owner = resolve_position_wallet(app_ctx, coldkey_ss58)
        app_ctx.wallet_name = wallet_name
        app_ctx.wallet_given = True

        with app_ctx.output.activity("reading root positions…"):
            positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))

        if origin is None:
            chosen = pick_claim_hotkey(
                console,
                app_ctx,
                positions,
                flag="--from",
                prompt="Validator to move from — answer with a number, name, or hotkey.",
            )
            record_answers(["--from", chosen.hotkey])
            console.print()
            origin_hotkey = chosen.hotkey
            staked, accrued = chosen.staked, chosen.accrued
        else:
            origin_hotkey = resolve_validator_selector(app_ctx, origin)
            row = next((p for p in positions if p.hotkey == origin_hotkey), None)
            staked = row.staked if row else Balance.from_rao(0)
            accrued = row.accrued if row else Balance.from_rao(0)

        if dest is None:
            with app_ctx.output.activity("fetching validator baskets…"):
                records = app_ctx.run(lambda c: c.read("root_baskets"))
                normalize_positions(records)
                records.sort(key=lambda record: -record["nav_tao"].rao)
            chosen_dest = pick_fund(
                console,
                app_ctx,
                records,
                flag="--to",
                prompt="Validator to move to — answer with a number, name, or "
                "hotkey; Enter shows more.",
                exclude={origin_hotkey},
            )
            record_answers(["--to", chosen_dest["hotkey"]])
            console.print()
            dest_hotkey = chosen_dest["hotkey"]
            dest_name = chosen_dest.get("name")
        else:
            dest_hotkey = resolve_validator_selector(app_ctx, dest)
            dest_name = None
    else:
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        origin_hotkey = resolve_validator_selector(app_ctx, origin)
        dest_hotkey = resolve_validator_selector(app_ctx, dest)
        dest_name = None
        with app_ctx.output.activity("reading your position…"):
            positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
        row = next((p for p in positions if p.hotkey == origin_hotkey), None)
        staked = row.staked if row else Balance.from_rao(0)
        accrued = row.accrued if row else Balance.from_rao(0)

    if origin_hotkey == dest_hotkey:
        app_ctx.output.error("source and destination are the same validator")
        raise typer.Exit(2)
    if staked.rao <= 0 and accrued.rao <= 0:
        app_ctx.output.error(f"no root position to move on {origin_hotkey}")
        raise typer.Exit(1)

    names = local_address_names(app_ctx.wallet_path)
    origin_name = names.get(origin_hotkey)
    if dest_name is None:
        dest_name = names.get(dest_hotkey)
    unnamed = [hk for hk in (origin_hotkey, dest_hotkey) if names.get(hk) is None]
    if unnamed and (origin_name is None or dest_name is None):
        identities = app_ctx.run(lambda c: chain_identity_names(c, unnamed))
        origin_name = origin_name or identities.get(origin_hotkey)
        dest_name = dest_name or identities.get(dest_hotkey)

    confirm_wallet(app_ctx, help_text="Wallet whose coldkey signs this transaction.")
    summary, rows = _move_review(
        app_ctx,
        owner=owner,
        origin=origin_hotkey,
        dest=dest_hotkey,
        origin_name=origin_name,
        dest_name=dest_name,
        staked=staked,
        accrued=accrued,
    )
    app_ctx.submit(
        MoveStake(
            origin_hotkey_ss58=origin_hotkey,
            origin_netuid=0,
            dest_hotkey_ss58=dest_hotkey,
            dest_netuid=0,
            amount_alpha=ALL,
            claim=True,
        ),
        summary=summary,
        card_sections=[("Move", rows)],
    )
