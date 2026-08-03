"""Registration and subnet-lifecycle intents."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ._money import UNBOUNDED, Spend
from .base import Intent
from .registry import register


@register
@dataclass
class BurnedRegister(Intent):
    """Register a hotkey on a subnet by paying the registration cost.

    Pays the subnet's current floating registration cost from the signing
    coldkey and assigns the hotkey a UID. When the subnet's collateral lock
    share (p) is zero, the full cost is burned/recycled. When p > 0, the
    ``(1 - p)`` share is burned and the ``p`` share is staked to the hotkey and
    locked as miner collateral (released only through earned emission). The
    exact TAO charge is only known at execution time, so a configured spend
    cap blocks this call until raised. Fails with
    ``SubNetRegistrationDisabled`` while the subnet's ``registration_allowed``
    toggle is off. On a full subnet, registering evicts the non-immune neuron
    with the lowest emission (ties broken by older registration block, then
    lower UID), and the new UID can itself be evicted once its immunity
    period ends. Use ``root_register`` instead for the root network
    (netuid 0).
    """

    op = "burned_register"
    signer = "coldkey"
    wraps = (("SubtensorModule", "burned_register"),)
    mev_shield_default = True
    # Registration can buy the collateral share through the AMM; keep the
    # mempool entry encrypted so a delayed inclusion cannot be sandwiched
    # into a worse fill against the runtime's 5% collateral limit.
    mev_shield_required = True

    netuid: int = field(metadata={"help": "Subnet to register on."})
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey that receives the UID; defaults to the wallet's hotkey."},
    )

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.burned_register(netuid=self.netuid, hotkey=hotkey)
        )

    def summary(self) -> str:
        target = self.hotkey_ss58 or "wallet hotkey"
        return f"register {target} on netuid {self.netuid} (burned/collateral)"

    def spend(self) -> Spend:
        # Pays the subnet's current registration cost from the coldkey. The exact
        # amount isn't known without a read, so a spend cap must block until raised.
        return UNBOUNDED


@register
@dataclass
class RegisterSubnet(Intent):
    """Create a new subnet owned by the signing coldkey.

    Registers a brand-new subnet with the signing coldkey as its owner and the
    wallet's hotkey as the subnet-owner hotkey. The network registration cost —
    potentially thousands of TAO — is taken from the coldkey; it doubles after
    each new subnet registration and decays linearly back over the lock
    reduction interval, and is only known at execution time, so a configured
    spend cap blocks this call until raised. The full cost becomes the new
    subnet's initial TAO pool reserve — a sunk cost, not a refundable deposit.
    Network registrations are rate-limited per coldkey. If capacity is
    available, the subnet is created in the registration block. If the chain
    is at its subnet limit, registration first queues while the non-immune
    subnet with the lowest EMA price is dissolved across idle block time; SDK
    execution waits for the matching ``NetworkAdded`` event before returning
    by default. The result's ``registration_mode`` distinguishes the two paths
    and ``netuid`` is the subnet actually assigned.
    The new subnet starts inactive: call ``start_call`` once the chain's
    activation delay has passed to activate it; the subnet's share of TAO
    emission additionally stays off until root enables the subnet's
    emission-enabled flag. This is a major, expensive commitment — check the
    current cost before sending.
    """

    op = "register_subnet"
    signer = "coldkey"
    wraps = (("SubtensorModule", "register_network"),)

    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Subnet-owner hotkey for the new subnet; defaults to the wallet's hotkey."
        },
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.register_network(
                hotkey=self.hotkey_address(wallet, self.hotkey_ss58)
            )
        )

    def summary(self) -> str:
        return "register a new subnet"

    def spend(self) -> Spend:
        # Locks/burns the network registration cost (can be thousands of TAO).
        return UNBOUNDED


@register
@dataclass
class StartCall(Intent):
    """Activate a subnet (subtoken trading, epochs) as its owner.

    Flips a freshly registered subnet from inactive to active: the subnet
    token becomes tradable and alpha emission into the subnet's epochs
    begins. It does not enable the subnet's share of TAO emission — that
    additionally requires the root-gated emission-enabled flag, which only
    root can set. Owner-only, callable once per subnet, and only after the
    chain's minimum delay since the subnet was registered — calling too
    early fails. Until this is called the subnet earns nothing, so run it
    as soon as the delay allows.
    """

    op = "start_call"
    signer = "coldkey"
    origin = "subnet_owner"
    wraps = (("SubtensorModule", "start_call"),)

    netuid: int = field(metadata={"help": "Subnet to activate; the signer must be its owner."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.SubtensorModule.start_call(netuid=self.netuid))

    def summary(self) -> str:
        return f"activate subnet {self.netuid}"


@register
@dataclass
class RootRegister(Intent):
    """Register a hotkey on the root network (netuid 0).

    Joins the hotkey to the root network, the TAO staking pool (netuid 0 has
    no miners and no alpha; validators register here to receive root stake).
    Admission is burn-based, like subnet registration: the coldkey pays the
    root burn price (recycled out of issuance, demand-priced — each
    registration bumps it and it decays back toward the floor). No prior
    stake is required, but root slots are limited: joining a full root
    network evicts the member with the least stake, so a seat is only held
    by keeping stake behind the hotkey. Root registrations are also capped
    per block (``max_registrations_per_block``) and per interval (three
    times ``target_registrations_per_interval``); hitting either cap fails
    until the window passes. Use ``burned_register`` for ordinary subnets.
    """

    op = "root_register"
    signer = "coldkey"
    wraps = (("SubtensorModule", "root_register"),)
    # Docs: the friendly path is the ordinary subnet register command, which
    # routes netuid 0 here.
    cli_example = "btcli subnets register --netuid 0"

    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Hotkey to register on the root network; defaults to the wallet's hotkey."
        },
    )

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(calls.SubtensorModule.root_register(hotkey=hotkey))

    def summary(self) -> str:
        return f"register {self.hotkey_ss58 or 'wallet hotkey'} on the root network"

    def touches_netuids(self) -> list[int]:
        return [0]


@register
@dataclass
class ClaimRoot(Intent):
    """Redeem accrued root dividends across every validator for the coldkey.

    Root dividends accrue as shares of each validator's basket — an
    escrowed index fund of subnet alpha the chain builds from the validator's
    root dividends per its root weights (see ``set_root_weights``). This call
    redeems the signing coldkey's owed shares on every validator it
    root-stakes to. The ``subnets`` argument is retained for call-data
    compatibility with pre-basket clients and is ignored — baskets have no
    per-subnet claim selection.

    Prefer :class:`ClaimRootWithHotkey` to claim a single validator.
    """

    op = "claim_root"
    signer = "coldkey"
    wraps = (("SubtensorModule", "claim_root"),)

    subnets: list[int] = field(
        default_factory=lambda: [0],
        metadata={
            "help": "Ignored (kept for old-client call-data compatibility). "
            "Pass any non-empty netuid list; baskets claim fund-level."
        },
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.SubtensorModule.claim_root(subnets=self.subnets))

    def summary(self) -> str:
        return "claim root dividends on all validators (redeem basket shares to root stake)"


@register
@dataclass
class ClaimRootWithHotkey(Intent):
    """Redeem accrued root dividends (basket shares) for one validator.

    Redeems the signing coldkey's owed shares on the given validator only:
    that basket pays out pro-rata (subnet alpha holdings are sold to TAO at
    the current pool price) and the proceeds are staked back to root on the
    same validator. Other validators' accrued yield is left untouched.
    Claims whose estimated payout is below the chain's claim threshold
    (see ``root_claim_threshold``) are silently skipped and keep accruing.
    Orphaned dust holdings in the basket (subnets outside the validator's
    current weight vector, worth less than the same threshold) are
    consolidated into the fund's root (TAO) slot as a side effect, so the
    per-holding claim fee shrinks over time; curated positions are left to
    compound. The transaction fee is charged by work actually done:
    holdings redeemed pay full weight, holdings merely scanned pay a small
    per-row cost.
    Preview per-validator payouts with ``root_basket_owed_breakdown``.
    """

    op = "claim_root_with_hotkey"
    signer = "coldkey"
    wraps = (("SubtensorModule", "claim_root_with_hotkey"),)

    hotkey_ss58: str = field(
        metadata={"help": "Validator whose accrued basket yield to claim."},
    )

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(calls.SubtensorModule.claim_root_with_hotkey(hotkey=hotkey))

    def summary(self) -> str:
        return f"claim root dividends on {self.hotkey_ss58} (redeem basket shares to root stake)"


@register
@dataclass
class SwapHotkey(Intent):
    """Swap a hotkey for a new one (all subnets, or one netuid).

    Re-keys the neuron identity: the old hotkey's registrations, stake, and
    history move to ``new_hotkey_ss58``, either everywhere (``netuid`` omitted)
    or on a single subnet. The all-subnets swap recycles 0.1 TAO from the
    coldkey; the per-subnet swap recycles 0.001 TAO. Both respect a
    7,200-block (one day) per-(subnet, coldkey) cooldown — the all-subnets
    swap checks and records it on every subnet the old hotkey participates
    in. The old hotkey stops earning immediately, so update running
    miners/validators to sign with the new key at the same time. The new
    hotkey must not already be registered where the swap applies, so plan
    the change rather than iterating. This wraps the legacy ``swap_hotkey``
    extrinsic, deprecated on chain in favor of ``swap_hotkey_v2``;
    behavior is identical to ``swap_hotkey_v2`` with ``keep_stake=false``
    (stake moves to the new hotkey). This rotates a leaked hotkey without
    touching the coldkey; a compromised coldkey needs a coldkey swap
    instead.
    """

    op = "swap_hotkey"
    signer = "coldkey"
    wraps = (("SubtensorModule", "swap_hotkey"),)

    new_hotkey_ss58: str = field(
        metadata={
            "help": "Replacement hotkey that takes over the old hotkey's registrations and stake."
        }
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey being replaced; defaults to the wallet's hotkey."},
    )
    netuid: Optional[int] = field(
        default=None,
        metadata={"help": "Limit the swap to this subnet; omit to swap across all subnets."},
    )

    async def build(self, substrate, wallet: Any):
        old = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.swap_hotkey(
                hotkey=old, new_hotkey=self.new_hotkey_ss58, netuid=self.netuid
            )
        )

    def summary(self) -> str:
        scope = f"netuid {self.netuid}" if self.netuid is not None else "all subnets"
        return f"swap hotkey to {self.new_hotkey_ss58} ({scope})"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["re-keys the neuron identity; the old hotkey stops earning"]
