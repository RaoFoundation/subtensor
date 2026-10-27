"""Explicit authorization for recipient registration added by the SDK."""

from __future__ import annotations

import inspect
from collections.abc import Awaitable, Callable
from dataclasses import dataclass

from .balance import Balance
from .result import BittensorError


@dataclass(frozen=True)
class RegistrationSponsorship:
    """The permanent reserve in the composed transaction, separate from its fee.

    ``payer_account`` permanently reserves ``deposit`` to register
    ``recipient_account``. Both accounts use the chain's SS58 representation.
    """

    recipient_account: str
    payer_account: str
    deposit: Balance


RegistrationSponsorshipCallback = Callable[[RegistrationSponsorship], bool | Awaitable[bool]]


class RegistrationSponsorshipRequired(BittensorError):
    """Automatic registration was not explicitly authorized before signing."""

    def __init__(self, quote: RegistrationSponsorship, reason: str):
        self.quote = quote
        super().__init__(
            f"{reason}: registering {quote.recipient_account} permanently reserves "
            f"{quote.deposit.decimal} TAO from {quote.payer_account}, in addition to "
            "the payment and transaction fees; authorize with max_registration_deposit "
            "or on_registration_sponsorship"
        )


def validate_registration_deposit_limit(maximum: Balance | None) -> None:
    """Reject ambiguous units and invalid caps, including on no-registration paths."""
    if maximum is None:
        return
    if not isinstance(maximum, Balance):
        raise TypeError("max_registration_deposit must be a native TAO Balance")
    if maximum.netuid != 0 or maximum.rao < 0:
        raise ValueError("max_registration_deposit must be a nonnegative native TAO Balance")


def registration_sponsorship_quote(
    extras: dict, payer_account: str
) -> RegistrationSponsorship | None:
    """Read automatic sponsorship from the actual composed call's metadata."""
    reserve = extras.get(
        "hashed_registration_max_deposit_rao",
        extras.get("hashed_registration_deposit_rao", 0),
    )
    if not isinstance(reserve, int) or isinstance(reserve, bool) or reserve < 0:
        raise ValueError("invalid automatic registration deposit quote")
    if reserve == 0:
        return None
    recipient = extras.get("hashed_registration")
    if not isinstance(recipient, str) or not recipient:
        raise ValueError("automatic registration deposit is missing its recipient")
    return RegistrationSponsorship(recipient, payer_account, Balance.from_rao(reserve))


async def authorize_registration_sponsorship(
    extras: dict,
    payer_account: str,
    *,
    max_registration_deposit: Balance | None = None,
    on_registration_sponsorship: RegistrationSponsorshipCallback | None = None,
) -> None:
    """Authorize the actual composed call's reserve before submission signing.

    The SDK never prompts. A caller can provide a bounded deposit allowance or a
    callback returning exactly ``True`` for this quote. When both are supplied,
    both must approve; callbacks cannot override an insufficient allowance.
    """
    validate_registration_deposit_limit(max_registration_deposit)
    quote = registration_sponsorship_quote(extras, payer_account)
    if quote is None:
        return
    if max_registration_deposit is not None and quote.deposit.rao > max_registration_deposit.rao:
        raise RegistrationSponsorshipRequired(
            quote, "registration deposit exceeds the allowed maximum"
        )
    if on_registration_sponsorship is not None:
        accepted = on_registration_sponsorship(quote)
        if inspect.isawaitable(accepted):
            accepted = await accepted
        if accepted is True:
            return
        raise RegistrationSponsorshipRequired(quote, "registration sponsorship was declined")
    if max_registration_deposit is None:
        raise RegistrationSponsorshipRequired(quote, "registration sponsorship requires consent")
