"""Three commands for fixed-principal subnet loans: open, close and list."""

from __future__ import annotations

from decimal import ROUND_CEILING, Decimal
from enum import Enum
from typing import Optional

import typer

from ...balance import Balance
from ...intents import CloseLoan, OpenLoan
from ...settings import guide_docs_url
from ..context import address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals

Side = Enum("Side", [("short", "short"), ("long", "long")], type=str)

app = typer.Typer(
    no_args_is_help=True,
    help=f"Subnet reserve loans: open, close, list.\n\nGuide: {guide_docs_url('pool-lending')}",
)


def _bound(amount: Balance, percent: float, *, ceiling: bool = False) -> Balance:
    """Integer quote protection with ceilings rounded up and floors down."""
    factor = Decimal(1) + Decimal(str(percent)) / 100 * (1 if ceiling else -1)
    raw = Decimal(amount.rao) * factor
    rao = int(raw.to_integral_value(rounding=ROUND_CEILING)) if ceiling else int(raw)
    return Balance.from_rao(max(rao, 0), amount.netuid)


@app.command("open")
@with_tx_globals
def open_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet to borrow on."),
    side: Side = typer.Option(..., "--side", help="short: TAO collateral; long: alpha collateral."),
    collateral: str = typer.Option(..., "--collateral", help=OpenLoan.field_help("collateral")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=OpenLoan.field_help("hotkey_ss58")
    ),
    max_slippage: float = typer.Option(
        1.0,
        "--max-slippage",
        min=0.0,
        max=99.0,
        help="Principal may be at most this percent below the full runtime quote (default 1%).",
    ),
):
    """Lock collateral and open one short or long on a subnet.

    Shorts borrow and sell alpha, keeping TAO proceeds locked. Longs borrow
    transferable TAO against existing alpha stake. btcli quotes the complete
    opening first and refuses to submit when a quote is unavailable.
    """
    context = ctx_of(ctx)
    hotkey = context.resolve_address("hotkey_ss58", hotkey_ss58)
    intent = OpenLoan(netuid=netuid, side=side.value, collateral=collateral, hotkey_ss58=hotkey)
    with context.output.activity("quoting the loan…"):
        quote = context.run(
            lambda client: client.read(
                "lending_open_quote", netuid=netuid, side=side.value, collateral=collateral
            )
        )
    intent.min_borrow = _bound(quote["principal"], max_slippage)
    context.submit(
        intent,
        card_sections=[
            (
                "Loan",
                [
                    ("collateral", str(intent.collateral)),
                    ("fixed principal", str(quote["principal"])),
                    ("minimum principal", str(intent.min_borrow)),
                    ("interest per year", str(quote["annual_interest"])),
                ],
            )
        ],
    )


@app.command("close")
@with_tx_globals
def close_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your position."),
    repay_from_wallet: bool = typer.Option(
        False,
        "--repay-from-wallet",
        help="Repay a short with alpha on its saved hotkey instead of buying through the AMM.",
    ),
    max_slippage: float = typer.Option(
        1.0,
        "--max-slippage",
        min=0.0,
        max=99.0,
        help="Maximum percentage above quoted payment or below quoted refund (default 1%).",
    ),
):
    """Repay fixed principal and return remaining collateral.

    A short normally buys alpha back using its locked TAO. A long repays
    TAO from your wallet. The full runtime quote sets a payment ceiling and
    refund floor; a failed quote stops submission.
    """
    context = ctx_of(ctx)
    owner = context.review_account()
    if owner is None:
        context.output.error("select the account that owns this position before closing")
        raise typer.Exit(2)
    with context.output.activity("quoting the repayment…"):
        quote = context.run(
            lambda client: client.read(
                "lending_close_quote",
                coldkey_ss58=owner,
                netuid=netuid,
                repay_from_wallet=repay_from_wallet,
            )
        )
    intent = CloseLoan(
        netuid=netuid,
        repay_from_wallet=repay_from_wallet,
        max_payment=_bound(quote["payment"], max_slippage, ceiling=True),
        min_refund=_bound(quote["refund"], max_slippage),
    )
    context.submit(
        intent,
        card_sections=[
            (
                "Repayment",
                [
                    ("quoted payment", str(quote["payment"])),
                    ("maximum payment", str(intent.max_payment)),
                    ("quoted refund", str(quote["refund"])),
                    ("minimum refund", str(intent.min_refund)),
                ],
            )
        ],
    )


@app.command("list")
@with_globals
def list_positions(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", min=1, max=65535, help="Subnet whose loans to list."
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """List open loans on a subnet; optionally select one coldkey."""
    context = ctx_of(ctx)
    owner = (
        context.resolve_address("coldkey_ss58", coldkey_ss58) if coldkey_ss58 is not None else None
    )

    async def _positions(client):
        if owner is None:
            return await client.read("lending_positions", netuid=netuid)
        position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        return [position] if position is not None else []

    positions = context.run(_positions)
    context.output.table(
        f"loans on netuid {netuid}",
        ["owner", "side", "principal", "collateral", "proceeds", "interest due", "runway"],
        [
            [
                position["coldkey"],
                position["side"],
                str(position["principal"]),
                str(position["collateral"]),
                str(position["proceeds"]),
                str(position["interest_due"]),
                f"{position['runway_days']:.0f}d" if position["runway_days"] is not None else "-",
            ]
            for position in positions
        ],
        positions,
    )
