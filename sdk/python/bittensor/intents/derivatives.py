"""Derivatives: long and short positions on a subnet's alpha.

A position borrows a slice of the subnet's own liquidity pool. A short borrows
alpha and sells it for TAO; a long borrows TAO and buys alpha. Both are backed
by a TAO cushion the user deposits. There is one position per coldkey and
subnet, and one call that moves it: ``add``. Adding on the position's side
puts more in; adding on the other side takes that much off, paying that share
out at the current price, and flips through zero if there is more. ``close``
settles everything. At settlement the pool gets its slice plus the interest back;
the owner gets what is left of the cushion and the trade's profit or loss.

Two root-set numbers are the design: the pool lends out at most ``pool_share``
of itself per side, at ``interest_rate`` on TAO exposure, the same for both
sides, fixed per tranche when it is added and accrued per block. Once a week,
on its own block, each position's interest is collected out of its cushion and
spent buying alpha from the pool, which is then recycled: the interest reaches
the pool as buy pressure, on either side. There is no term: a position lives
until its owner closes it, or until its cushion can no longer pay and the chain
forfeits it to the pool. Nobody else can touch it.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any

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
    alpha on a long), and the result is folded into the position. With a
    position on the other side, that much exposure is settled at the current
    price and its share of the cushion, less interest and loss, is paid back;
    nothing is deposited. Asking for more than the position holds closes it
    and opens the rest on the new side, taking only that rest's cushion.
    """

    op = "add_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "add"),)

    netuid: int = field(metadata={"help": "Subnet whose alpha the position is on."})
    side: str = field(metadata={"help": "Direction to add: `Short` or `Long`."})
    amount: Money = field(
        metadata={
            "help": (
                "TAO the tranche is sized by. Exposure is `leverage` times it, measured against "
                "the pool's TAO reserve. Deposited as cushion when it adds to your position; "
                "only sizes the reduction when it goes against it."
            )
        }
    )
    leverage: float = field(
        default=1.0,
        metadata={
            "help": (
                "Exposure as a multiple of `amount`: 1, 1.2, 1.5. At most the side's maximum: 1x "
                "for shorts, 1.5x for longs (`max_short_leverage` / `max_long_leverage` in "
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
                deposit=self.amount.rao,
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
            "interest accrues per block at `interest_rate` on exposure for as long as the position "
            "is open; once a week the chain takes it out of the cushion and buys and recycles "
            "alpha with it",
            "once the cushion can no longer pay the interest, the chain forfeits the position to "
            "the pool; watch `runway_days` and add cushion or close before that",
        ]

    def spend(self) -> Spend:
        # An upper bound: a reduce deposits nothing, a flip only the surplus.
        if isinstance(self.amount, Balance):
            return self.amount
        return None


@register
@dataclass
class ClosePosition(Intent):
    """Close your derivatives position on a subnet and settle it against the pool.

    Only the owner can close. Settlement reverses the opening trade, repays
    the pool plus the interest owed, and pays you what remains. If the
    position is underwater the pool absorbs the shortfall and you get nothing
    back.
    """

    op = "close_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "close"),)

    netuid: int = field(metadata={"help": "Subnet the position is on."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Derivatives.close(netuid=self.netuid))

    def summary(self) -> str:
        return f"close derivatives position on netuid {self.netuid}"
