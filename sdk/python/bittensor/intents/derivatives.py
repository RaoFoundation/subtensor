"""Derivatives: long and short positions on a subnet's alpha.

A position borrows a slice of the subnet's own liquidity pool. A short borrows
alpha and sells it for TAO; a long borrows TAO and buys alpha. Both are backed
by a TAO cushion the user deposits. There is one position per coldkey and
subnet, and one call that moves it: ``add``. Adding on the position's side
puts more in; adding on the other side takes that much off, paying that share
out at the current price, and flips through zero if there is more. ``close``
settles everything. At settlement the pool gets its slice plus the interest back;
the owner gets what is left of the cushion and the trade's profit or loss.

Three root-set numbers are the design: the pool lends out at most
``pool_share`` of itself per side, at a flat yearly rate on TAO exposure that
is ``short_interest_rate`` for shorts and ``long_interest_rate`` for longs,
fixed per tranche when it is added and accrued per block. Once a week,
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

    `min_amount_out` is your floor on the TAO the call pays you, after
    interest. It binds when the add reduces or closes a position: the
    settlement runs at the live pool price, which anyone can move in the same
    block ahead of it, and a payout below the floor fails the whole call with
    `SettlementBelowMinimum` instead of filling worse. An add that only opens
    or grows pays nothing out and must leave it at `0`, which is also no
    floor. `btcli deriv short` / `long` derive it from a quote and
    `--max-slippage`.

    Refused with `DerivativesDisabled` while the network-wide switch is off
    (`enabled` in `btcli deriv params`); `ClosePosition` still works then.
    Refused with `LongsDisabled` while the long side is off (`longs_enabled`,
    off at launch) if the result would be a long: `Long` opens, grows, or
    flips into one. A `Long` add that only reduces or closes a short goes
    through, and so does every `Short` add.
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
    min_amount_out: Money = field(
        default=0,
        metadata={
            "help": (
                "Least TAO this call must pay you, after interest, when it reduces or closes a "
                "position; below it the call fails with `SettlementBelowMinimum` and nothing "
                "moves. Must be 0 for an add that only opens or grows. 0 (default) is no floor."
            )
        },
    )

    def __post_init__(self):
        self.side = check_side(self.side)
        self.amount = tao_amount(self.amount)
        self.leverage = float(self.leverage)
        leverage_percent(self.leverage)
        self.min_amount_out = tao_amount(self.min_amount_out)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Derivatives.add(
                netuid=self.netuid,
                side=self.side,
                deposit=self.amount.rao,
                leverage_percent=leverage_percent(self.leverage),
                min_amount_out=self.min_amount_out.rao,
            )
        )

    def summary(self) -> str:
        floor = f", at least {self.min_amount_out} back" if self.min_amount_out.rao > 0 else ""
        return (
            f"add {self.side.lower()} {self.amount} at {self.leverage:g}x on netuid "
            f"{self.netuid}{floor}"
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        notes: list[str] = []
        if self.min_amount_out.rao == 0:
            notes.append(
                "no floor on the payout: if this reduces or closes a position, a price pushed "
                "against it in the same block can take from what you get back; set "
                "`min_amount_out` (btcli: `--max-slippage`) to bound that"
            )
        return [
            *notes,
            "against an open position of the other side this reduces or flips it at the "
            "current price: that share's loss or profit is realized now",
            "interest accrues per block on exposure for as long as the position is open, at the "
            "side's rate (`short_interest_rate` or `long_interest_rate` in `btcli deriv params`); "
            "once a week the chain takes it out of the cushion and buys and recycles alpha with it",
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
    back. Works whether or not the network-wide switch is on: the switch
    gates adds, never exits.

    `min_amount_out` is your floor on the payout, after interest. The close
    runs at the live pool price, which anyone can move in the same block
    ahead of it; a payout below the floor fails the call with
    `SettlementBelowMinimum` and leaves the position as it was. Since an
    underwater close pays nothing, any floor above `0` also stops a price
    pushed against you from forfeiting the position. `0` (the default) is
    no floor. `btcli deriv close` derives it from a quote and
    `--max-slippage`.
    """

    op = "close_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "close"),)

    netuid: int = field(metadata={"help": "Subnet the position is on."})
    min_amount_out: Money = field(
        default=0,
        metadata={
            "help": (
                "Least TAO the close must pay you, after interest; below it the call fails with "
                "`SettlementBelowMinimum` and the position stays open. 0 (default) is no floor."
            )
        },
    )

    def __post_init__(self):
        self.min_amount_out = tao_amount(self.min_amount_out)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Derivatives.close(
                netuid=self.netuid,
                min_amount_out=self.min_amount_out.rao,
            )
        )

    def summary(self) -> str:
        floor = f" for at least {self.min_amount_out}" if self.min_amount_out.rao > 0 else ""
        return f"close derivatives position on netuid {self.netuid}{floor}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.min_amount_out.rao > 0:
            return []
        return [
            "no floor on the payout: a price pushed against the close in the same block can "
            "take from what you get back, and an underwater close forfeits everything to the "
            "pool; set `min_amount_out` (btcli: `--max-slippage`) to bound that"
        ]
