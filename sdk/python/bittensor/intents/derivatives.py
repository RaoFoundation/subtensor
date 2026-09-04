"""Derivatives: expiry-bounded long and short positions on a subnet's alpha.

A position borrows a slice of the subnet's own liquidity pool. A short borrows
alpha and sells it for TAO; a long borrows TAO and buys alpha. Both are backed
by a TAO cushion the user deposits. The position stays open until the owner
closes it, or until it expires (then anyone may close it and the ``on_idle``
sweep will). At close the pool gets its slice plus a per-day borrow fee back;
the owner gets what is left of the cushion and the trade's profit or loss, in
TAO.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, ClassVar, Optional

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


@dataclass
class _OpenPosition(Intent):
    """Shared body of ``open_short`` / ``open_long``; the subclass fixes ``side``."""

    signer = "coldkey"
    wraps = (("Derivatives", "open"),)
    side: ClassVar[str]

    netuid: int = field(metadata={"help": "Subnet whose alpha the position is on."})
    amount: Money = field(
        metadata={
            "help": (
                "TAO cushion to deposit from the coldkey balance. Exposure is the side's "
                "leverage times this amount, measured against the pool's TAO reserve."
            )
        }
    )

    def __post_init__(self):
        self.amount = tao_amount(self.amount)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Derivatives.open(netuid=self.netuid, side=self.side, cushion=self.amount.rao)
        )

    def summary(self) -> str:
        return f"open {self.side.lower()} on netuid {self.netuid} with a {self.amount} cushion"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "the position expires after the pallet's lifetime; after that anyone may close it",
            "a per-day borrow fee, fixed at open, is charged at close with a one-day minimum",
        ]

    def spend(self) -> Spend:
        return self.amount if isinstance(self.amount, Balance) else None


@register
@dataclass
class OpenShort(_OpenPosition):
    """Open a short on a subnet's alpha, backed by a TAO cushion.

    The pool lends the position a slice of alpha, which is sold for TAO at
    once. Closing buys the alpha back: if the price fell the buyback is cheaper
    and the difference is profit; if it rose the cushion covers the loss. The
    borrowed slice is sized from the cushion (`short_leverage_percent` of it
    against the pool's TAO reserve) and capped by the pool-share limit.
    """

    op = "open_short"
    side = "Short"


@register
@dataclass
class OpenLong(_OpenPosition):
    """Open a long on a subnet's alpha, backed by a TAO cushion.

    The pool lends the position a slice of TAO, which buys alpha at once.
    Closing sells the alpha back: if the price rose the sale covers the loan
    with profit left over; if it fell the cushion covers the loss. The borrowed
    slice is sized from the cushion (`long_leverage_percent` of it against the
    pool's TAO reserve) and capped by the pool-share limit.
    """

    op = "open_long"
    side = "Long"


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
    side: str = field(metadata={"help": "Which position to close: `Short` or `Long`."})
    owner_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": (
                "Coldkey that owns the position. Defaults to the signer; pass another "
                "owner only to close their expired position."
            )
        },
    )

    def __post_init__(self):
        self.side = check_side(self.side)

    async def build(self, substrate, wallet: Any):
        owner = self.owner_ss58 or self.coldkey_address(wallet)
        return await substrate.compose(
            calls.Derivatives.close(owner=owner, netuid=self.netuid, side=self.side)
        )

    def summary(self) -> str:
        whose = f" owned by {self.owner_ss58}" if self.owner_ss58 else ""
        return f"close {self.side.lower()} on netuid {self.netuid}{whose}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.owner_ss58 and self.owner_ss58 != signer_address:
            return ["closing another owner's position only succeeds once it has expired"]
        return []


@register
@dataclass
class RollPosition(Intent):
    """Settle a position at today's price and reopen it in the same transaction.

    The old position is closed like ``close``: the pool gets its slice and the
    borrow fee back, and the owner's cushion plus profit or loss comes back in
    TAO. That TAO, plus an optional ``top_up``, is then the cushion of a fresh
    position on the same side with a full lifetime and today's entry price.
    Fails without touching the position if the new cushion is below the
    minimum deposit or the pool cap is reached; ``close`` instead.
    """

    op = "roll_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "roll"),)

    netuid: int = field(metadata={"help": "Subnet the position is on."})
    side: str = field(metadata={"help": "Which position to roll: `Short` or `Long`."})
    top_up: Optional[Money] = field(
        default=None,
        metadata={"help": "Extra TAO to add to the new cushion from the coldkey balance."},
    )

    def __post_init__(self):
        self.side = check_side(self.side)
        self.top_up = tao_amount(self.top_up) if self.top_up is not None else None

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Derivatives.roll(
                netuid=self.netuid,
                side=self.side,
                top_up=self.top_up.rao if self.top_up is not None else 0,
            )
        )

    def summary(self) -> str:
        extra = f" adding {self.top_up}" if self.top_up is not None else ""
        return f"roll {self.side.lower()} on netuid {self.netuid}{extra}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "the old position is settled at the current price: its loss or profit is realized now",
            "the fee accrued so far is paid at the roll; the new position starts a fresh fee clock",
        ]

    def spend(self) -> Spend:
        return self.top_up if isinstance(self.top_up, Balance) else None
