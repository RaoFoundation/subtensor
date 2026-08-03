"""Staking intents: add, remove, and move stake."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, ClassVar, Optional

from .._generated import calls
from .._generated import storage as st
from .._generated.runtime_apis import BetaBasketRuntimeApi, StakeInfoRuntimeApi, SwapRuntimeApi
from ..balance import Balance
from ..result import BittensorError
from ..settings import RAO_PER_TAO
from ..signing import public_view
from ._money import ALL, UNBOUNDED, Money, Spend, call_amount
from .base import Intent
from .registry import register

# Default slippage-protection tolerance: the pool price may move at most this
# fraction from the price observed at build time before the call stops filling.
DEFAULT_RATE_TOLERANCE = 0.05

NETUID_HELP = "Subnet the stake lives on (netuid 0 is the root network)."

STAKE_HOTKEY_HELP = "Hotkey the stake is held on (the validator backing the position)."

ORIGIN_NETUID_HELP = "Subnet the stake currently sits on."

DEST_NETUID_HELP = (
    "Subnet the stake ends up on. When it differs from the origin, the position is "
    "swapped through both subnet pools, which can incur slippage on each leg."
)

LIMIT_PRICE_HELP = (
    "Worst pool price you will accept for the swap. The call fails (or fills "
    "partially when allow-partial is set) instead of executing beyond this price."
)

ALLOW_PARTIAL_HELP = (
    "Execute whatever portion fits within the limit price and drop the remainder, "
    "instead of failing the whole call when the limit would be breached."
)

SLIPPAGE_PROTECTION_HELP = (
    "Bound the price the swap may execute at (on by default): the call fails "
    "(`SlippageTooHigh`) instead of filling once the pool price moves more than "
    "`rate_tolerance` from the price at submission. Disable to execute at any price."
)

RATE_TOLERANCE_HELP = (
    "Maximum price move slippage protection accepts, as a fraction (0.05 = 5%). "
    "Ignored when slippage protection is disabled."
)


def _check_rate_tolerance(tolerance: float) -> None:
    if not 0 <= tolerance < 1:
        raise BittensorError(
            f"rate_tolerance must be a fraction in [0, 1), got {tolerance!r} "
            "(0.05 = 5%). To trade with no price bound, disable slippage protection "
            "(slippage_protection=False / --no-slippage-protection) instead."
        )


async def _alpha_price_rao(substrate, netuid: int) -> int:
    """Current spot alpha price (rao per alpha) used to derive a limit price."""
    price = await substrate.runtime_call(*SwapRuntimeApi.current_alpha_price, [netuid])
    if price is None:
        raise BittensorError(
            f"could not read the alpha price for netuid {netuid} to derive a slippage "
            "limit; retry, or disable slippage protection to submit without a bound"
        )
    return int(price)


async def _staked_rao(substrate, wallet: Any, hotkey_ss58: str, netuid: int) -> int:
    """Current stake (rao) the signing coldkey holds on ``hotkey_ss58`` at ``netuid``.

    Resolves an ``amount = "all"`` at build time; refuses to build a no-op when
    there is nothing staked.
    """
    coldkey = public_view(wallet, "coldkey").ss58_address
    info = await substrate.runtime_call(
        *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
        [hotkey_ss58, coldkey, netuid],
    )
    rao = 0 if info is None else int(info["stake"])
    if rao <= 0:
        raise BittensorError(f"nothing to unstake: no stake on {hotkey_ss58} at netuid {netuid}")
    return rao


async def _lock_hotkey(substrate, coldkey_ss58: str, netuid: int) -> Optional[str]:
    """Hotkey a coldkey's conviction lock targets on ``netuid``, or None."""
    rows = await substrate.query_map(*st.SubtensorModule.Lock, [coldkey_ss58])
    for key, _value in rows:
        # Key shape is (netuid, hotkey) when the map is scoped to the coldkey.
        if isinstance(key, (list, tuple)) and len(key) >= 2 and int(key[0]) == netuid:
            return str(key[1])
    return None


async def _availability_rao(substrate, coldkey_ss58: str, netuid: int) -> tuple[int, int, int]:
    """``(total, locked, available)`` rao for a coldkey on one subnet."""
    raw = await substrate.runtime_call(
        *StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
        [[coldkey_ss58], [netuid]],
    )
    entry = ((raw or {}).get(coldkey_ss58) or {}).get(netuid) or {}
    if not entry:
        entry = ((raw or {}).get(coldkey_ss58) or {}).get(str(netuid)) or {}
    return (
        int(entry.get("total") or 0),
        int(entry.get("locked") or 0),
        int(entry.get("available") or 0),
    )


async def _root_claimable_warning(substrate, coldkey_ss58: str, hotkey_ss58: str) -> Optional[str]:
    """Warn when unstaking root would leave basket yield unclaimed.

    Root ``remove_stake`` / ``unstake_all`` move principal only; accrued basket
    entitlement stays owed until ``btcli root claim``. Best-effort: a failed
    payout read is silent so warnings never block the plan.
    """
    try:
        payout = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_basket_payout,
            [hotkey_ss58, coldkey_ss58],
        )
    except Exception:
        return None
    rao = int(payout or 0)
    if rao <= 0:
        return None
    return (
        f"{Balance.from_rao(rao)} remains claimable via `btcli root claim` "
        "(unstaking root principal does not claim basket yield)"
    )


@register
@dataclass
class AddStake(Intent):
    """Stake TAO from the coldkey onto a hotkey.

    Swaps TAO from the coldkey's free balance into the subnet's alpha at the
    current pool price and credits the result to your stake on the hotkey; on
    netuid 0 (root) the stake stays TAO-denominated. The swap moves the pool,
    so large amounts incur slippage. By default the call is slippage-protected:
    it fails (``SlippageTooHigh``) instead of filling once the price rises more
    than ``rate_tolerance`` (5%) above the price at submission — raise the
    tolerance or set ``slippage_protection`` to False to execute at any price,
    or use ``add_stake_limit`` to set an explicit limit price. The position's
    value then follows the pool price and the validator's performance, and can
    be exited later with ``remove_stake``. Fails if the coldkey's free balance
    cannot cover the amount plus the transaction fee, and with ``AmountTooLow``
    when the amount is below the chain minimum of 0.002 TAO plus the swap fee.
    Dynamic subnets also reject a single swap larger than 1000x the pool's TAO
    reserve (``InsufficientLiquidity``).
    """

    op = "add_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "add_stake"), ("SubtensorModule", "add_stake_limit"))
    mev_shield_default = True

    hotkey_ss58: str = field(
        metadata={"help": "Hotkey the stake is added to (the validator you are backing)."}
    )
    netuid: int = field(metadata={"help": NETUID_HELP})
    amount_tao: Money = field(metadata={"help": "How much of the coldkey's free balance to stake."})
    slippage_protection: bool = field(default=True, metadata={"help": SLIPPAGE_PROTECTION_HELP})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_tao = call_amount(
            self.amount_tao, self.wraps[0], "amount_staked", netuid=self.netuid
        )
        _check_rate_tolerance(self.rate_tolerance)

    async def build(self, substrate, wallet: Any):
        if self.slippage_protection:
            price = await _alpha_price_rao(substrate, self.netuid)
            return await substrate.compose(
                calls.SubtensorModule.add_stake_limit(
                    hotkey=self.hotkey_ss58,
                    netuid=self.netuid,
                    amount_staked=self.amount_tao.rao,
                    limit_price=int(price * (1 + self.rate_tolerance)),
                    allow_partial=False,
                )
            )
        return await substrate.compose(
            calls.SubtensorModule.add_stake(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                amount_staked=self.amount_tao.rao,
            )
        )

    def summary(self) -> str:
        note = (
            f" (fails if price moves >{self.rate_tolerance:.2%})"
            if self.slippage_protection
            else " (no slippage protection)"
        )
        return f"stake {self.amount_tao} to {self.hotkey_ss58} on netuid {self.netuid}{note}"

    def spend(self) -> Spend:
        return self.amount_tao


@register
@dataclass
class RemoveStake(Intent):
    """Unstake alpha from a hotkey back to the coldkey.

    Swaps the alpha position back to TAO at the current pool price and credits
    it to the signing coldkey's free balance. Pass ``all`` to exit the entire
    position on that hotkey and subnet (the build fails if nothing is staked
    there). Like staking, the swap moves the pool, so large amounts incur
    slippage. By default the call is slippage-protected: it fails
    (``SlippageTooHigh``) instead of filling once the price falls more than
    ``rate_tolerance`` (5%) below the price at submission — raise the tolerance
    or set ``slippage_protection`` to False to execute at any price, or use
    ``remove_stake_limit`` to set an explicit limit price. The hotkey and
    netuid must match where the stake is actually held, and the subnet must
    have subtoken trading enabled. The requested amount is capped to the
    stake currently available. A partial unstake must leave a remainder
    worth at least 0.002 TAO at the simulated pool price — exit the full
    position instead of leaving dust (``AmountTooLow``).
    """

    op = "remove_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "remove_stake"), ("SubtensorModule", "remove_stake_limit"))
    mev_shield_default = True
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_alpha",)

    hotkey_ss58: str = field(metadata={"help": STAKE_HOTKEY_HELP})
    netuid: int = field(metadata={"help": NETUID_HELP})
    amount_alpha: Money = field(
        metadata={"help": "How much to unstake from this position, or ``all``."}
    )
    slippage_protection: bool = field(default=True, metadata={"help": SLIPPAGE_PROTECTION_HELP})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = call_amount(
            self.amount_alpha, self.wraps[0], "amount_unstaked", netuid=self.netuid, allow_all=True
        )
        _check_rate_tolerance(self.rate_tolerance)

    async def build(self, substrate, wallet: Any):
        if self.amount_alpha == ALL:
            rao = await _staked_rao(substrate, wallet, self.hotkey_ss58, self.netuid)
        else:
            rao = self.amount_alpha.rao
        if self.slippage_protection:
            price = await _alpha_price_rao(substrate, self.netuid)
            return await substrate.compose(
                calls.SubtensorModule.remove_stake_limit(
                    hotkey=self.hotkey_ss58,
                    netuid=self.netuid,
                    amount_unstaked=rao,
                    limit_price=int(price * (1 - self.rate_tolerance)),
                    allow_partial=False,
                )
            )
        return await substrate.compose(
            calls.SubtensorModule.remove_stake(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                amount_unstaked=rao,
            )
        )

    def summary(self) -> str:
        amount = "ALL alpha" if self.amount_alpha == ALL else str(self.amount_alpha)
        note = (
            f" (fails if price moves >{self.rate_tolerance:.2%})"
            if self.slippage_protection
            else " (no slippage protection)"
        )
        return f"unstake {amount} from {self.hotkey_ss58} on netuid {self.netuid}{note}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out: list[str] = []
        if self.amount_alpha == ALL:
            out.append("removes the entire stake from this hotkey on this subnet")
        if self.netuid == 0:
            claimable = await _root_claimable_warning(substrate, signer_address, self.hotkey_ss58)
            if claimable:
                out.append(claimable)
        return out


@register
@dataclass
class MoveStake(Intent):
    """Move alpha between hotkeys and/or subnets.

    Re-delegates an existing position without passing through the coldkey's
    free balance: the stake leaves the origin hotkey on the origin subnet and
    lands on the destination hotkey at the destination subnet. Moving within
    one subnet just changes which validator backs the stake; moving across
    subnets swaps through both pools and can incur slippage on each leg.
    Ownership stays with the signing coldkey — use ``transfer_stake`` to hand
    the position to another coldkey, or ``swap_stake`` when only the subnet
    changes.
    """

    op = "move_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "move_stake"),)
    mev_shield_default = True

    origin_hotkey_ss58: str = field(metadata={"help": "Hotkey the stake moves away from."})
    origin_netuid: int = field(metadata={"help": ORIGIN_NETUID_HELP})
    dest_hotkey_ss58: str = field(metadata={"help": "Hotkey the stake moves to."})
    dest_netuid: int = field(metadata={"help": DEST_NETUID_HELP})
    amount_alpha: Money = field(
        metadata={
            "help": "How much of the origin position to move (an explicit "
            "amount; ``all`` is not accepted)."
        }
    )

    def __post_init__(self):
        self.amount_alpha = call_amount(
            self.amount_alpha, self.wraps[0], "alpha_amount", netuid=self.origin_netuid
        )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.move_stake(
                origin_hotkey=self.origin_hotkey_ss58,
                destination_hotkey=self.dest_hotkey_ss58,
                origin_netuid=self.origin_netuid,
                destination_netuid=self.dest_netuid,
                alpha_amount=self.amount_alpha.rao,
            )
        )

    def summary(self) -> str:
        return (
            f"move {self.amount_alpha} from {self.origin_hotkey_ss58} "
            f"on netuid {self.origin_netuid} to {self.dest_hotkey_ss58} "
            f"on netuid {self.dest_netuid}"
        )

    def touches_netuids(self) -> list[int]:
        return [self.origin_netuid, self.dest_netuid]


@register
@dataclass
class AddStakeLimit(Intent):
    """Stake TAO with a limit price (slippage protection).

    Same as ``add_stake`` except the TAO-to-alpha swap only executes while the
    pool price stays within the limit. With ``allow_partial`` the call stakes
    as much as fits under the limit and leaves the rest in the free balance;
    without it the whole call fails once the limit would be breached. Like
    ``add_stake``, the amount must be at least the chain minimum of 0.002 TAO
    plus the swap fee (``AmountTooLow``). Prefer this over plain ``add_stake``
    for large amounts or thin pools, where the swap itself moves the price.
    """

    op = "add_stake_limit"
    signer = "coldkey"
    wraps = (("SubtensorModule", "add_stake_limit"),)
    mev_shield_default = True

    hotkey_ss58: str = field(
        metadata={"help": "Hotkey the stake is added to (the validator you are backing)."}
    )
    netuid: int = field(metadata={"help": NETUID_HELP})
    amount_tao: Money = field(metadata={"help": "How much of the coldkey's free balance to stake."})
    limit_price_rao: int = field(metadata={"help": LIMIT_PRICE_HELP})
    allow_partial: bool = field(default=False, metadata={"help": ALLOW_PARTIAL_HELP})

    def __post_init__(self):
        self.amount_tao = call_amount(
            self.amount_tao, self.wraps[0], "amount_staked", netuid=self.netuid
        )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.add_stake_limit(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                amount_staked=self.amount_tao.rao,
                limit_price=self.limit_price_rao,
                allow_partial=self.allow_partial,
            )
        )

    def summary(self) -> str:
        return (
            f"stake {self.amount_tao} to {self.hotkey_ss58} on netuid {self.netuid} "
            f"(limit {self.limit_price_rao} rao/alpha)"
        )

    def spend(self) -> Spend:
        return self.amount_tao


@register
@dataclass
class RemoveStakeLimit(Intent):
    """Unstake alpha with a limit price (slippage protection).

    Same as ``remove_stake`` except the alpha-to-TAO swap only executes while
    the pool price stays within the limit. With ``allow_partial`` it unstakes
    what it can within the limit and leaves the rest staked; without it the
    whole call fails once the limit would be breached. Pass ``all`` to target
    the entire position (the build fails if nothing is staked there). Like
    ``remove_stake``, a partial unstake must leave a remainder worth at
    least 0.002 TAO at the simulated pool price (``AmountTooLow``). Prefer
    this over plain ``remove_stake`` when exiting large positions.
    """

    op = "remove_stake_limit"
    signer = "coldkey"
    wraps = (("SubtensorModule", "remove_stake_limit"),)
    mev_shield_default = True
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_alpha",)

    hotkey_ss58: str = field(metadata={"help": STAKE_HOTKEY_HELP})
    netuid: int = field(metadata={"help": NETUID_HELP})
    amount_alpha: Money = field(
        metadata={"help": "How much to unstake from this position, or ``all``."}
    )
    limit_price_rao: int = field(metadata={"help": LIMIT_PRICE_HELP})
    allow_partial: bool = field(default=False, metadata={"help": ALLOW_PARTIAL_HELP})

    def __post_init__(self):
        self.amount_alpha = call_amount(
            self.amount_alpha, self.wraps[0], "amount_unstaked", netuid=self.netuid, allow_all=True
        )

    async def build(self, substrate, wallet: Any):
        if self.amount_alpha == ALL:
            rao = await _staked_rao(substrate, wallet, self.hotkey_ss58, self.netuid)
        else:
            rao = self.amount_alpha.rao
        return await substrate.compose(
            calls.SubtensorModule.remove_stake_limit(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                amount_unstaked=rao,
                limit_price=self.limit_price_rao,
                allow_partial=self.allow_partial,
            )
        )

    def summary(self) -> str:
        amount = "ALL alpha" if self.amount_alpha == ALL else str(self.amount_alpha)
        return (
            f"unstake {amount} from {self.hotkey_ss58} on netuid "
            f"{self.netuid} (limit {self.limit_price_rao} rao/alpha)"
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out: list[str] = []
        if self.amount_alpha == ALL:
            out.append("removes the entire stake from this hotkey on this subnet")
        if self.netuid == 0:
            claimable = await _root_claimable_warning(substrate, signer_address, self.hotkey_ss58)
            if claimable:
                out.append(claimable)
        return out


@register
@dataclass
class UnstakeAll(Intent):
    """Unstake everything from a hotkey across all subnets.

    Sweeps the signing coldkey's entire stake held on this hotkey across every
    subnet (root included) back to TAO in the coldkey's free balance. Subnets
    where subtoken trading is disabled or where the position fails validation
    (e.g. dust below the chain minimum) are silently skipped, so the call can
    succeed while leaving some positions untouched. Alpha positions are sold
    at each pool's current price with no limit protection, so large positions
    can incur significant slippage. Use ``remove_stake`` to exit a single
    subnet, or ``unstake_all_alpha`` to consolidate onto root while staying
    staked.
    """

    op = "unstake_all"
    signer = "coldkey"
    wraps = (("SubtensorModule", "unstake_all"),)
    mev_shield_default = True

    hotkey_ss58: str = field(metadata={"help": "Hotkey whose entire stake is removed."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.SubtensorModule.unstake_all(hotkey=self.hotkey_ss58))

    def summary(self) -> str:
        return f"unstake ALL from {self.hotkey_ss58}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out = ["removes the entire stake from this hotkey"]
        claimable = await _root_claimable_warning(substrate, signer_address, self.hotkey_ss58)
        if claimable:
            out.append(claimable)
        return out

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class UnstakeAllAlpha(Intent):
    """Unstake all alpha from a hotkey across subnets (moves it to root).

    Sells every alpha position the signing coldkey holds on this hotkey and
    restakes the proceeds as TAO on the root network (netuid 0), instead of
    releasing them to the free balance. Subnets where subtoken trading is
    disabled or where the position fails validation are silently skipped.
    Use it to consolidate onto root while keeping funds staked; use
    ``unstake_all`` to exit to free balance instead. Each pool swap happens
    at the current price with no limit protection, so large positions can
    incur slippage.
    """

    op = "unstake_all_alpha"
    signer = "coldkey"
    wraps = (("SubtensorModule", "unstake_all_alpha"),)
    mev_shield_default = True

    hotkey_ss58: str = field(metadata={"help": "Hotkey whose alpha stake is moved to root."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.unstake_all_alpha(hotkey=self.hotkey_ss58)
        )

    def summary(self) -> str:
        return f"unstake ALL alpha from {self.hotkey_ss58}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["removes all alpha stake from this hotkey across every subnet"]

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class SwapStake(Intent):
    """Swap stake on one hotkey between two subnets.

    Moves part of a position from the origin subnet to the destination subnet
    while staying on the same hotkey: the alpha is swapped to TAO in the
    origin pool and then to alpha in the destination pool, so both legs can
    incur slippage. By default the call is slippage-protected: it fails
    (``SlippageTooHigh``) instead of filling once the origin/destination price
    ratio falls more than ``rate_tolerance`` (5%) below the ratio at submission
    — raise the tolerance or set ``slippage_protection`` to False to execute at
    any price. The two netuids must differ (``SameNetuid``). Use ``move_stake``
    when the hotkey should change too, and ``remove_stake`` plus ``add_stake``
    only if you want to control each leg separately.
    """

    op = "swap_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "swap_stake"), ("SubtensorModule", "swap_stake_limit"))
    mev_shield_default = True

    hotkey_ss58: str = field(metadata={"help": STAKE_HOTKEY_HELP})
    origin_netuid: int = field(metadata={"help": ORIGIN_NETUID_HELP})
    dest_netuid: int = field(metadata={"help": DEST_NETUID_HELP})
    amount_alpha: Money = field(
        metadata={
            "help": "How much of the origin position to swap across (an "
            "explicit amount; ``all`` is not accepted)."
        }
    )
    slippage_protection: bool = field(default=True, metadata={"help": SLIPPAGE_PROTECTION_HELP})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = call_amount(
            self.amount_alpha, self.wraps[0], "alpha_amount", netuid=self.origin_netuid
        )
        _check_rate_tolerance(self.rate_tolerance)

    async def build(self, substrate, wallet: Any):
        if self.slippage_protection:
            origin_price = await _alpha_price_rao(substrate, self.origin_netuid)
            dest_price = await _alpha_price_rao(substrate, self.dest_netuid)
            if dest_price <= 0:
                raise BittensorError(
                    f"netuid {self.dest_netuid} has no alpha price; cannot derive a "
                    "slippage limit — disable slippage protection to submit anyway"
                )
            # The chain compares the limit against the origin/destination price
            # ratio (falling as the swap executes), scaled by 1e9.
            ratio_rao = origin_price * RAO_PER_TAO // dest_price
            return await substrate.compose(
                calls.SubtensorModule.swap_stake_limit(
                    hotkey=self.hotkey_ss58,
                    origin_netuid=self.origin_netuid,
                    destination_netuid=self.dest_netuid,
                    alpha_amount=self.amount_alpha.rao,
                    limit_price=int(ratio_rao * (1 - self.rate_tolerance)),
                    allow_partial=False,
                )
            )
        return await substrate.compose(
            calls.SubtensorModule.swap_stake(
                hotkey=self.hotkey_ss58,
                origin_netuid=self.origin_netuid,
                destination_netuid=self.dest_netuid,
                alpha_amount=self.amount_alpha.rao,
            )
        )

    def summary(self) -> str:
        note = (
            f" (fails if price ratio moves >{self.rate_tolerance:.2%})"
            if self.slippage_protection
            else " (no slippage protection)"
        )
        return (
            f"swap {self.amount_alpha} on {self.hotkey_ss58} from netuid "
            f"{self.origin_netuid} to netuid {self.dest_netuid}{note}"
        )

    def touches_netuids(self) -> list[int]:
        return [self.origin_netuid, self.dest_netuid]


@register
@dataclass
class TransferStake(Intent):
    """Transfer stake ownership to another coldkey.

    Hands the position itself to the destination coldkey: after this call that
    coldkey — not you — controls and can unstake those funds, so this is a
    transfer of value and is irreversible. Double-check the destination
    address. The stake stays on the same hotkey by default; pass a destination
    hotkey to re-delegate it in the same call (dispatched as
    ``transfer_stake_and_hotkey``).

    Conviction locks: a lock is a coldkey-wide floor (stake hotkey and lock
    hotkey may differ). Amounts at or below free (unlocked) alpha transfer
    without moving the lock. Amounts above free pull locked mass with the
    stake — that locked portion must land on the **receiver's** existing lock
    hotkey or the call fails with ``LockHotkeyMismatch``. Fix: keep
    ``hotkey_ss58`` as the hotkey that holds the stake and set
    ``dest_hotkey_ss58`` to the receiver's lock hotkey (see ``btcli stake
    list`` / ``btcli lock show``). Pulling from the lock hotkey when the stake
    still sits elsewhere fails with ``NotEnoughStakeToWithdraw``. See the
    conviction guide's "Transferring locked stake" section.

    It can also land on a different subnet, swapping through both pools (with
    slippage) when the netuids differ. Fails with ``TransferDisallowed`` when
    the subnet owner has disabled stake transfers on the origin or destination
    subnet. A spend-cap policy treats this as an unbounded spend and blocks it
    until the cap is raised. Use ``move_stake`` to re-delegate without changing
    owners.
    """

    op = "transfer_stake"
    signer = "coldkey"
    wraps = (
        ("SubtensorModule", "transfer_stake"),
        ("SubtensorModule", "transfer_stake_and_hotkey"),
    )
    mev_shield_default = True

    dest_coldkey_ss58: str = field(
        metadata={"help": "Coldkey that becomes the new owner of the stake."}
    )
    hotkey_ss58: str = field(metadata={"help": STAKE_HOTKEY_HELP})
    origin_netuid: int = field(metadata={"help": ORIGIN_NETUID_HELP})
    dest_netuid: int = field(metadata={"help": DEST_NETUID_HELP})
    amount_alpha: Money = field(
        metadata={
            "help": "How much of the position to hand over (an explicit "
            "amount; ``all`` is not accepted)."
        }
    )
    dest_hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Hotkey the stake lands on. Defaults to the origin hotkey. "
            "Required when the transfer moves locked alpha and the receiver "
            "already locks to a different hotkey — pass their lock hotkey "
            "(see `btcli stake list` / `btcli lock show`)."
        },
    )

    def __post_init__(self):
        self.amount_alpha = call_amount(
            self.amount_alpha, self.wraps[0], "alpha_amount", netuid=self.origin_netuid
        )

    def _changes_hotkey(self) -> bool:
        return self.dest_hotkey_ss58 is not None and self.dest_hotkey_ss58 != self.hotkey_ss58

    async def build(self, substrate, wallet: Any):
        if self._changes_hotkey():
            return await substrate.compose(
                calls.SubtensorModule.transfer_stake_and_hotkey(
                    destination_coldkey=self.dest_coldkey_ss58,
                    origin_hotkey=self.hotkey_ss58,
                    destination_hotkey=self.dest_hotkey_ss58,
                    origin_netuid=self.origin_netuid,
                    destination_netuid=self.dest_netuid,
                    alpha_amount=self.amount_alpha.rao,
                )
            )
        return await substrate.compose(
            calls.SubtensorModule.transfer_stake(
                destination_coldkey=self.dest_coldkey_ss58,
                hotkey=self.hotkey_ss58,
                origin_netuid=self.origin_netuid,
                destination_netuid=self.dest_netuid,
                alpha_amount=self.amount_alpha.rao,
            )
        )

    def summary(self) -> str:
        hotkey_note = (
            f" (landing on hotkey {self.dest_hotkey_ss58})" if self._changes_hotkey() else ""
        )
        return (
            f"transfer {self.amount_alpha} on netuid {self.origin_netuid} to "
            f"coldkey {self.dest_coldkey_ss58}{hotkey_note}"
        )

    def _landing_hotkey(self) -> str:
        return self.dest_hotkey_ss58 if self.dest_hotkey_ss58 is not None else self.hotkey_ss58

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out = ["transfers stake OWNERSHIP to another coldkey"]
        # Lock mass only follows same-subnet transfers; cross-subnet swaps have
        # their own availability checks and are left to the chain.
        if self.origin_netuid != self.dest_netuid:
            return out

        amount_rao = int(self.amount_alpha.rao)
        landing = self._landing_hotkey()
        (
            (_total, locked_rao, available_rao),
            sender_lock_hotkey,
            receiver_lock_hotkey,
            position_info,
        ) = await asyncio.gather(
            _availability_rao(substrate, signer_address, self.origin_netuid),
            _lock_hotkey(substrate, signer_address, self.origin_netuid),
            _lock_hotkey(substrate, self.dest_coldkey_ss58, self.dest_netuid),
            substrate.runtime_call(
                *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
                [self.hotkey_ss58, signer_address, self.origin_netuid],
            ),
        )
        position_rao = 0 if position_info is None else int(position_info.get("stake") or 0)

        if position_rao < amount_rao:
            out.append(
                f"origin hotkey {self.hotkey_ss58} holds only "
                f"{Balance.from_rao(position_rao, self.origin_netuid)} on netuid "
                f"{self.origin_netuid} — transfer will fail with NotEnoughStakeToWithdraw; "
                f"move stake onto this hotkey first, or pass a different origin hotkey"
            )

        if sender_lock_hotkey and locked_rao > 0 and sender_lock_hotkey != self.hotkey_ss58:
            out.append(
                f"your conviction lock on netuid {self.origin_netuid} targets "
                f"{sender_lock_hotkey}, but stake is leaving {self.hotkey_ss58} "
                f"(lock hotkey and stake hotkey can differ)"
            )

        if amount_rao > available_rao and locked_rao > 0:
            locked_moving = min(amount_rao - available_rao, locked_rao)
            out.append(
                f"{Balance.from_rao(locked_moving, self.origin_netuid)} locked alpha "
                f"will move with this transfer "
                f"({Balance.from_rao(available_rao, self.origin_netuid)} is free; "
                f"the rest pulls from the conviction lock)"
            )
            if receiver_lock_hotkey and receiver_lock_hotkey != landing:
                out.append(
                    f"receiver already locks to {receiver_lock_hotkey}; locked alpha "
                    f"can only land on that hotkey — pass "
                    f"--destination-hotkey {receiver_lock_hotkey} "
                    f"(current landing hotkey is {landing}) or transfer only the "
                    f"free amount"
                )
            elif not receiver_lock_hotkey and landing != (sender_lock_hotkey or landing):
                out.append(
                    f"receiver has no lock yet; locked alpha will create one on "
                    f"landing hotkey {landing}"
                )
        return out

    def touches_netuids(self) -> list[int]:
        return [self.origin_netuid, self.dest_netuid]

    def spend(self) -> Spend:
        # Moves an alpha position out to another coldkey; not TAO-denominated and
        # not cheaply bounded here, so a spend cap must block it until raised.
        return UNBOUNDED


@register
@dataclass
class SetAutoStake(Intent):
    """Auto-stake future mining rewards on a subnet to a chosen hotkey.

    Sets the coldkey's autostake destination for the subnet: all future
    rewards earned there are automatically staked to the chosen hotkey
    (defaulting to the wallet's own hotkey) instead of accumulating unstaked.
    A configuration change only — it moves no funds by itself and applies just
    to that subnet. The hotkey must be registered on the subnet
    (``HotKeyNotRegisteredInSubNet``), and setting the hotkey that is
    already the destination fails (``SameAutoStakeHotkeyAlreadySet``). Call
    it again with a different hotkey to redirect; read the current setting
    back with the ``auto_stake`` read.
    """

    op = "set_auto_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_coldkey_auto_stake_hotkey"),)

    netuid: int = field(metadata={"help": "Subnet whose future rewards are auto-staked."})
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey the rewards are staked to. Defaults to the wallet's own hotkey."},
    )

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.set_coldkey_auto_stake_hotkey(netuid=self.netuid, hotkey=hotkey)
        )

    def summary(self) -> str:
        target = self.hotkey_ss58 or "the wallet hotkey"
        return f"auto-stake rewards on netuid {self.netuid} to {target}"
