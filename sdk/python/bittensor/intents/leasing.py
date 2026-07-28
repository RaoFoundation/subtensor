"""Subnet leasing: register a crowdloan-funded leased subnet, and terminate it.

A leased subnet is created with crowdloan funds; contributors receive a share of
its emissions as dividends, and the beneficiary operates it through a proxy. If
the lease has an end block, the beneficiary can take full ownership after it
passes by terminating the lease. Inspect leases with the ``lease`` / ``leases``
reads.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ._money import UNBOUNDED, Spend
from .base import Intent
from .registry import register


@register
@dataclass
class RegisterLeasedNetwork(Intent):
    """Register a new crowdloan-funded leased subnet.

    Creates a subnet paid for by a crowdloan rather than a single coldkey: the
    crowdloan's funds cover the network registration cost (leftover cap is
    refunded to contributors pro-rata, with any rounding remainder going to
    the beneficiary), contributors earn ``emissions_share`` percent of the
    subnet owner's emission cut (not of total subnet emissions) as dividends,
    and the beneficiary operates the subnet through a scoped proxy. Must be
    dispatched in a crowdloan context — it fails as a standalone call. The
    cost is not cheaply boundable up front, so a configured spend cap blocks
    this until raised. With an ``end_block`` the beneficiary can later take
    full ownership via ``terminate_lease``; without one the lease is
    perpetual and ownership never transfers.
    """

    op = "register_leased_network"
    signer = "coldkey"
    wraps = (("SubtensorModule", "register_leased_network"),)

    emissions_share: int = field(
        metadata={
            "help": "Percent (0-100) of the subnet owner's emission cut paid to "
            "crowdloan contributors as dividends."
        }
    )
    end_block: Optional[int] = field(
        default=None,
        metadata={
            "help": "Block at which the lease ends and the beneficiary may take "
            "ownership; omit for a perpetual lease."
        },
    )

    def __post_init__(self):
        if not 0 <= self.emissions_share <= 100:
            raise ValueError(f"emissions_share must be a percent 0-100, got {self.emissions_share}")

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.register_leased_network(
                emissions_share=self.emissions_share, end_block=self.end_block
            )
        )

    def summary(self) -> str:
        horizon = f"until block {self.end_block}" if self.end_block is not None else "perpetual"
        return f"register leased subnet ({self.emissions_share}% to contributors, {horizon})"

    def spend(self) -> Spend:
        # Lock cost is drawn from crowdloan funds with a leftover charged to the
        # beneficiary; not cheaply boundable, so a spend cap must block it.
        return UNBOUNDED


@register
@dataclass
class TerminateLease(Intent):
    """Terminate an ended lease and take subnet ownership (beneficiary only).

    Ends the lease and transfers full subnet ownership to the beneficiary:
    contributor dividends stop and the subnet becomes an ordinary owned
    subnet. Only the lease's beneficiary can call it, and only after the
    lease's end block has passed — earlier attempts fail, and perpetual leases
    (no end block) can never be terminated this way. Check the lease's end
    block with the ``lease`` read before calling.
    """

    op = "terminate_lease"
    signer = "coldkey"
    wraps = (("SubtensorModule", "terminate_lease"),)

    lease_id: int = field(metadata={"help": "Lease to terminate (see the leases read)."})
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Beneficiary hotkey recorded as the subnet's owner hotkey; defaults to "
            "the wallet's hotkey."
        },
    )

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.terminate_lease(lease_id=self.lease_id, hotkey=hotkey)
        )

    def summary(self) -> str:
        return f"terminate lease {self.lease_id} and take subnet ownership"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["only succeeds after the lease's end block has passed"]
