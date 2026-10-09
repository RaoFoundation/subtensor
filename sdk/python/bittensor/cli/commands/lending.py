"""Three commands for fixed-principal subnet loans: open, close and list."""

from __future__ import annotations

from decimal import ROUND_CEILING, Decimal
from enum import Enum
from typing import Optional

import typer

from ...balance import Balance
from ...intents import CloseLoan, OpenLoan
from ...intents._money import alpha_amount, tao_amount
from ...result import BittensorError, ChainError
from ...settings import guide_docs_url
from ..context import address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..prompt import PromptSpec, fill_missing, interactive

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


async def _minimum_collateral(client, netuid: int, side: str, owner: str, hotkey: str):
    """Search pinned runtime quotes, including the owner's existing loan and caps."""
    view = await client.at()
    target = max(1_000_000_000, int(await view.constant(("Lending", "MinimumLoanValue"))))
    unit = 0 if side == "short" else netuid

    async def classify(amount: int) -> int:
        try:
            quote = await view.read(
                "lending_open_quote",
                netuid=netuid,
                side=side,
                collateral=Balance.from_rao(amount, unit),
                coldkey_ss58=owner,
                hotkey_ss58=hotkey,
            )
        except ChainError as error:
            if error.name == "AmountTooSmall":
                return -1
            if error.name in ("BorrowingLimit", "InsufficientReserves"):
                return 1
            raise
        return 0 if quote["opening_value"].rao >= target else -1

    low, high = 0, 1_000_000_000
    maximum = (1 << 64) - 1
    found = False
    for _ in range(64):
        result = await classify(high)
        if result >= 0:
            found = result == 0
            break
        low = high
        if high == maximum:
            raise BittensorError("no collateral amount can quote the minimum loan")
        high = min(high * 2, maximum)
    for _ in range(64):
        if high - low <= 1:
            break
        middle = (low + high) // 2
        result = await classify(middle)
        if result < 0:
            low = middle
        else:
            high = middle
            found = result == 0
    if not found:
        raise BittensorError("available lending inventory cannot quote the minimum loan")
    return view.balance(high, unit), Balance.from_rao(target)


def _collateral_prompt(context, netuid: int, side: str, owner: str, hotkey: str) -> str:
    spec = PromptSpec(
        field="collateral",
        flag="--collateral",
        help=OpenLoan.field_help("collateral"),
        parse=lambda _ctx, value: (
            tao_amount(value) if side == "short" else alpha_amount(value, netuid)
        ),
    )
    if interactive(context):
        with context.output.activity("estimating minimum collateral…"):
            minimum, target = context.run(
                lambda client: _minimum_collateral(client, netuid, side, owner, hotkey)
            )
        value = format(minimum.decimal, "f")
        spec.help += (
            f" Estimated minimum additional collateral: {minimum} for {target} "
            + ("of borrowed alpha's opening value." if side == "short" else "of borrowed TAO.")
            + " Based on a current quote; the amount is requoted before submission."
        )
        spec.default = value
    answers = {"collateral": None}
    fill_missing(context, [spec], answers)
    return format(answers["collateral"].decimal, "f")


@app.command("open")
@with_tx_globals
def open_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet to borrow on."),
    side: Side = typer.Option(..., "--side", help="short: TAO collateral; long: alpha collateral."),
    collateral: Optional[str] = typer.Option(
        None, "--collateral", help=OpenLoan.field_help("collateral")
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=OpenLoan.field_help("hotkey_ss58")
    ),
    max_slippage: float = typer.Option(
        1.0,
        "--max-slippage",
        min=0.0,
        max=99.0,
        help="Maximum fall in quoted principal or short opening value (percent; default 1%).",
    ),
):
    """Add collateral and open or increase a short or long on a subnet.

    Shorts receive freely usable alpha on the selected hotkey; opening does
    not sell it. Longs borrow transferable TAO against existing alpha stake.
    An existing position must
    use the same side and hotkey. btcli quotes additional debt after checking
    the combined position and refuses to submit when a quote is unavailable.
    """
    context = ctx_of(ctx)
    if collateral is None and not interactive(context):
        fill_missing(
            context,
            [PromptSpec("collateral", "--collateral", None, lambda _ctx, value: value)],
            {"collateral": None},
        )
    owner = context.review_account()
    if owner is None:
        context.output.error("select the account that owns this loan before borrowing")
        raise typer.Exit(2)
    hotkey = context.resolve_address("hotkey_ss58", hotkey_ss58)
    if collateral is None:
        collateral = _collateral_prompt(context, netuid, side.value, owner, hotkey)
    intent = OpenLoan(netuid=netuid, side=side.value, collateral=collateral, hotkey_ss58=hotkey)
    with context.output.activity("quoting the loan…"):
        quote = context.run(
            lambda client: client.read(
                "lending_open_quote",
                netuid=netuid,
                side=side.value,
                collateral=collateral,
                coldkey_ss58=owner,
                hotkey_ss58=hotkey,
            )
        )
    intent.min_borrow = _bound(quote["principal"], max_slippage)
    if side == Side.short:
        intent.min_proceeds = _bound(quote["opening_value"], max_slippage)
    context.submit(
        intent,
        card_sections=[
            (
                "Loan",
                [
                    ("additional collateral", str(intent.collateral)),
                    ("additional fixed principal", str(quote["principal"])),
                    ("minimum additional principal", str(intent.min_borrow)),
                    ("minimum additional opening value", str(intent.min_proceeds)),
                    ("additional interest per year", str(quote["annual_interest"])),
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
        True,
        "--repay-from-wallet/--no-repay-from-wallet",
        help="Repay short alpha from its saved hotkey; disable to buy with remaining collateral.",
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

    A short normally supplies alpha from its saved hotkey. Opt out to buy
    that alpha using only remaining TAO collateral. A long repays
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
        ["owner", "side", "principal", "collateral", "interest due", "runway"],
        [
            [
                position["coldkey"],
                position["side"],
                str(position["principal"]),
                str(position["collateral"]),
                str(position["interest_due"]),
                f"{position['runway_days']:.0f}d" if position["runway_days"] is not None else "-",
            ]
            for position in positions
        ],
        positions,
    )
