"""Registration and subnet-lifecycle intents."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from .._generated import storage as st
from ..balance import Balance
from ._money import UNBOUNDED, Spend
from ._root_claim_fee import (
    quote_root_claim_fee,
    root_claim_admission,
    root_claim_reserve,
)
from .base import Intent, IntentPreflight
from .registry import register

# CollateralLockShare is u16 where 65535 = 100%. Matches the chain's
# `get_collateral_lock_share_float` / `get_collateral_requirement_tao`.
_U16_MAX = 65535


async def neuron_registration_split(substrate, netuid: int) -> tuple[Balance, Balance]:
    """Split the current registration price into (burn, lock) TAO shares.

    ``Burn`` is the full floating registration price. The subnet's
    ``CollateralLockShare`` (p) locks ``p * price`` as miner collateral and
    burns the rest. Root (netuid 0) has no collateral path: the full price
    is the burn share and lock is zero.
    """
    if netuid == 0:
        cost_raw = await substrate.query(*st.SubtensorModule.Burn, [netuid])
        return Balance.from_rao(int(cost_raw or 0)), Balance.from_rao(0)
    cost_raw, share_raw = await asyncio.gather(
        substrate.query(*st.SubtensorModule.Burn, [netuid]),
        substrate.query(*st.SubtensorModule.CollateralLockShare, [netuid]),
    )
    cost_rao = int(cost_raw or 0)
    lock_rao = (cost_rao * int(share_raw or 0)) // _U16_MAX
    return Balance.from_rao(cost_rao - lock_rao), Balance.from_rao(lock_rao)


def _registration_split_suffix(burn: Balance, lock: Balance) -> str:
    lock_part = str(lock) if lock.rao else "none"
    return f"burn {burn} · lock {lock_part}"


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

    async def effects(self, substrate, signer_address: str) -> list[str]:
        burn, lock = await neuron_registration_split(substrate, self.netuid)
        lock_line = (
            f"lock {lock} as miner collateral" if lock.rao else "lock none (full cost is burned)"
        )
        return [
            self.summary(),
            f"burn {burn} (destroyed)",
            lock_line,
        ]

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

    async def effects(self, substrate, signer_address: str) -> list[str]:
        burn, _lock = await neuron_registration_split(substrate, 0)
        return [
            self.summary(),
            f"burn {burn} (recycled into issuance)",
            "lock none",
        ]

    def touches_netuids(self) -> list[int]:
        return [0]


class _RootClaimIntent(Intent):
    """Shared, fail-closed root-claim admission and best-effort fee preview."""

    def _claim_hotkeys(self) -> Optional[list[str]]:
        raise NotImplementedError

    def _claim_call(self):
        raise NotImplementedError

    async def _claim_preflight(
        self,
        substrate,
        dispatch_origin: str,
        fee_payer: str,
        *,
        call: Any = None,
    ) -> IntentPreflight:
        hotkeys = self._claim_hotkeys()
        try:
            admission = await root_claim_admission(
                substrate,
                dispatch_origin,
                hotkeys=hotkeys,
            )
        except Exception as error:
            return IntentPreflight(
                effects=[self.summary()],
                warnings=[],
                blocks=[
                    "could not verify the root claim's 256-unit admission budget; "
                    f"refusing to risk the unreduced declared fee ({error})"
                ],
            )

        admission_blocks = admission.blocks()
        if admission_blocks:
            return IntentPreflight(
                effects=[self.summary()],
                warnings=[],
                blocks=admission_blocks,
            )

        async def compose():
            return await substrate.compose(self._claim_call())

        try:
            reserve = await root_claim_reserve(
                substrate,
                fee_payer,
                compose=compose,
                call=call,
            )
        except Exception as error:
            return IntentPreflight(
                effects=[self.summary()],
                warnings=[],
                blocks=[
                    "could not verify the root claim's reserved fee and free TAO; "
                    f"refusing to risk the unreduced declared fee ({error})"
                ],
            )

        quote = await quote_root_claim_fee(
            substrate,
            dispatch_origin,
            fee_payer_address=fee_payer,
            hotkeys=hotkeys,
            compose=compose,
            call=call,
            admission=admission,
            reserve=reserve,
        )
        if quote is None:
            return IntentPreflight(
                effects=[self.summary()],
                warnings=[],
                blocks=reserve.blocks(),
                required_free=reserve.reserved,
                available_free=reserve.free,
                estimated_fee=reserve.reserved if reserve.exact else None,
            )
        return IntentPreflight(
            effects=[self.summary(), *quote.effects()],
            warnings=quote.warnings(),
            blocks=quote.blocks(),
            required_free=quote.reserved,
            available_free=quote.free,
            estimated_fee=quote.reserved if reserve.exact else None,
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        return await self._claim_preflight(
            substrate,
            dispatch_origin,
            fee_payer,
            call=call,
        )

    async def effects(self, substrate, signer_address: str) -> list[str]:
        return (await self._claim_preflight(substrate, signer_address, signer_address)).effects

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return (await self._claim_preflight(substrate, signer_address, signer_address)).warnings

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        return (await self._claim_preflight(substrate, signer_address, signer_address)).blocks


@register
@dataclass
class ClaimRoot(_RootClaimIntent):
    """Redeem accrued root dividends across every validator for the coldkey.

    Root dividends accrue as shares of each validator's basket — an
    escrowed index fund of subnet alpha the chain builds from the validator's
    root dividends per its root weights (see ``set_root_weights``). This call
    redeems the signing coldkey's owed shares on every validator it
    root-stakes to. The ``subnets`` argument is retained for call-data
    compatibility with pre-basket clients and is ignored — baskets have no
    per-subnet claim selection.

    Prefer :class:`ClaimRootWithHotkey` to claim a single validator.

    ``plan`` (and ``btcli root claim --dry-run``) estimates the reserved
    inclusion fee versus the fee that will actually settle, compares that
    spent fee to accrued yield, warns when the claim loses money, and
    refuses when free TAO cannot cover the reserve.
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

    def _claim_hotkeys(self) -> Optional[list[str]]:
        return None

    def _claim_call(self):
        return calls.SubtensorModule.claim_root(subnets=self.subnets)


@register
@dataclass
class ClaimRootWithHotkey(_RootClaimIntent):
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
    per-row cost. The chain reserves a fixed 256-unit declared-work envelope at
    inclusion, independent of the current network count, and refunds the unused
    part after.
    ``plan`` and ``btcli root claim --dry-run`` show reserved versus spent,
    warn when the spent fee exceeds accrued yield, and refuse when free
    TAO cannot cover the reserve.
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

    def _claim_hotkeys(self) -> Optional[list[str]]:
        return [self.hotkey_ss58]

    def _claim_call(self):
        return calls.SubtensorModule.claim_root_with_hotkey(hotkey=self.hotkey_ss58)


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
