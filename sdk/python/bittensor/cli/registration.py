"""Explicit approval of automatic recipient-registration deposits."""

from __future__ import annotations

import re
import sys
from decimal import Decimal, InvalidOperation

import typer

from ..balance import Balance


def parse_registration_deposit(value: str | None) -> Balance | None:
    """Parse an exact, non-negative native-token budget, never rounding consent."""
    if value is None:
        return None
    try:
        if not re.fullmatch(r"(?:[0-9]+(?:\.[0-9]{1,9})?|\.[0-9]{1,9})", value):
            raise ValueError
        amount = Decimal(value)
        if not amount.is_finite() or amount < 0 or amount > Decimal(2**64 - 1) / 10**9:
            raise ValueError
        rao = amount * 10**9
        if rao != rao.to_integral_value():
            raise ValueError
        return Balance.from_rao(int(rao))
    except (InvalidOperation, ValueError, OverflowError) as error:
        raise typer.BadParameter(
            "use a non-negative TAO amount with at most nine decimal places",
            param_hint="--max-registration-deposit",
        ) from error


def confirm_registration_sponsorship(app_ctx, quote) -> bool:
    """A general --yes is not permission to lock an additional deposit."""
    amount = format(quote.deposit.decimal, "f")
    maximum = app_ctx.max_registration_deposit
    details = (
        f"register recipient account {quote.recipient_account} on {app_ctx.network} "
        f"and permanently lock {amount} TAO from {quote.payer_account}, "
        "in addition to the transfer and transaction fees"
    )
    if maximum is not None:
        if quote.deposit.rao > maximum.rao:
            app_ctx.output.error(
                f"registration requires {amount} TAO, exceeding --max-registration-deposit "
                f"{maximum.decimal:f}",
                help="review the deposit and explicitly increase the limit if you accept it",
            )
            raise typer.Exit(1)
        app_ctx.output.message(f"Registration sponsorship authorized: {details}.")
        return True
    if app_ctx.output.json_mode or not sys.stdin.isatty():
        app_ctx.output.error(
            f"explicit registration sponsorship consent required to {details}",
            help=f"pass --max-registration-deposit {amount} to authorize this deposit; "
            "--yes alone does not authorize registration sponsorship",
        )
        raise typer.Exit(1)
    try:
        accepted = app_ctx.output.confirm(f"{details[0].upper() + details[1:]}?")
    except (EOFError, KeyboardInterrupt):
        app_ctx.output.message("aborted.")
        raise typer.Exit(130) from None
    if not accepted:
        app_ctx.output.message("aborted; no transaction submitted.")
        raise typer.Exit(1)
    return True
