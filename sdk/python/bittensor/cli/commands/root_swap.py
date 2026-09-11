"""``btcli root swap``: rebalance a validator's basket between subnets."""

from __future__ import annotations

from decimal import Decimal
from typing import Optional

import typer

from ...balance import Balance
from ...intents import ALL, SwapBasket
from ..context import AppContext, address_cli_name, ctx_of
from ..globals import with_tx_globals
from ..prompt import confirm_wallet

DEFAULT_MAX_SLIPPAGE_PCT = 1.0


def _min_amount_out(expected: Balance, max_slippage_pct: float) -> Balance:
    """The user floor: ``expected × (1 - max_slippage / 100)``, floored to whole rao."""
    factor = Decimal(1) - Decimal(str(max_slippage_pct)) / Decimal(100)
    rao = int(Decimal(expected.rao) * factor)
    return Balance.from_rao(max(rao, 0), expected.netuid)


def _swap_review(
    app_ctx: AppContext,
    intent: SwapBasket,
    *,
    holdings: list[dict],
    status: Optional[dict],
    expected_out: Optional[Balance],
    max_slippage_pct: float,
) -> tuple[str, list[tuple]]:
    """Confirm line and the Swap stage of the review card: the origin holding
    being sold, the destination, the quoted and minimum output, and how much
    of the fund's daily turnover budget is left."""
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
    if expected_out is not None:
        rows.append(("expected out", f"~{expected_out} (quoted now, after fees)"))
        rows.append(
            (
                "minimum out",
                f"{intent.min_amount_out} (expected less {max_slippage_pct:g}% max slippage; "
                "the trade rolls back below this)",
            )
        )
    else:
        rows.append(
            (
                "warning",
                "no quote available: minimum out is 0, so only the 2% protocol band "
                "bounds this trade",
                "yellow",
            )
        )
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
    max_slippage: float = typer.Option(
        DEFAULT_MAX_SLIPPAGE_PCT,
        "--max-slippage",
        min=0.0,
        max=100.0,
        help="Your floor on the fill, in percent of the quoted output: the trade rolls "
        "back (`BasketMinOutNotMet`) if the buy leg credits less than "
        "`quote × (1 - max_slippage/100)`. This is on top of the chain's 2% per-leg band. "
        "100 disables the floor.",
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

    btcli quotes the trade first and sets `min_amount_out` to the quoted
    output less `--max-slippage` (default 1%), so a pool that moves against
    you between the quote and execution rolls the trade back instead of
    filling worse. If the quote is unavailable the floor is 0 (chain band only).
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet whose coldkey signs this transaction.")
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    # Validate and normalize the inputs first (same-subnet check, amount units);
    # the floor is filled in once the trade has been quoted.
    intent = SwapBasket(
        hotkey_ss58=hotkey,
        origin_netuid=origin_netuid,
        dest_netuid=dest_netuid,
        amount=amount,
    )

    def _origin_amount(holdings: list[dict]) -> Optional[Balance]:
        if intent.amount != ALL:
            return intent.amount
        for row in holdings:
            if int(row["netuid"]) == int(origin_netuid):
                return row["alpha"]
        return None

    async def _quote_out(client, sell: Balance) -> Balance:
        """Expected destination credit: the sell leg's TAO through the origin pool,
        then the buy leg through the destination pool. Root legs are TAO 1:1."""
        if origin_netuid == 0:
            tao_mid = sell
        else:
            leg = await client.read(
                "quote_unstake", netuid=origin_netuid, amount_alpha=sell.amount
            )
            tao_mid = leg.tao
        if dest_netuid == 0:
            return tao_mid
        leg = await client.read("quote_stake", netuid=dest_netuid, amount_tao=tao_mid.tao)
        return leg.alpha

    async def _fund_context(
        client,
    ) -> tuple[list[dict], Optional[dict], Optional[Balance]]:
        holdings = await client.read("validator_basket", hotkey_ss58=hotkey)
        status = await client.read("basket_trading_status", hotkey_ss58=hotkey)
        expected: Optional[Balance] = None
        sell = _origin_amount(holdings)
        if sell is not None and sell.rao > 0:
            try:
                expected = await _quote_out(client, sell)
            except Exception:
                # The floor is a convenience on top of the chain band: no quote, no floor.
                expected = None
        return holdings, status, expected

    try:
        with app_ctx.output.activity("quoting the fund…"):
            holdings, status, expected_out = app_ctx.run(_fund_context)
    except Exception:
        # Display-only context; a quoting hiccup (or a pre-v4 node) must not block the swap.
        holdings, status, expected_out = [], None, None

    if expected_out is not None and expected_out.rao > 0:
        intent = SwapBasket(
            hotkey_ss58=hotkey,
            origin_netuid=origin_netuid,
            dest_netuid=dest_netuid,
            amount=amount,
            min_amount_out=_min_amount_out(expected_out, max_slippage),
        )
    else:
        expected_out = None

    summary, rows = _swap_review(
        app_ctx,
        intent,
        holdings=holdings,
        status=status,
        expected_out=expected_out,
        max_slippage_pct=max_slippage,
    )
    app_ctx.submit(intent, summary=summary, card_sections=[("Swap", rows)])
