"""Derivatives: long and short positions on a subnet's alpha.

A position borrows a slice of the subnet's own liquidity pool. A short borrows
alpha and sells it for TAO; a long borrows TAO and buys alpha. Both are backed
by a cushion the user deposits, in TAO or (where root allows it) in the
subnet's alpha. There is one position per coldkey and subnet, and one call
that moves it: ``add``. Adding on the position's side puts more in; adding on
the other side takes that much off, paying that share out at the current
price, and flips through zero if there is more. ``close`` settles everything.
At settlement the pool gets its slice plus the per-day borrow fee back; the
owner gets what is left of the cushion and the trade's profit or loss, in
kind.

The fee is one rate on TAO exposure, the same for both sides, fixed per
tranche when it is added. A position lives for ``lifetime_blocks`` from its
first add (90 days by default) or until its equity no longer covers one day
of fee. After either, anyone may close it and is paid for doing so. An
owner's same-side add on an expired position settles it at today's price and
reopens it: a roll.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional

from .._generated import calls
from ..balance import Balance
from ._money import Money, Spend, alpha_amount, tao_amount
from .base import Intent
from .registry import register

# Variants of the runtime's `Side` enum (pallets/derivatives/src/position.rs).
SIDES = ("Short", "Long")
SideChoice = Enum("SideChoice", [(name, name) for name in SIDES], type=str)

# Variants of the runtime's `Deposit` enum, lower-cased for the CLI.
DEPOSIT_ASSETS = ("tao", "alpha")
DepositAssetChoice = Enum("DepositAssetChoice", [(name, name) for name in DEPOSIT_ASSETS], type=str)

DEPOSIT_IN_HELP = (
    "Asset the cushion is paid in: `tao` from the coldkey balance, or `alpha` already staked "
    "on `hotkey_ss58` at this subnet. Alpha cushions must be switched on by root for the side "
    "(`alpha_cushion_shorts` / `alpha_cushion_longs` in `btcli deriv params`)."
)
HOTKEY_HELP = (
    "Hotkey the alpha cushion is staked on (only with `deposit_in=alpha`). Defaults to the "
    "wallet hotkey. The cushion comes back to the same hotkey."
)


def check_side(side: str) -> str:
    if side not in SIDES:
        raise ValueError(f"unknown side {side!r}; expected one of: {', '.join(SIDES)}")
    return side


def check_deposit_asset(asset: str) -> str:
    asset = str(asset).lower()
    if asset not in DEPOSIT_ASSETS:
        raise ValueError(
            f"unknown deposit asset {asset!r}; expected one of: {', '.join(DEPOSIT_ASSETS)}"
        )
    return asset


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

    One call for open, add, reduce, flip, and roll. With no position, or one
    on the same side, `amount` is taken as cushion (TAO from the coldkey, or
    alpha from stake on `hotkey_ss58`), the pool lends the matching slice
    (alpha sold for TAO on a short, TAO spent on alpha on a long), and the
    result is folded into the position; one day of the new tranche's fee is
    booked up front. The expiry is set by the first add and does not move.
    With a position on the other side, that much exposure is settled at the
    current price and its share of the cushion, less fee and loss, is paid
    back; nothing is deposited. Asking for more than the position holds closes
    it and opens the rest on the new side, taking only that rest's cushion.
    On an expired position, a same-side add settles it first and opens a fresh
    one from `amount` alone.
    """

    op = "add_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "add"),)

    netuid: int = field(metadata={"help": "Subnet whose alpha the position is on."})
    side: str = field(metadata={"help": "Direction to add: `Short` or `Long`."})
    amount: Money = field(
        metadata={
            "help": (
                "Cushion the tranche is sized by, in TAO or in the subnet's alpha depending on "
                "`deposit_in`. Exposure is `--leverage` times its TAO value, measured against "
                "the pool's TAO reserve. Deposited when it adds to your position; only sizes "
                "the reduction when it goes against it."
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
    deposit_in: str = field(default="tao", metadata={"help": DEPOSIT_IN_HELP})
    hotkey_ss58: Optional[str] = field(default=None, metadata={"help": HOTKEY_HELP})

    def __post_init__(self):
        self.side = check_side(self.side)
        self.deposit_in = check_deposit_asset(self.deposit_in)
        if self.deposit_in == "tao":
            self.amount = tao_amount(self.amount)
        else:
            self.amount = alpha_amount(self.amount, self.netuid)
        self.leverage = float(self.leverage)
        leverage_percent(self.leverage)

    def _deposit(self, wallet: Any) -> dict:
        if self.deposit_in == "tao":
            return {"Tao": self.amount.rao}
        return {
            "Alpha": {
                "hotkey": self.hotkey_address(wallet, self.hotkey_ss58),
                "amount": self.amount.rao,
            }
        }

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Derivatives.add(
                netuid=self.netuid,
                side=self.side,
                deposit=self._deposit(wallet),
                leverage_percent=leverage_percent(self.leverage),
            )
        )

    def summary(self) -> str:
        return (
            f"add {self.side.lower()} {self.amount} at {self.leverage:g}x on netuid {self.netuid}"
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out = [
            "against an open position of the other side this reduces or flips it at the "
            "current price: that share's loss or profit is realized now",
            "each add books one day of its borrow fee up front; the fee then accrues per block",
            "the position expires `lifetime_blocks` after its first add (90 days by default); "
            "adding does not extend it, and after it anyone may close it for one day of fee",
            "once the position's equity drops below one day of fee, anyone may close it and "
            "keeps the fee; add cushion or close before that",
        ]
        if self.deposit_in == "alpha":
            out.append("the alpha cushion earns no staking emission while the position is open")
        return out

    def spend(self) -> Spend:
        # An upper bound: a reduce deposits nothing, a flip only the surplus.
        if self.deposit_in == "tao" and isinstance(self.amount, Balance):
            return self.amount
        return None


@register
@dataclass
class ClosePosition(Intent):
    """Close a derivatives position and settle it against the pool.

    The owner may close at any time. Anyone may close a position that is no
    longer healthy (its equity is below one day of fee) or that has expired.
    A liquidator is paid the fee owed plus whatever is left after the pool is
    repaid, topped up by the pool to one day of fee, and the owner gets
    nothing. The closer of an expired position is paid one day of fee and the
    owner gets the rest. Settlement reverses the opening trade, repays the pool
    plus the borrow fee, and pays the owner what remains, in kind. If the
    position is underwater the pool absorbs the shortfall and the owner gets
    nothing back.
    """

    op = "close_derivative"
    signer = "coldkey"
    wraps = (("Derivatives", "close"),)

    netuid: int = field(metadata={"help": "Subnet the position is on."})
    owner_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": (
                "Coldkey that owns the position. Defaults to the signer; pass another owner "
                "to close their position once `healthy` is False or `expired` is True in "
                "`derivative-position`."
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
            return [
                "closing another owner's position only succeeds once it has expired or is "
                "unhealthy at the chain's own quote; the SDK's `healthy` flag is an estimate"
            ]
        return []
