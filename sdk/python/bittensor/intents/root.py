"""Root-origin chain administration intents.

These intents build the inner call the chain only accepts from its root
origin. ``Executor`` nests that call in ``Sudo.sudo`` from ``origin = "root"``,
so the signer must be the chain sudo key. When the sudo key is a multisig,
that ``Sudo.sudo`` call is what the multisig signatories approve and dispatch
(via the multisig intents or ``btcli call``'s ``--multisig`` flags). Every
intent here declares a ``verify`` read that confirms its effect after inclusion.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .._generated import calls
from ._money import Money, tao_amount
from .base import Intent
from .registry import register


@register
@dataclass
class SetSubnetEmissionEnabled(Intent):
    """Enable or disable a subnet's pool-side TAO emission (root only).

    Flips the ``SubnetEmissionEnabled`` storage flag for each listed subnet —
    the ROOT-ONLY switch that gates whether a subnet earns its share of TAO
    emission on the pool side. Distinct from the owner's one-shot
    ``start_call`` (``subnet_is_active``): a subnet can be active — epochs
    running, alpha trading — yet earn no TAO emission share until root flips
    this flag on. Disabling only zeros the pool-side ``alpha_in`` /
    ``tao_in`` / ``excess_tao`` chain-buy paths; it does not remove the
    subnet from emission share calculation and does not touch ``alpha_out``,
    the owner cut, root proportion, or pending server/validator emission.

    Requires the chain sudo key: ``Executor`` wraps the built call in
    ``Sudo.sudo`` because ``origin`` is ``root``, and when root is a multisig
    that ``Sudo.sudo`` call is what the multisig must dispatch. Multiple
    netuids batch atomically via ``Utility.batch_all`` inside the single sudo
    call. Verify the effect afterwards with the ``subnet_emission_enabled``
    read.
    """

    op = "set_subnet_emission_enabled"
    signer = "coldkey"
    origin = "root"
    verify = "subnet_emission_enabled"
    wraps = (
        ("AdminUtils", "sudo_set_subnet_emission_enabled"),
        ("Sudo", "sudo"),
    )

    netuids: list[int] = field(
        metadata={"help": "Subnets to toggle; multiple netuids batch atomically."}
    )
    enabled: bool = field(
        metadata={
            "help": "True to let the subnets earn their pool-side TAO emission share, "
            "false to zero the pool-side alpha_in/tao_in/excess_tao chain-buy paths."
        }
    )

    def __post_init__(self):
        if not self.netuids:
            raise ValueError("set_subnet_emission_enabled requires at least one netuid")
        self.netuids = [int(n) for n in self.netuids]

    async def build(self, substrate, wallet: Any):
        # Inner call only — Executor wraps ``Sudo.sudo`` from ``origin = "root"``
        # so privilege metadata and dispatch stay in lockstep.
        inner = [
            calls.AdminUtils.sudo_set_subnet_emission_enabled(netuid=n, enabled=self.enabled)
            for n in self.netuids
        ]
        if len(inner) == 1:
            return await substrate.compose(inner[0])
        composed = [await substrate.compose(c) for c in inner]
        return await substrate.compose(calls.Utility.batch_all(calls=composed))

    def summary(self) -> str:
        action = "enable" if self.enabled else "disable"
        return f"{action} subnet emission on netuids {self.netuids}"

    def touches_netuids(self) -> list[int]:
        return list(self.netuids)


@register
@dataclass
class SetRootClaimThreshold(Intent):
    """Set the minimum TAO payout for a root dividend claim (root only).

    ``claim_root`` and ``claim_root_with_hotkey`` silently skip any
    per-validator basket redemption whose estimated payout falls below this
    threshold — the shares keep accruing and pay out once they clear it. The
    threshold exists so dust claims cannot grind the chain with tiny swaps;
    the default is 500,000 rao (0.0005 TAO) and the chain caps it at
    10,000,000 rao (0.01 TAO).

    Requires the chain sudo key: ``Executor`` wraps the built call in
    ``Sudo.sudo`` because ``origin`` is ``root``, and when root is a multisig
    that ``Sudo.sudo`` call is what the multisig must dispatch. Verify the
    effect afterwards with the ``root_claim_threshold`` read.
    """

    op = "set_root_claim_threshold"
    signer = "coldkey"
    origin = "root"
    verify = "root_claim_threshold"
    wraps = (
        ("SubtensorModule", "sudo_set_root_claim_threshold"),
        ("Sudo", "sudo"),
    )

    threshold: Money = field(
        metadata={
            "help": "Minimum claim payout in TAO; per-validator redemptions estimated "
            "below it are skipped and keep accruing. At most 0.01 TAO."
        }
    )

    def __post_init__(self):
        self.threshold = tao_amount(self.threshold)

    async def build(self, substrate, wallet: Any):
        # The chain only accepts the root netuid entry for this threshold.
        return await substrate.compose(
            calls.SubtensorModule.sudo_set_root_claim_threshold(
                netuid=0, new_value=self.threshold.rao
            )
        )

    def summary(self) -> str:
        return f"set the root claim threshold to {self.threshold}"
