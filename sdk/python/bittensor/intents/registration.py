"""Registration and subnet-lifecycle intents."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ._money import UNBOUNDED, Spend
from .base import Intent
from .registry import register

# Variants of the runtime's RootClaimTypeEnum (subtensor/pallets/subtensor/src/lib.rs).
ROOT_CLAIM_TYPES = ("Swap", "Keep", "KeepSubnets")

ROOT_CLAIM_TYPE_HELP = (
    "How root alpha emission is claimed. One of: "
    + ", ".join(ROOT_CLAIM_TYPES)
    + ". Swap converts all alpha emission to TAO, Keep keeps everything as alpha, "
    "KeepSubnets keeps alpha only on the subnets given via --subnets and swaps the rest."
)


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
    Placement is stake-based rather than burn-based: root slots are limited, so
    joining a full root network evicts the member with the least stake, and a
    hotkey without enough stake behind it will not hold a seat. Root
    registrations are also capped per block (``max_registrations_per_block``)
    and per interval (three times ``target_registrations_per_interval``);
    hitting either cap fails until the window passes. Use
    ``burned_register`` for ordinary subnets.
    """

    op = "root_register"
    signer = "coldkey"
    wraps = (("SubtensorModule", "root_register"),)

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
    """Claim accumulated root dividends from one or more subnets.

    Pays out the signing coldkey's accrued root-stake dividends from the listed
    subnets. What the payout looks like depends on the coldkey's root claim
    type (see ``set_root_claim_type``): swapped to TAO, kept as subnet alpha,
    or a per-subnet mix. At most 5 subnets per call — an empty or longer list
    fails with ``InvalidSubnetNumber``. Subnets whose accrued dividends are
    below the per-subnet claim threshold (default 500,000 rao; adjustable by
    the subnet owner or root) are silently skipped while the transaction
    still succeeds. Unclaimed dividends simply keep accruing — there is no
    deadline — but each call pays out only the subnets listed.
    """

    op = "claim_root"
    signer = "coldkey"
    wraps = (("SubtensorModule", "claim_root"),)

    subnets: list[int] = field(
        metadata={"help": "Netuids to claim accumulated root dividends from; at most 5 per call."}
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.claim_root(subnets=[int(n) for n in self.subnets])
        )

    def summary(self) -> str:
        return f"claim root dividends from subnets {self.subnets}"


@register
@dataclass
class SetRootClaimType(Intent):
    """Set how a coldkey's root alpha emission is claimed.

    Controls what happens to root dividends when they are claimed (see
    ``claim_root``): ``Swap`` converts all alpha emission to TAO (the chain
    default), ``Keep`` keeps everything as subnet alpha, and ``KeepSubnets``
    keeps alpha on the listed ``subnets`` while swapping the rest. The setting
    is per-coldkey and persists until changed again; it does not move anything
    already claimed. Read it back with the ``root_claim_type`` read.
    """

    op = "set_root_claim_type"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_root_claim_type"),)

    claim_type: str = field(default="Swap", metadata={"help": ROOT_CLAIM_TYPE_HELP})
    subnets: Optional[list] = field(
        default=None,
        metadata={"help": "Netuids to keep alpha on; required for KeepSubnets, invalid otherwise."},
    )

    def __post_init__(self):
        if self.claim_type not in ROOT_CLAIM_TYPES:
            raise ValueError(
                f"claim_type must be one of {ROOT_CLAIM_TYPES}, got {self.claim_type!r}"
            )
        if self.claim_type == "KeepSubnets" and not self.subnets:
            raise ValueError("claim_type 'KeepSubnets' requires a non-empty subnets list")
        if self.claim_type != "KeepSubnets" and self.subnets:
            raise ValueError(
                f"subnets is only valid for claim_type 'KeepSubnets', not {self.claim_type!r}"
            )

    async def build(self, substrate, wallet: Any):
        if self.claim_type == "KeepSubnets":
            value: Any = {"KeepSubnets": {"subnets": sorted({int(n) for n in self.subnets})}}
        else:
            value = self.claim_type
        return await substrate.compose(
            calls.SubtensorModule.set_root_claim_type(new_root_claim_type=value)
        )

    def summary(self) -> str:
        if self.claim_type == "KeepSubnets":
            return f"set root claim type to keep alpha on subnets {sorted(self.subnets)}"
        return f"set root claim type to {self.claim_type}"


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
