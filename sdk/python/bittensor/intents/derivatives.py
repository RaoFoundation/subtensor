"""Derivatives: expiry-bounded long and short positions on a subnet's alpha.

A position borrows a slice of the subnet's own liquidity pool. A short borrows
alpha and sells it for TAO; a long borrows TAO and buys alpha. Both are backed
by a TAO cushion the user deposits. There is one position per coldkey and
subnet, and one call that moves it: ``add``. Adding on the position's side
puts more in; adding on the other side takes that much off, paying that share
out at the current price, and flips through zero if there is more. ``close``
settles everything. At settlement the pool gets its slice plus the per-day
borrow fee back; the owner gets what is left of the cushion and the trade's
profit or loss, in TAO.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional

from .._generated import calls
from ..balance import Balance
from ._money import Money, Spend, tao_amount
from .base import Intent
from .registry import register

# Variants of the runtime's `Side` enum (pallets/derivatives/src/position.rs).
SIDES = ("Short", "Long")
SideChoice = Enum("SideChoice", [(name, name) for name in SIDES], type=str)


def check_side(side: str) -> str:
    if side not in SIDES:
        raise ValueError(f"unknown side {side!r}; expected one of: {', '.join(SIDES)}")
    return side


def leverage_percent(leverage: float) -> int:
    """`1` -> `100`, `2.5` -> `250`. The runtime takes whole percent points in a `u16`."""
    percent = round(float(leverage) * 100)
    if abs(percent - float(leverage) * 100) > 1e-6:
        raise ValueError(f"leverage {leverage} is finer than 0.01x")
    if not 1 <= percent <= 65_535:
        raise ValueError(f"leverage {leverage} is out of range; use 0.01x to 655.35x")
    return percent


@register
@dataclass
class AddPosition(Intent):
    """Add `leverage` times `amount` of `side` exposure to your position on a subnet.

    One call for open, add, reduce, and flip. With no position, or one on the
    same side, `amount` TAO is taken from the coldkey as cushion, the pool
    lends the matching slice (alpha sold for TAO on a short, TAO spent on
    alpha on a long), and the result is folded into the position; one day of
    the new tranche's fee is booked up front. With a position on the other
    side, that much exposure is settled at the current price and its share of
    the cushion, less fee and loss, is paid back; nothing is deposited. Asking
    for more than the position holds closes it and opens the rest on the new
    side, taking only that rest's cushion. Nothing can be added to a position
    past its expiry; it can still be reduced or closed.
    """

    op = "add_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "add"),)

    netuid: int = field(metadata={"help": "Subnet whose alpha the position is on."})
    side: str = field(metadata={"help": "Direction to add: `Short` or `Long`."})
    amount: Money = field(
        metadata={
            "help": (
                "TAO the tranche is sized by. Exposure is `--leverage` times this, measured "
                "against the pool's TAO reserve. Deposited as cushion when it adds to your "
                "position; only sizes the reduction when it goes against it."
            )
        }
    )
    leverage: float = field(
        default=1.0,
        metadata={
            "help": (
                "Exposure as a multiple of `amount`: 1, 2, 5, ... Must be at most the side's "
                "maximum (`max_short_leverage_percent` / `max_long_leverage_percent` in "
                "`btcli deriv params`)."
            )
        },
    )

    def __post_init__(self):
        self.side = check_side(self.side)
        self.amount = tao_amount(self.amount)
        self.leverage = float(self.leverage)
        leverage_percent(self.leverage)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Derivatives.add(
                netuid=self.netuid,
                side=self.side,
                amount=self.amount.rao,
                leverage_percent=leverage_percent(self.leverage),
            )
        )

    def summary(self) -> str:
        return (
            f"add {self.side.lower()} {self.amount} at {self.leverage:g}x on netuid {self.netuid}"
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "against an open position of the other side this reduces or flips it at the "
            "current price: that share's loss or profit is realized now",
            "the position expires after the pallet's lifetime; after that anyone may close it",
            "each add books one day of its borrow fee up front; the fee then accrues per block",
        ]

    def spend(self) -> Spend:
        # An upper bound: a reduce deposits nothing, a flip only the surplus.
        return self.amount if isinstance(self.amount, Balance) else None


@register
@dataclass
class ClosePosition(Intent):
    """Close a derivatives position and settle it against the pool.

    The owner may close at any time. After the position's expiry anyone may
    close it on the owner's behalf, so the pool always gets its liquidity back.
    Settlement reverses the opening trade, repays the pool plus the borrow
    fee, and pays the owner what remains in TAO. If the position is underwater
    the pool absorbs the shortfall and the owner gets nothing back.
    """

    op = "close_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "close"),)

    netuid: int = field(metadata={"help": "Subnet the position is on."})
    owner_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": (
                "Coldkey that owns the position. Defaults to the signer; pass another "
                "owner only to close their expired position."
            )
        },
    )

    async def build(self, substrate, wallet: Any):
        owner = self.owner_ss58 or self.coldkey_address(wallet)
        return await substrate.compose(calls.Derivatives.close(owner=owner, netuid=self.netuid))

    def summary(self) -> str:
        whose = f" owned by {self.owner_ss58}" if self.owner_ss58 else ""
        return f"close derivatives position on netuid {self.netuid}{whose}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.owner_ss58 and self.owner_ss58 != signer_address:
            return ["closing another owner's position only succeeds once it has expired"]
        return []
