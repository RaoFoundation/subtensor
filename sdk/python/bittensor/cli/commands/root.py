"""``btcli root``: subscribe to, claim from, and curate root validator funds.

Everything is denominated in TAO. Subscribing moves τ from your free balance
into root stake with a validator (assets in). Claiming realizes accrued yield
and optionally withdraws τ back to free balance (assets out).
"""

from __future__ import annotations

from typing import Optional

import typer
from rich.console import Console

from ...balance import Balance
from ...intents import AddStake, ClaimRootWithHotkey, RemoveStake, SetRootWeights
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import dust_note, list_coldkeys
from ..prompt import interactive
from ..root_helpers import (
    fetch_all_root_positions,
    fetch_root_positions,
    filter_dust_positions,
    is_dust_position,
    pick_claim_hotkey,
    pick_validator,
    position_columns,
    position_rows,
    print_command_hint,
    prompt_claim_amount,
    render_validator_detail,
    resolve_claim_wallet,
    resolve_show_wallet,
)
from .weights import _parse_weight_pairs

app = typer.Typer(
    no_args_is_help=True,
    help="Root network: validator funds, dividend weights, and your TAO positions.",
)


@app.command("list")
@with_globals
def root_list(
    ctx: typer.Context,
    all_wallets: bool = typer.Option(
        False,
        "--all",
        "-a",
        help="List root positions for every wallet (unified view).",
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show validators whose total position is below τ0.001. JSON always includes all.",
    ),
):
    """List your root positions: staked principal and accrued yield per validator.

    Each row shows staked τ (principal on netuid 0), accrued τ (fund yield,
    realizable quote), and the total value. JSON records use ``staked_tao``,
    ``accrued_tao``, and ``total_tao``.
    """
    app_ctx: AppContext = ctx_of(ctx)

    if all_wallets:
        coldkeys = list_coldkeys(app_ctx.wallet_path)
        if not coldkeys:
            app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
            raise typer.Exit(1)
        positions = app_ctx.run(lambda c: fetch_all_root_positions(c, coldkeys))
        title = "root positions (all wallets)"
    else:
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
        title = f"root positions of {owner}"

    records = [pos.as_record() for pos in positions]
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

    shown_records = [pos.as_record() for pos in shown]
    total = Balance(sum(pos.total.rao for pos in shown))
    app_ctx.output.table(
        title,
        position_columns(all_wallets),
        position_rows(shown, all_wallets),
        shown_records,
    )
    app_ctx.output.message(
        f"total: {total} (τ {total.tao:.6f}) — "
        "staked is principal; accrued is fund yield (realizable quote)"
    )
    if dust:
        app_ctx.output.message(dust_note([pos.as_record() for pos in dust]))


@app.command("subscribe")
@with_tx_globals
def root_subscribe(
    ctx: typer.Context,
    amount: str = typer.Option(
        ...,
        "--amount",
        help="TAO to subscribe: moved from free balance to root stake with the validator.",
    ),
    hotkey_ss58: str = typer.Option(
        ...,
        address_cli_name("hotkey_ss58"),
        help="Validator whose fund to subscribe to.",
    ),
):
    """Subscribe to a validator's root fund (assets in)."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(AddStake(hotkey_ss58=hotkey, netuid=0, amount_tao=amount))


def _claim_position(
    app_ctx: AppContext,
    *,
    hotkey: str,
    owner: str,
    amount: Optional[str],
) -> None:
    """Realize accrued yield, and optionally withdraw τ to free balance."""
    positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
    row = next((p for p in positions if p.hotkey == hotkey), None)
    staked = row.staked if row else Balance.from_rao(0)
    accrued = row.accrued if row else Balance.from_rao(0)
    total = staked.rao + accrued.rao

    if amount is None:
        if accrued.rao <= 0:
            app_ctx.output.error(f"no accrued yield to claim on {hotkey}")
            raise typer.Exit(1)
        app_ctx.submit(ClaimRootWithHotkey(hotkey_ss58=hotkey))
        return

    if amount.strip().lower() == "all":
        if total == 0:
            app_ctx.output.error(f"nothing to claim on {hotkey}")
            raise typer.Exit(1)
        if accrued.rao > 0:
            app_ctx.output.message("claiming accrued yield into root stake…")
            app_ctx.submit(ClaimRootWithHotkey(hotkey_ss58=hotkey))
        app_ctx.submit(RemoveStake(hotkey_ss58=hotkey, netuid=0, amount_alpha="all"))
        return

    try:
        amount_rao = Balance.from_tao(amount).rao
    except Exception as error:
        app_ctx.output.error(f"invalid --amount: {error}")
        raise typer.Exit(1) from error

    if amount_rao <= 0:
        app_ctx.output.error("--amount must be positive")
        raise typer.Exit(1)

    if amount_rao > total:
        app_ctx.output.error(
            f"only {Balance.from_rao(total)} on {hotkey} (staked {staked}, accrued {accrued})"
        )
        raise typer.Exit(1)

    if amount_rao > staked.rao and accrued.rao > 0:
        app_ctx.output.message("claiming accrued yield into root stake…")
        app_ctx.submit(ClaimRootWithHotkey(hotkey_ss58=hotkey))
        positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
        row = next((p for p in positions if p.hotkey == hotkey), None)
        staked = row.staked if row else Balance.from_rao(0)

    if amount_rao > staked.rao:
        app_ctx.output.error(
            f"after the claim, staked τ is only {staked}; "
            "accrued yield may be below the claim threshold"
        )
        raise typer.Exit(1)

    app_ctx.submit(
        RemoveStake(
            hotkey_ss58=hotkey,
            netuid=0,
            amount_alpha=Balance.from_rao(amount_rao).tao,
        )
    )


@app.command("claim")
@with_tx_globals
def root_claim(
    ctx: typer.Context,
    amount: Optional[str] = typer.Option(
        None,
        "--amount",
        help="TAO to withdraw to free balance (`all` for the full position). "
        "Omit to only claim accrued yield into root stake.",
    ),
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
    """Claim from a validator's root fund.

    Without ``--amount``: realize accrued yield into root stake on that
    validator. With ``--amount`` / ``all``: withdraw to free balance, claiming
    accrued yield first when needed.

    Interactively: pick a wallet, then a validator (staked + accrued shown),
    then optionally an amount.
    """
    app_ctx: AppContext = ctx_of(ctx)
    console = Console()

    if hotkey_ss58 is not None:
        hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        _claim_position(app_ctx, hotkey=hotkey, owner=owner, amount=amount)
        return

    if not interactive(app_ctx):
        app_ctx.output.error(
            "missing required option: `--hotkey`",
            help="pass `--hotkey`, or run on a terminal to pick a root position",
        )
        raise typer.Exit(2)

    wallet_name, owner = resolve_claim_wallet(console, app_ctx, coldkey_ss58, interactive=True)
    app_ctx.wallet_name = wallet_name
    app_ctx.wallet_given = True

    positions = app_ctx.run(lambda c: fetch_root_positions(c, owner))
    chosen = pick_claim_hotkey(console, app_ctx, positions, flag="--hotkey")
    withdraw = amount if amount is not None else prompt_claim_amount(console, chosen)

    hint = ["btcli", "root", "claim", "-w", wallet_name, "--hotkey", chosen.hotkey]
    if withdraw is not None:
        hint += ["--amount", withdraw]
    print_command_hint(console, hint)

    _claim_position(app_ctx, hotkey=chosen.hotkey, owner=owner, amount=withdraw)


@app.command("set-weights")
@with_tx_globals
def root_set_weights(
    ctx: typer.Context,
    weights: str = typer.Option(
        ...,
        "--weights",
        help="Comma-separated netuid:weight pairs (e.g. '0:0.2,4:0.3,8:0.5'). "
        "Netuid 0 holds that share as TAO instead of subnet alpha.",
    ),
):
    """Set how your root dividends are deployed (validator fund weights)."""
    app_ctx: AppContext = ctx_of(ctx)
    pairs = _parse_weight_pairs(weights)
    app_ctx.submit(
        SetRootWeights(
            netuids=sorted(pairs),
            weights=[pairs[netuid] for netuid in sorted(pairs)],
        )
    )


@app.command("get-weights")
@with_globals
def root_get_weights(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Show a validator's root dividend weights (fund allocation)."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    rows = app_ctx.run(lambda c: c.read("validator_root_weights", hotkey_ss58=hotkey))
    if not rows:
        app_ctx.output.detail("root weights", {"hotkey": hotkey, "weights": []})
        app_ctx.output.message(
            "no custom weights set: dividends accumulate in place on their origin subnet"
        )
        return
    table_rows = [[r["netuid"], f"{r['share']:.2%}", r["weight"]] for r in rows]
    app_ctx.output.table(
        f"weights of {hotkey}", ["netuid", "share", "weight (u16)"], table_rows, rows
    )


@app.command("show")
@with_globals
def root_show(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Inspect a validator's fund: weights, holdings, and performance.

    Without ``--hotkey``, prompts for a wallet, lists validators where you hold
    more than dust, then prompts for one to inspect. Pass ``--hotkey`` to skip
    the picker.
    """
    app_ctx: AppContext = ctx_of(ctx)
    console = Console(stderr=True, highlight=False)

    if hotkey_ss58:
        hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        your_rows = app_ctx.run(lambda c: fetch_root_positions(c, owner))
        yours = next((p for p in your_rows if p.hotkey == hotkey), None)
        summary = app_ctx.run(lambda c: c.read("validator_basket_summary", hotkey_ss58=hotkey))
        render_validator_detail(app_ctx, summary, yours)
        return

    wallet_name, owner = resolve_show_wallet(
        console, app_ctx, coldkey_ss58, interactive=interactive(app_ctx)
    )

    async def _fetch(client):
        yours = await fetch_root_positions(client, owner)
        by_hotkey = {p.hotkey: p for p in yours}
        held = filter_dust_positions(yours)
        if not held:
            return [], by_hotkey, wallet_name, owner

        summaries = await client.read("root_baskets")
        summary_by_hotkey = {row["hotkey"]: row for row in summaries}
        rows = []
        for pos in sorted(held, key=lambda p: -p.total.rao):
            hotkey = pos.hotkey
            summary = summary_by_hotkey.get(hotkey)
            if summary is None:
                summary = await client.read("validator_basket_summary", hotkey_ss58=hotkey)
            rows.append(
                {
                    "hotkey": hotkey,
                    "nav_tao": summary["nav_tao"],
                    "weight_count": len(summary.get("weights") or []),
                    "lifetime_return": summary.get("lifetime_return"),
                    "your_tao": pos.total.tao,
                    "summary": summary,
                }
            )
        return rows, by_hotkey, wallet_name, owner

    validator_rows, by_hotkey, wallet_name, owner = app_ctx.run(_fetch)

    if app_ctx.output.json_mode:
        app_ctx.output.value(
            {
                "wallet": wallet_name,
                "coldkey": owner,
                "validators": validator_rows,
            }
        )
        return

    app_ctx.output.message(f"wallet {wallet_name} ({owner})")
    chosen = pick_validator(
        console,
        app_ctx,
        validator_rows,
        flag=address_cli_name("hotkey_ss58"),
    )
    hotkey = chosen["hotkey"]
    summary = chosen["summary"]
    render_validator_detail(app_ctx, summary, by_hotkey.get(hotkey))
    hint_argv = ["btcli"]
    if wallet_name != app_ctx.wallet_name:
        hint_argv += ["-w", wallet_name]
    hint_argv += ["root", "show", "--hotkey", hotkey]
    print_command_hint(console, hint_argv)
