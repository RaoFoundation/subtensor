"""``btcli root``: stake, inspect, and curate the root network in beta (τ).

Beta is the currency of root: staked beta is principal on netuid 0, accrued beta
is yield from a validator's fund. Stake and unstake move β; accrued beta is merged
into staked beta automatically when you unstake more than your staked balance.
"""

from __future__ import annotations

from typing import Optional

import typer
from rich.console import Console

from ...balance import Balance
from ...intents import AddStake, ClaimRoot, RemoveStake, SetRootWeights
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import dust_note, list_coldkeys
from ..prompt import interactive
from ..root_helpers import (
    beta_list_columns,
    beta_list_rows,
    fetch_all_beta_positions,
    fetch_beta_positions,
    filter_dust_beta,
    is_dust_beta,
    pick_validator,
    print_command_hint,
    render_validator_detail,
    resolve_show_wallet,
)
from .weights import _parse_weight_pairs

app = typer.Typer(
    no_args_is_help=True,
    help="Root network: beta stake, validator funds, and dividend weights.",
)


@app.command("list")
@with_globals
def root_list(
    ctx: typer.Context,
    all_wallets: bool = typer.Option(
        False,
        "--all",
        "-a",
        help="List beta for every wallet (unified view).",
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show validators whose total β is below τ0.001. JSON always includes all.",
    ),
):
    """List your beta on root: staked principal and accrued yield per validator.

    Each row shows staked β (principal on netuid 0), accrued β (fund yield,
    realizable τ quote), total β, and the τ value. JSON records use ``beta``,
    ``staked_beta``, and ``accrued_beta``.
    """
    app_ctx: AppContext = ctx_of(ctx)

    if all_wallets:
        coldkeys = list_coldkeys(app_ctx.wallet_path)
        if not coldkeys:
            app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
            raise typer.Exit(1)
        positions = app_ctx.run(lambda c: fetch_all_beta_positions(c, coldkeys))
        title = "beta (all wallets)"
    else:
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        positions = app_ctx.run(lambda c: fetch_beta_positions(c, owner))
        title = f"beta of {owner}"

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
        shown = filter_dust_beta(positions)
        dust = [pos for pos in positions if is_dust_beta(pos)]

    if not shown:
        app_ctx.output.message(f"{title}: none above dust (τ0.001)")
        if dust:
            app_ctx.output.message(dust_note([pos.as_record() for pos in dust]))
        return

    shown_records = [pos.as_record() for pos in shown]
    total = Balance(sum(pos.total_beta.rao for pos in shown))
    app_ctx.output.table(
        title,
        beta_list_columns(all_wallets),
        beta_list_rows(shown),
        shown_records,
    )
    app_ctx.output.message(
        f"total beta: {total} (τ {total.tao:.6f}) — "
        "staked β is principal; accrued β is fund yield (realizable quote)"
    )
    if dust:
        app_ctx.output.message(dust_note([pos.as_record() for pos in dust]))


@app.command("stake")
@with_tx_globals
def root_stake(
    ctx: typer.Context,
    amount_beta: str = typer.Option(
        ...,
        "--amount-beta",
        help="Beta to stake (τ): moved from free balance to root on the validator.",
    ),
    hotkey_ss58: str = typer.Option(
        ...,
        address_cli_name("hotkey_ss58"),
        help="Validator hotkey to stake beta with.",
    ),
):
    """Stake beta on root (τ from free balance → staked β on a validator)."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(AddStake(hotkey_ss58=hotkey, netuid=0, amount_tao=amount_beta))


@app.command("unstake")
@with_tx_globals
def root_unstake(
    ctx: typer.Context,
    amount_beta: str = typer.Option(
        ...,
        "--amount-beta",
        help="Beta to unstake (τ): staked β returned to free balance. "
        "If the amount exceeds staked β, accrued β is merged first.",
    ),
    hotkey_ss58: str = typer.Option(
        ...,
        address_cli_name("hotkey_ss58"),
        help="Validator hotkey to unstake beta from.",
    ),
):
    """Unstake beta from root (staked β → free τ).

    When the amount is larger than your staked β on that validator, accrued β
    is merged into staked β first (one chain transaction), then the unstake
    runs. Pass ``all`` to exit the full position including accrued β.
    """
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    owner = app_ctx.resolve_address("coldkey_ss58", None)

    positions = app_ctx.run(lambda c: fetch_beta_positions(c, owner))
    row = next((p for p in positions if p.hotkey == hotkey), None)
    staked = row.staked_beta if row else Balance.from_rao(0)
    accrued = row.accrued_beta if row else Balance.from_rao(0)
    total = staked.rao + accrued.rao

    if amount_beta.strip().lower() == "all":
        if total == 0:
            app_ctx.output.error(f"no beta on {hotkey}")
            raise typer.Exit(1)
        if accrued.rao > 0:
            app_ctx.output.message("merging accrued β into staked β…")
            app_ctx.submit(ClaimRoot())
            positions = app_ctx.run(lambda c: fetch_beta_positions(c, owner))
            row = next((p for p in positions if p.hotkey == hotkey), None)
            staked = row.staked_beta if row else Balance.from_rao(0)
        app_ctx.submit(RemoveStake(hotkey_ss58=hotkey, netuid=0, amount_alpha="all"))
        return

    try:
        amount_rao = Balance.from_tao(amount_beta).rao
    except Exception as error:
        app_ctx.output.error(f"invalid --amount-beta: {error}")
        raise typer.Exit(1) from error

    if amount_rao > total:
        app_ctx.output.error(
            f"only {Balance.from_rao(total)} β on {hotkey} "
            f"(staked {staked}, accrued {accrued})"
        )
        raise typer.Exit(1)

    if amount_rao > staked.rao and accrued.rao > 0:
        app_ctx.output.message("merging accrued β into staked β…")
        app_ctx.submit(ClaimRoot())
        positions = app_ctx.run(lambda c: fetch_beta_positions(c, owner))
        row = next((p for p in positions if p.hotkey == hotkey), None)
        staked = row.staked_beta if row else Balance.from_rao(0)

    if amount_rao > staked.rao:
        app_ctx.output.error(
            f"after merge, staked β is only {staked}; accrued may be below claim threshold"
        )
        raise typer.Exit(1)

    app_ctx.submit(
        RemoveStake(
            hotkey_ss58=hotkey,
            netuid=0,
            amount_alpha=Balance.from_rao(amount_rao).tao,
        )
    )


@app.command("set-weights")
@with_tx_globals
def root_set_weights(
    ctx: typer.Context,
    weights: str = typer.Option(
        ...,
        "--weights",
        help="Comma-separated netuid:weight pairs (e.g. '0:0.2,4:0.3,8:0.5'). "
        "Netuid 0 holds that share as β (τ) instead of subnet alpha.",
    ),
    version_key: int = typer.Option(
        0, "--version-key", help=SetRootWeights.field_help("version_key")
    ),
):
    """Set how your root dividends are deployed (validator fund weights)."""
    app_ctx: AppContext = ctx_of(ctx)
    pairs = _parse_weight_pairs(weights)
    app_ctx.submit(
        SetRootWeights(
            netuids=sorted(pairs),
            weights=[pairs[netuid] for netuid in sorted(pairs)],
            version_key=version_key,
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
            "no custom weights set: dividends default to 100% root (TAO in the basket)"
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
    more than dust β, then prompts for one to inspect. Pass ``--hotkey`` to skip
    the picker.
    """
    app_ctx: AppContext = ctx_of(ctx)
    console = Console(stderr=True, highlight=False)

    if hotkey_ss58:
        hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
        owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
        your_rows = app_ctx.run(lambda c: fetch_beta_positions(c, owner))
        yours = next((p for p in your_rows if p.hotkey == hotkey), None)
        summary = app_ctx.run(lambda c: c.read("validator_basket_summary", hotkey_ss58=hotkey))
        render_validator_detail(app_ctx, summary, yours)
        return

    wallet_name, owner = resolve_show_wallet(
        console, app_ctx, coldkey_ss58, interactive=interactive(app_ctx)
    )

    async def _fetch(client):
        yours = await fetch_beta_positions(client, owner)
        by_hotkey = {p.hotkey: p for p in yours}
        held = filter_dust_beta(yours)
        if not held:
            return [], by_hotkey, wallet_name, owner

        summaries = await client.read("root_baskets")
        summary_by_hotkey = {row["hotkey"]: row for row in summaries}
        rows = []
        for pos in sorted(held, key=lambda p: -p.total_beta.rao):
            hotkey = pos.hotkey
            summary = summary_by_hotkey.get(hotkey)
            if summary is None:
                summary = await client.read(
                    "validator_basket_summary", hotkey_ss58=hotkey
                )
            rows.append(
                {
                    "hotkey": hotkey,
                    "nav_tao": summary["nav_tao"],
                    "weight_count": len(summary.get("weights") or []),
                    "lifetime_return": summary.get("lifetime_return"),
                    "your_beta_tao": pos.total_beta.tao,
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
