"""``btcli root swap``: rebalance a validator's basket between subnets."""

from __future__ import annotations

from typing import Optional

import typer

from ...intents import ALL, SwapBasket
from ..context import AppContext, address_cli_name, ctx_of
from ..globals import with_tx_globals
from ..prompt import confirm_wallet


def _swap_review(
    app_ctx: AppContext,
    intent: SwapBasket,
    *,
    holdings: list[dict],
    status: Optional[dict],
) -> tuple[str, list[tuple]]:
    """Confirm line and the Swap stage of the review card: the origin holding
    being sold, the destination, and how much of the fund's daily turnover
    budget is left."""
    by_netuid = {int(row["netuid"]): row for row in holdings}
    origin = by_netuid.get(int(intent.origin_netuid))
    dest = by_netuid.get(int(intent.dest_netuid))
    amount = "the whole holding" if intent.amount == ALL else str(intent.amount)
    line = (
        f"sell {amount} on netuid {intent.origin_netuid} to buy netuid "
        f"{intent.dest_netuid} in {intent.hotkey_ss58[:8]}…'s fund"
    )
    rows: list[tuple] = []
    owner = app_ctx.review_account()
    if owner:
        rows.append(("wallet", owner))
    rows.append(("validator", intent.hotkey_ss58))
    rows.append(("sell", f"{amount} (netuid {intent.origin_netuid})"))
    if origin is not None:
        rows.append(("origin holding", f"{origin['alpha']} worth {origin['value_tao']}"))
    rows.append(("buy", f"netuid {intent.dest_netuid}"))
    if dest is not None:
        rows.append(("dest holding", f"{dest['alpha']} worth {dest['value_tao']}"))
    if status is not None:
        rows.append(
            (
                "turnover budget",
                f"{status['remaining_tao']} of {status['budget_tao']} available "
                f"(refills {status['refill_per_block_tao']} per block)",
            )
        )
        if not status["enabled"]:
            rows.append(("warning", "basket trading is disabled network-wide", "red"))
        if status["frozen"]:
            rows.append(("warning", "trading is frozen for this validator", "red"))
    rows.append(
        (
            "note",
            "each leg must fill within 2% of both the subnet's moving and spot price; "
            "the TAO through the middle counts against the fund's daily turnover budget",
            "dim",
        )
    )
    return line, rows


@with_tx_globals
def root_swap(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help="Validator whose fund to rebalance. Defaults to the wallet's own hotkey.",
    ),
    origin_netuid: int = typer.Option(
        ..., "--from", help="Subnet to sell out of (0 = the fund's TAO cash slot)."
    ),
    dest_netuid: int = typer.Option(
        ..., "--to", help="Subnet to buy into (0 = the fund's TAO cash slot)."
    ),
    amount: str = typer.Option(
        ...,
        "--amount",
        help="How much of the origin holding to sell, in the origin subnet's alpha "
        "(TAO when `--from 0`), or `all` for the whole holding.",
    ),
):
    """Rebalance a validator's basket: sell one holding to buy another.

    Sells `--amount` of the fund's `--from` holding for TAO and buys `--to`
    with it. Stakers' entitlements do not change; only the fund's composition
    moves. Sign with the validator's coldkey, or run as its `BasketTrading`
    proxy with `--proxy-for <coldkey>` (the intended setup for a trader
    multisig). Each leg must fill within 2% of both the subnet's moving and
    spot price, the TAO through the middle counts against the fund's daily
    turnover budget, and the destination may not end above the
    concentration cap.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet whose coldkey signs this transaction.")
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    intent = SwapBasket(
        hotkey_ss58=hotkey,
        origin_netuid=origin_netuid,
        dest_netuid=dest_netuid,
        amount=amount,
    )

    async def _fund_context(client) -> tuple[list[dict], Optional[dict]]:
        holdings = await client.read("validator_basket", hotkey_ss58=hotkey)
        status = await client.read("basket_trading_status", hotkey_ss58=hotkey)
        return holdings, status

    try:
        with app_ctx.output.activity("quoting the fund…"):
            holdings, status = app_ctx.run(_fund_context)
    except Exception:
        # Display-only context; a quoting hiccup (or a pre-v4 node) must not block the swap.
        holdings, status = [], None
    summary, rows = _swap_review(app_ctx, intent, holdings=holdings, status=status)
    app_ctx.submit(intent, summary=summary, card_sections=[("Swap", rows)])
