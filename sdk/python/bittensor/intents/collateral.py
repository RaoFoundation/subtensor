"""Miner collateral intents: voluntary top-ups and the self-maintaining floor."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

# TODO(codegen): switch to `calls.SubtensorModule.add_collateral` /
# `set_min_collateral` once the registry is regenerated against spec >= 435.
from .._generated.calls import Call
from ._money import Money, alpha_amount, tao_amount
from .base import Intent
from .registry import register


@register
@dataclass
class AddCollateral(Intent):
    """Stake TAO to your own hotkey and lock it as additional miner collateral.

    Tops up the hotkey's registration collateral on the subnet — for example
    to meet a validator-published per-machine requirement on resource
    subnets. The signing coldkey must own the hotkey, because the lock is
    charged against the owner's stake. The locked alpha is real stake (it
    appreciates with the subnet pool) but is not withdrawable: it is released
    back to free stake through earned miner incentive at the drain ratio
    snapshot the hotkey already carries, survives deregistration, and is
    credited against the collateral requirement on re-registration. There is
    no direct withdrawal path — see ``set_min_collateral`` for maintaining a
    level without re-locking drained funds.
    """

    op = "add_collateral"
    signer = "coldkey"
    wraps = (("SubtensorModule", "add_collateral"),)

    netuid: int = field(metadata={"help": "Subnet to lock collateral on."})
    amount_tao: Money = field(
        metadata={"help": "TAO to stake and lock as collateral (min: the staking minimum)."}
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Miner hotkey the collateral attaches to. Defaults to the wallet hotkey."
        },
    )

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            Call(
                "SubtensorModule",
                "add_collateral",
                {"netuid": self.netuid, "hotkey": hotkey, "tao": self.amount_tao.rao},
            )
        )

    def summary(self) -> str:
        return f"lock {self.amount_tao} as collateral on netuid {self.netuid}"


@register
@dataclass
class SetMinCollateral(Intent):
    """Set the self-maintaining collateral floor for your hotkey on a subnet.

    The collateral drain never releases the lock below the floor, and while
    the lock is under it, earned miner incentive is captured into the lock
    until the floor is met — so a miner tracking a validator-published
    collateral requirement does not need to keep re-locking drained funds.
    Raising the floor above the current lock does not require fresh capital:
    the shortfall fills from future incentive (use ``add_collateral`` to fund
    it immediately). Zero clears the floor and restores pure drain behavior.
    """

    op = "set_min_collateral"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_min_collateral"),)

    netuid: int = field(metadata={"help": "Subnet the floor applies to."})
    min_alpha: Money = field(
        metadata={"help": "The floor, in the subnet's alpha. Zero clears it."}
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Miner hotkey the floor applies to. Defaults to the wallet hotkey."
        },
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
