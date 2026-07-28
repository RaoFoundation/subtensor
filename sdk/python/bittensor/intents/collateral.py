"""Miner collateral intents: voluntary top-ups and the self-maintaining floor."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from .._generated.calls import Call
from ._money import UNBOUNDED, Money, Spend, alpha_amount
from .base import Intent
from .registry import register
from .staking import (
    DEFAULT_RATE_TOLERANCE,
    RATE_TOLERANCE_HELP,
    _alpha_price_rao,
    _check_rate_tolerance,
)


@register
@dataclass
class AddCollateral(Intent):
    """Lock additional miner collateral on your own hotkey.

    Tops up the hotkey's registration collateral on the subnet — for example
    to meet a validator-published per-machine requirement on resource
    subnets. Prefers free alpha already staked on that hotkey and only buys
    the shortfall with TAO from the coldkey. The signing coldkey must own
    the hotkey. The locked alpha is real stake (it appreciates with the
    subnet pool) but is not withdrawable: it is released back to free stake
    through earned emission at the drain ratio snapshot the hotkey already
    carries, survives deregistration, and is credited against the collateral
    requirement on re-registration. There is no direct withdrawal path —
    see ``set_min_collateral`` for maintaining a level without re-locking
    drained funds.

    Any TAO→alpha buy is fill-or-kill against ``rate_tolerance`` above spot
    and must be submitted MEV-shielded — collateral purchases are not allowed
    to clear unshielded at an unbounded AMM price.
    """

    op = "add_collateral"
    signer = "coldkey"
    wraps = (("SubtensorModule", "add_collateral"),)
    mev_shield_default = True
    mev_shield_required = True

    netuid: int = field(metadata={"help": "Subnet to lock collateral on."})
    amount_alpha: Money = field(
        metadata={
            "help": (
                "Alpha of collateral to add. Uses free stake on the hotkey "
                "first; only the shortfall is bought with TAO (that remainder "
                "must meet the staking minimum)."
            )
        },
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Miner hotkey the collateral attaches to. Defaults to the wallet hotkey."
        },
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid)
        _check_rate_tolerance(self.rate_tolerance)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        price = await _alpha_price_rao(substrate, self.netuid)
        return await substrate.compose(
            calls.SubtensorModule.add_collateral(
                netuid=self.netuid,
                hotkey=hotkey,
                alpha=self.amount_alpha.rao,
                limit_price=int(price * (1 + self.rate_tolerance)),
            )
        )

    def summary(self) -> str:
        return (
            f"lock {self.amount_alpha} as collateral on netuid {self.netuid}"
            f" (fails if price moves >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        # May buy a TAO shortfall when free alpha on the hotkey is insufficient;
        # exact spend needs a stake read, so a spend cap blocks until raised.
        return UNBOUNDED


@register
@dataclass
class SetMinCollateral(Intent):
    """Set the self-maintaining collateral floor for your hotkey on a subnet.

    The collateral drain never releases the lock below the floor, and while
    the lock is under it, earned emission is captured into the lock until
    the floor is met — so a miner tracking a validator-published collateral
    requirement does not need to keep re-locking drained funds. Raising the
    floor above the current lock does not require fresh capital: the shortfall
    fills from future emission (use ``add_collateral`` to fund it
    immediately). Zero clears the floor and restores pure drain behavior.
    """

    op = "set_min_collateral"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_min_collateral"),)

    netuid: int = field(metadata={"help": "Subnet the floor applies to."})
    min_alpha: Money = field(metadata={"help": "The floor, in the subnet's alpha. Zero clears it."})
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Miner hotkey the floor applies to. Defaults to the wallet hotkey."},
    )

    def __post_init__(self):
        self.min_alpha = alpha_amount(self.min_alpha, self.netuid)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            Call(
                "SubtensorModule",
                "set_min_collateral",
                {"netuid": self.netuid, "hotkey": hotkey, "min_locked": self.min_alpha.rao},
            )
        )

    def summary(self) -> str:
        return f"set collateral floor to {self.min_alpha} on netuid {self.netuid}"
