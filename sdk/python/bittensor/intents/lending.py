"""Fixed-principal loans backed by a subnet's separate lending reserves."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional, cast

from .._generated.calls import Call
from .._generated.storage import Item
from ..balance import Balance
from ..result import BittensorError
from ._money import Money, Spend, alpha_amount, tao_amount
from .base import Intent
from .registry import register

POSITIONS = Item("Lending", "Positions", "Position")


def check_side(side: str) -> str:
    """Accept the two runtime variants, including lower-case CLI spelling."""
    result = str(side).capitalize()
    if result not in ("Short", "Long"):
        raise BittensorError("side must be 'short' or 'long'")
    return result


def _check_netuid(netuid: int) -> None:
    if not 1 <= netuid <= 65535:
        raise BittensorError("lending requires a subnet netuid from 1 to 65535")


def _contextual_amount(value: Money) -> Money:
    """Keep exact decimals until the owned position identifies the currency.

    An explicitly tagged Balance keeps its tag for validation at build time.
    Plain amounts become decimal strings rather than floats. A close's units
    depend on stored position state, not an extra caller-supplied side field.
    """
    normalized = cast(Balance, tao_amount(value)) if not isinstance(value, Balance) else value
    if normalized.rao < 0:
        raise BittensorError("amount must be non-negative")
    return normalized if isinstance(value, Balance) else format(normalized.decimal, "f")


@register
@dataclass
class OpenLoan(Intent):
    """Open or increase a fixed-principal subnet loan at up to 25% LTV.

    A short locks TAO collateral, borrows alpha and immediately sells it;
    the TAO proceeds stay in contract custody. A long locks existing free
    alpha stake and receives freely transferable TAO. The debt stays in the
    borrowed currency. Interest is 100% annually on opening loan value,
    charged in fixed collateral units and collected weekly. There are no
    price-triggered liquidations; exhausted collateral forfeits the position.

    An existing position must have the same side and hotkey. ``collateral``
    adds to its remaining collateral after accrued interest; borrowed principal,
    short sale proceeds and the new fixed coupon add to the saved position.
    The old coupon is not repriced. The combined position must pass current
    opening guards, and an already exhausted position cannot be increased.

    ``min_borrow`` is a floor on additional principal, including opening swap
    fees. ``min_proceeds`` separately bounds TAO from the additional short sale.
    The call is atomic and refuses a smaller loan or sale. There remains one
    position per coldkey and subnet; closing repays its total principal.
    """

    op = "open_loan"
    signer = "coldkey"
    wraps = (("Lending", "open"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet to borrow against."})
    side: str = field(metadata={"help": "short: lock TAO; long: lock subnet alpha."})
    collateral: Money = field(
        metadata={"help": "Additional collateral to lock: TAO for a short, alpha for a long."}
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Hotkey holding long collateral or wallet-supplied short repayment alpha."
        },
    )
    min_borrow: Money = field(
        default=0,
        metadata={"help": "Minimum additional principal: alpha for a short, TAO for a long."},
    )
    min_proceeds: Money = field(
        default=0,
        metadata={"help": "Minimum TAO from the additional short sale; unused for longs."},
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.side = check_side(self.side)
        collateral_unit = (
            tao_amount if self.side == "Short" else lambda x: alpha_amount(x, self.netuid)
        )
        debt_unit = tao_amount if self.side == "Long" else lambda x: alpha_amount(x, self.netuid)
        normalized = cast(Balance, collateral_unit(self.collateral))
        self.collateral = normalized
        self.min_borrow = debt_unit(self.min_borrow)
        self.min_proceeds = tao_amount(self.min_proceeds)
        if normalized.rao == 0:
            raise BittensorError("collateral must be greater than zero")

    async def build(self, substrate, wallet: Any):
        collateral = cast(Balance, self.collateral)
        minimum = cast(Balance, self.min_borrow)
        return await substrate.compose(
            Call(
                "Lending",
                "open",
                {
                    "netuid": self.netuid,
                    "side": self.side,
                    "collateral": collateral.rao,
                    "hotkey": self.hotkey_address(wallet, self.hotkey_ss58),
                    "min_borrow": minimum.rao,
                    "min_proceeds": cast(Balance, self.min_proceeds).rao,
                },
            )
        )

    def summary(self) -> str:
        return (
            f"open or increase {self.side.lower()} on netuid {self.netuid}, add {self.collateral}"
        )

    def spend(self) -> Spend:
        return cast(Balance, self.collateral) if self.side == "Short" else None


@register
@dataclass
class CloseLoan(Intent):
    """Repay total principal and return remaining collateral on a subnet.

    A short normally buys its fixed alpha debt through the AMM using locked
    TAO. ``repay_from_wallet`` instead takes that alpha from the saved hotkey.
    A long always repays TAO from the owner's free balance. Both charge
    accrued interest through the closing block and then refund the remainder.
    A failed repayment, payment ceiling or refund floor rolls back the call.

    ``max_payment`` is TAO for AMM short closes and long repayment, or alpha
    for a wallet-repaid short. ``min_refund`` is TAO for shorts and alpha for
    longs. Units are checked against the owner's actual stored position.
    Closing remains available when new borrowing is disabled.
    """

    op = "close_loan"
    signer = "coldkey"
    wraps = (("Lending", "close"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet of the position to repay."})
    max_payment: Money = field(
        metadata={"help": "Maximum repayment: TAO, or alpha for a wallet-repaid short."}
    )
    min_refund: Money = field(
        default=0,
        metadata={"help": "Minimum returned collateral: TAO for shorts, alpha for longs."},
    )
    repay_from_wallet: bool = field(
        default=False,
        metadata={"help": "Repay a short with alpha from its saved hotkey instead of an AMM buy."},
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.max_payment = _contextual_amount(self.max_payment)
        self.min_refund = _contextual_amount(self.min_refund)

    async def build(self, substrate, wallet: Any):
        owner = self.coldkey_address(wallet)
        position = await substrate.query(*POSITIONS, [owner, self.netuid])
        if not isinstance(position, dict):
            raise BittensorError(f"no lending position on netuid {self.netuid}")
        side = position["side"]
        if isinstance(side, dict):
            side = next(iter(side))
        side = check_side(str(side))
        payment = cast(
            Balance,
            (
                alpha_amount(self.max_payment, self.netuid)
                if side == "Short" and self.repay_from_wallet
                else tao_amount(self.max_payment)
            ),
        )
        refund = cast(
            Balance,
            (
                tao_amount(self.min_refund)
                if side == "Short"
                else alpha_amount(self.min_refund, self.netuid)
            ),
        )
        return await substrate.compose(
            Call(
                "Lending",
                "close",
                {
                    "netuid": self.netuid,
                    "repay_from_wallet": self.repay_from_wallet,
                    "max_payment": payment.rao,
                    "min_refund": refund.rao,
                },
            )
        )

    def summary(self) -> str:
        return f"repay and close lending position on netuid {self.netuid}"

    def spend(self) -> Spend:
        # The ceiling also bounds any wallet TAO repayment. An AMM short close
        # uses escrow, so this can overstate free-TAO spend but cannot understate it.
        # Explicit alpha amounts are validated against stored side before dispatch.
        if isinstance(self.max_payment, Balance) and self.max_payment.netuid != 0:
            return None
        return cast(Balance, tao_amount(self.max_payment))
