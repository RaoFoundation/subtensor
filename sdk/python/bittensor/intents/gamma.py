"""Gamma intents: minting, deposits and daily spend caps, and spending."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from decimal import Decimal
from typing import Any, ClassVar, Optional

from .._generated import calls
from .._generated import storage as st
from .._generated.runtime_apis import GammaRuntimeApi, StakeInfoRuntimeApi
from ..balance import Balance
from ..hyperparams import U64_MAX
from ..result import BittensorError, chain_error_from_dispatch
from ..sp_core import ss58_decode
from ._money import (
    ALL,
    UNBOUNDED,
    Money,
    Spend,
    alpha_amount,
    gamma_amount,
    gamma_from_units,
    gamma_units,
    tao_amount,
)
from .base import Intent, IntentPreflight
from .registry import register
from .staking import (
    DEFAULT_RATE_TOLERANCE,
    STAKE_HOTKEY_HELP,
    _check_rate_tolerance,
    _staked_rao,
)

# Least gamma a spend or move takes, and a move leaves, unless it empties the deposit, in
# units (`MIN_GAMMA_AMOUNT`).
MIN_GAMMA_AMOUNT = 100_000_000

RATE_TOLERANCE_HELP = "Maximum fall of the minted gamma below the quote, as a fraction (0.05 = 5%)."
DAILY_SPEND_CAP_HELP = (
    "The deposit's daily spend cap, in gamma: 0 pauses it, and any other cap is at least 0.1. A "
    "new deposit opens with it, and an existing one must already have it. Omit to keep the "
    "deposit's cap, which for a new deposit is none."
)


async def _quote(substrate, method: tuple, netuid: int, amount: int) -> tuple[int, int, int]:
    """A mint's alpha burned, TAO value, and gamma; raises the chain error that stops it."""
    quote = await substrate.runtime_call(*method, [netuid, amount])
    if "Err" in quote:
        raise chain_error_from_dispatch(quote["Err"])
    return tuple(int(part) for part in quote["Ok"])


def _min_out(gamma: int, tolerance: float) -> int:
    return int(gamma * (1 - tolerance))


def _daily_spend_cap(value: Optional[Money]) -> Optional[Money]:
    if value is None:
        return None
    cap = gamma_amount(value)
    if 0 < gamma_units(cap) < MIN_GAMMA_AMOUNT:
        raise BittensorError("a daily spend cap is 0 or at least 0.1 gamma")
    return cap


def _cap_units(value: Optional[Money]) -> Optional[int]:
    return None if value is None else gamma_units(value)


async def _deposit(substrate, netuid: int, hotkey_ss58: str) -> Optional[dict]:
    return await substrate.query(*st.SubtensorModule.GammaDeposits, [netuid, hotkey_ss58])


async def _new_deposit_blocks(substrate, hotkey_ss58: str, signer: str) -> list[str]:
    """Match runtime Owner.try_get: a stored zero account is still an owner."""
    block_hash = await substrate.block_hash()
    if not await substrate.storage_exists(
        *st.SubtensorModule.Owner, [hotkey_ss58], block_hash=block_hash
    ):
        return []
    owner = await substrate.query(*st.SubtensorModule.Owner, [hotkey_ss58], block_hash=block_hash)
    if bytes(ss58_decode(str(owner))) != bytes(ss58_decode(signer)):
        return [
            f"the hotkey {hotkey_ss58} belongs to another coldkey; "
            "use your own or an unassociated hotkey"
        ]
    return []


def _own_deposit_blocks(deposit: Optional[dict], signer: str, hotkey_ss58: str) -> list[str]:
    """What stops the signer acting on the deposit at ``hotkey_ss58`` as its owner."""
    if deposit is None:
        return [f"there is no gamma deposit at {hotkey_ss58} on this subnet"]
    if str(deposit["coldkey"]) != signer:
        return [f"the gamma deposit at {hotkey_ss58} belongs to another coldkey"]
    return []


def _gamma_text(units: int) -> str:
    return f"{Decimal(gamma_from_units(units)):,f} gamma"


def _cap(deposit: dict) -> Optional[int]:
    """A deposit's daily spend cap, in gamma units."""
    cap = deposit["daily_spend_cap"]
    return None if cap is None else int(cap["cap"])


def _cap_text(units: Optional[int]) -> str:
    return "no daily spend cap" if units is None else f"a daily spend cap of {_gamma_text(units)}"


async def _mint_preflight(
    intent, preview: IntentPreflight, substrate, signer: str, quote
) -> IntentPreflight:
    """A mint's preview: its quote, the deposit it credits, and the hard stops the chain
    would refuse it with."""
    alpha, value, gamma = await quote
    deposit = await _deposit(substrate, intent.netuid, intent.hotkey_ss58)
    cap = _cap_units(intent.daily_spend_cap_gamma)
    if deposit is not None and str(deposit["coldkey"]) != signer:
        preview.blocks.append(
            f"the gamma deposit at {intent.hotkey_ss58} belongs to another coldkey; "
            "mint to a fresh hotkey"
        )
    elif deposit is not None and cap not in (None, _cap(deposit)):
        preview.blocks.append(
            f"the gamma deposit at {intent.hotkey_ss58} has "
            f"{_cap_text(_cap(deposit))}; mint without a cap, or set its cap "
            "first"
        )
    if deposit is None:
        preview.blocks += await _new_deposit_blocks(substrate, intent.hotkey_ss58, signer)
    if deposit is not None:
        cap = _cap(deposit)
    minimum = _min_out(gamma, intent.rate_tolerance)
    preview.facts = [
        ("gamma", f"~{_gamma_text(gamma)}"),
        ("minimum", _gamma_text(minimum)),
        ("alpha burned", f"~{Balance.from_rao(alpha, intent.netuid)}"),
        ("value", f"~{Balance.from_rao(value)}"),
        ("deposit", "new" if deposit is None else f"holds {_gamma_text(int(deposit['balance']))}"),
        ("daily spend cap", "none" if cap is None else _gamma_text(cap)),
        *preview.facts,
    ]
    preview.facts_title = "Quote"
    preview.preview = {
        "gamma": gamma_from_units(gamma),
        "minimum_gamma": gamma_from_units(minimum),
        "alpha_burned": format(Balance.from_rao(alpha, intent.netuid).decimal, "f"),
        "tao_value": format(Balance.from_rao(value).decimal, "f"),
        "deposit_balance_gamma": None if deposit is None else gamma_from_units(deposit["balance"]),
        "daily_spend_cap_gamma": None if cap is None else gamma_from_units(cap),
    }
    return preview


@register
@dataclass
class MintGamma(Intent):
    """Mint gamma with TAO into your deposit at an API-key hotkey.

    Buys the subnet's alpha with ``amount_tao``, burns it, and credits its value
    in gamma, one per US dollar, to the deposit at ``hotkey_ss58``, opening it
    when absent. The alpha is valued at the lower of the subnet's gamma price
    and its spot price before the buy. The hotkey spends the deposit with
    ``spend_gamma``. A new deposit has no daily spend cap unless
    ``daily_spend_cap_gamma`` sets one, and a mint naming a cap fails on a
    deposit with another.
    Fails rather than minting less than the current quote less
    ``rate_tolerance``. A plan's ``preview`` holds the quote's ``gamma``,
    ``minimum_gamma``, ``alpha_burned`` and ``tao_value``, and the deposit's
    balance and cap as ``deposit_balance_gamma`` (None when the mint opens it)
    and ``daily_spend_cap_gamma``. The result's ``data["minted"]`` holds the
    deposit's ``hotkey_ss58`` and ``daily_spend_cap_gamma``, and the
    ``alpha_burned``, ``tao_value`` and ``gamma`` minted.
    """

    op = "mint_gamma"
    signer = "coldkey"
    wraps = (("SubtensorModule", "mint_gamma"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet whose alpha is bought and burned."})
    hotkey_ss58: str = field(
        metadata={"help": "Hotkey of the deposit credited: an API key, not a staking hotkey."}
    )
    amount_tao: Money = field(metadata={"help": "TAO spent buying the alpha that is burned."})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )
    daily_spend_cap_gamma: Optional[Money] = field(
        default=None, metadata={"help": DAILY_SPEND_CAP_HELP}
    )

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)
        self.daily_spend_cap_gamma = _daily_spend_cap(self.daily_spend_cap_gamma)

    def _quote(self, substrate):
        return _quote(substrate, GammaRuntimeApi.sim_mint_gamma, self.netuid, self.amount_tao.rao)

    async def build(self, substrate, wallet: Any):
        _, _, gamma = await self._quote(substrate)
        return await substrate.compose(
            calls.SubtensorModule.mint_gamma(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                amount=self.amount_tao.rao,
                min_amount_out=_min_out(gamma, self.rate_tolerance),
                daily_spend_cap=_cap_units(self.daily_spend_cap_gamma),
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        return await _mint_preflight(
            self, preview, substrate, dispatch_origin, self._quote(substrate)
        )

    def summary(self) -> str:
        return (
            f"mint gamma with {self.amount_tao} on netuid {self.netuid} to {self.hotkey_ss58}"
            f" (fails if the quote falls >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        return self.amount_tao


@register
@dataclass
class MintGammaWithStake(Intent):
    """Mint gamma by burning your stake into your deposit at an API-key hotkey.

    Burns up to ``amount_alpha`` (``all`` for the whole stake) of your stake
    on ``origin_hotkey_ss58`` and credits its value in gamma to the deposit at
    ``hotkey_ss58``, valued as ``mint_gamma`` values bought alpha. The stake is
    destroyed, not moved. ``daily_spend_cap_gamma`` names the deposit's cap as
    for ``mint_gamma``. Fails rather than minting less than the current quote
    less ``rate_tolerance``. Its plan's ``preview`` and result's
    ``data["minted"]`` are as for ``mint_gamma``.
    """

    op = "mint_gamma_with_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "mint_gamma_with_stake"),)
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_alpha",)

    netuid: int = field(metadata={"help": "Subnet of the stake burned."})
    hotkey_ss58: str = field(
        metadata={"help": "Hotkey of the deposit credited: an API key, not a staking hotkey."}
    )
    amount_alpha: Money = field(metadata={"help": "Alpha of stake burned."})
    origin_hotkey_ss58: str = field(metadata={"help": STAKE_HOTKEY_HELP})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )
    daily_spend_cap_gamma: Optional[Money] = field(
        default=None, metadata={"help": DAILY_SPEND_CAP_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid, allow_all=True)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)
        self.daily_spend_cap_gamma = _daily_spend_cap(self.daily_spend_cap_gamma)

    def _burned(self, staked: int) -> int:
        """The alpha the chain burns: at most the stake held."""
        return staked if self.amount_alpha == ALL else min(self.amount_alpha.rao, staked)

    def _quote(self, substrate, alpha: int):
        return _quote(substrate, GammaRuntimeApi.sim_mint_gamma_with_stake, self.netuid, alpha)

    async def build(self, substrate, wallet: Any):
        staked = await _staked_rao(
            substrate, wallet, self.origin_hotkey_ss58, self.netuid, action="burn into gamma"
        )
        _, _, gamma = await self._quote(substrate, self._burned(staked))
        return await substrate.compose(
            calls.SubtensorModule.mint_gamma_with_stake(
                origin_hotkey=self.origin_hotkey_ss58,
                destination_hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                alpha_amount=U64_MAX if self.amount_alpha == ALL else self.amount_alpha.rao,
                min_amount_out=_min_out(gamma, self.rate_tolerance),
                daily_spend_cap=_cap_units(self.daily_spend_cap_gamma),
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        info = await substrate.runtime_call(
            *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
            [self.origin_hotkey_ss58, dispatch_origin, self.netuid],
        )
        staked = 0 if info is None else int(info["stake"])
        alpha = self._burned(staked)
        if self.amount_alpha != ALL and alpha < self.amount_alpha.rao:
            preview.warnings.append(
                f"only {Balance.from_rao(staked, self.netuid)} is staked; that is burned"
            )
        return await _mint_preflight(
            self, preview, substrate, dispatch_origin, self._quote(substrate, alpha)
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.amount_alpha == ALL:
            return ["burns the entire stake on this hotkey on this subnet"]
        return []

    def summary(self) -> str:
        amount = "all" if self.amount_alpha == ALL else self.amount_alpha
        return (
            f"burn {amount} of stake into gamma on netuid {self.netuid}"
            f" to {self.hotkey_ss58} (fails if the quote falls >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        # Destroys stake; not TAO-denominated and not cheaply bounded here, so a
        # spend cap must block it until raised.
        return UNBOUNDED


@register
@dataclass
class MoveGamma(Intent):
    """Move gamma between two of your deposits on a subnet.

    Moves up to ``amount_gamma`` (``all`` for the whole balance) from the
    deposit at ``origin_hotkey_ss58`` to the one at
    ``destination_hotkey_ss58``: the whole balance, or at least 0.1 gamma
    leaving at least as much. A destination with no deposit is opened with the
    origin's daily spend cap, and a move that empties the origin closes it.
    """

    op = "move_gamma"
    signer = "coldkey"
    wraps = (("SubtensorModule", "move_gamma"),)
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_gamma",)

    netuid: int = field(metadata={"help": "Subnet of both deposits."})
    origin_hotkey_ss58: str = field(metadata={"help": "Hotkey of the deposit moved from."})
    destination_hotkey_ss58: str = field(metadata={"help": "Hotkey of the deposit moved to."})
    amount_gamma: Money = field(metadata={"help": "Gamma moved."})

    def __post_init__(self):
        if self.origin_hotkey_ss58 == self.destination_hotkey_ss58:
            raise BittensorError("move_gamma: the origin and destination hotkeys are the same")
        self.amount_gamma = gamma_amount(self.amount_gamma, allow_all=True)
        if self.amount_gamma != ALL and not gamma_units(self.amount_gamma):
            raise BittensorError("move_gamma: the amount must be above 0")

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.move_gamma(
                origin_hotkey=self.origin_hotkey_ss58,
                destination_hotkey=self.destination_hotkey_ss58,
                netuid=self.netuid,
                amount=U64_MAX if self.amount_gamma == ALL else gamma_units(self.amount_gamma),
            )
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        origin, destination = await asyncio.gather(
            _deposit(substrate, self.netuid, self.origin_hotkey_ss58),
            _deposit(substrate, self.netuid, self.destination_hotkey_ss58),
        )
        blocks = _own_deposit_blocks(origin, signer_address, self.origin_hotkey_ss58)
        if not blocks and origin:
            balance = int(origin["balance"])
            amount = balance if self.amount_gamma == ALL else gamma_units(self.amount_gamma)
            rest = max(balance - amount, 0)
            if not balance:
                blocks.append(f"the gamma deposit at {self.origin_hotkey_ss58} holds no gamma")
            elif rest and min(amount, rest) < MIN_GAMMA_AMOUNT:
                blocks.append(
                    "a move takes the whole balance, or at least 0.1 gamma leaving at least as much"
                )
        if destination is not None:
            blocks += _own_deposit_blocks(destination, signer_address, self.destination_hotkey_ss58)
        else:
            blocks += await _new_deposit_blocks(
                substrate, self.destination_hotkey_ss58, signer_address
            )
        return blocks

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        origin, destination = await asyncio.gather(
            _deposit(substrate, self.netuid, self.origin_hotkey_ss58),
            _deposit(substrate, self.netuid, self.destination_hotkey_ss58),
        )
        if origin is None:
            return []
        warnings = []
        if self.amount_gamma == ALL or gamma_units(self.amount_gamma) >= int(origin["balance"]):
            warnings.append("moves the entire balance, which closes the origin deposit")
        if destination is None:
            return warnings
        if _cap(destination) != _cap(origin):
            caps = [
                "none" if cap is None else _gamma_text(cap)
                for cap in (_cap(destination), _cap(origin))
            ]
            warnings.append(
                f"the moved gamma falls under the destination deposit's daily spend cap, "
                f"{caps[0]}, instead of {caps[1]}"
            )
        return warnings

    def summary(self) -> str:
        amount = "all" if self.amount_gamma == ALL else self.amount_gamma
        return (
            f"move {amount} gamma on netuid {self.netuid} from {self.origin_hotkey_ss58}"
            f" to {self.destination_hotkey_ss58}"
        )

    def spend(self) -> Spend:
        # Hands the moved gamma's use to whoever holds the destination hotkey; not
        # TAO-denominated, so a spend cap must block it until raised.
        return UNBOUNDED


@register
@dataclass
class CloseGammaDeposit(Intent):
    """Close your empty gamma deposit, releasing its hotkey.

    Fails with ``GammaDepositNotEmpty`` while the deposit holds gamma.
    """

    op = "close_gamma_deposit"
    signer = "coldkey"
    wraps = (("SubtensorModule", "close_gamma_deposit"),)

    netuid: int = field(metadata={"help": "Subnet of the deposit."})
    hotkey_ss58: str = field(metadata={"help": "Hotkey of the empty deposit."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.close_gamma_deposit(hotkey=self.hotkey_ss58, netuid=self.netuid)
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        deposit = await _deposit(substrate, self.netuid, self.hotkey_ss58)
        blocks = _own_deposit_blocks(deposit, signer_address, self.hotkey_ss58)
        if not blocks and deposit and int(deposit["balance"]):
            blocks.append(
                f"the deposit holds {_gamma_text(int(deposit['balance']))}; move it out first, "
                "or close it once its hotkey has spent it"
            )
        return blocks

    def summary(self) -> str:
        return f"close the gamma deposit at {self.hotkey_ss58} on netuid {self.netuid}"


@register
@dataclass
class SetGammaDepositDailySpendCap(Intent):
    """Set or remove the daily spend cap of your gamma deposit.

    What the hotkey can spend refills linearly at the cap per 7200 blocks, up
    to the cap, and a cap of zero pauses the deposit. A changed cap keeps what
    the hotkey can spend now, up to the new cap. Set exactly one of
    ``daily_spend_cap_gamma`` or ``remove_cap``.
    """

    op = "set_gamma_deposit_daily_spend_cap"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_gamma_deposit_daily_spend_cap"),)

    netuid: int = field(metadata={"help": "Subnet of the deposit."})
    hotkey_ss58: str = field(metadata={"help": "Hotkey of the deposit."})
    daily_spend_cap_gamma: Optional[Money] = field(
        default=None,
        metadata={"help": "The cap, in gamma: 0 pauses the deposit, any other is at least 0.1."},
    )
    remove_cap: bool = field(default=False, metadata={"help": "Remove the cap."})

    def __post_init__(self):
        if (self.daily_spend_cap_gamma is None) != self.remove_cap:
            raise BittensorError(
                f"{self.op}: set exactly one of daily_spend_cap_gamma or remove_cap"
            )
        self.daily_spend_cap_gamma = _daily_spend_cap(self.daily_spend_cap_gamma)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_gamma_deposit_daily_spend_cap(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                daily_spend_cap=_cap_units(self.daily_spend_cap_gamma),
            )
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        deposit = await _deposit(substrate, self.netuid, self.hotkey_ss58)
        return _own_deposit_blocks(deposit, signer_address, self.hotkey_ss58)

    def summary(self) -> str:
        cap = (
            "no cap"
            if self.daily_spend_cap_gamma is None
            else f"a cap of {self.daily_spend_cap_gamma} gamma a day"
        )
        return f"set {cap} on the gamma deposit at {self.hotkey_ss58} on netuid {self.netuid}"


@register
@dataclass
class SpendGamma(Intent):
    """Spend gamma from the deposit at this hotkey, at a service on its subnet.

    Destroys ``amount_gamma`` of the deposit and adds it to what the hotkey has
    spent at the service at ``service_hotkey_ss58``, which credits it. The
    amount is at least 0.1 gamma unless it empties the deposit, and within its
    balance and what its daily spend cap allows. Signed by the hotkey, which
    pays no fee: the chain refuses an invalid spend before it enters a block.
    """

    op = "spend_gamma"
    signer = "hotkey"
    wraps = (("SubtensorModule", "spend_gamma"),)

    netuid: int = field(metadata={"help": "Subnet of the deposit and the service."})
    service_hotkey_ss58: str = field(metadata={"help": "Hotkey of the service spent at."})
    amount_gamma: Money = field(metadata={"help": "Gamma spent."})

    def __post_init__(self):
        self.amount_gamma = gamma_amount(self.amount_gamma)
        if not gamma_units(self.amount_gamma):
            raise BittensorError("spend_gamma: the amount must be above 0")

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.spend_gamma(
                service=self.service_hotkey_ss58,
                netuid=self.netuid,
                amount=gamma_units(self.amount_gamma),
            )
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        service, info = await asyncio.gather(
            substrate.query(*st.SubtensorModule.Services, [self.netuid, self.service_hotkey_ss58]),
            substrate.runtime_call(
                *GammaRuntimeApi.get_gamma_deposit, [self.netuid, signer_address]
            ),
        )
        if service is None:
            return [f"{self.service_hotkey_ss58} has no service on netuid {self.netuid}"]
        if info is None:
            return [f"there is no gamma deposit at {signer_address} on this subnet"]
        amount, balance = gamma_units(self.amount_gamma), int(info["deposit"]["balance"])
        if not balance:
            return ["the deposit holds no gamma"]
        if amount < MIN_GAMMA_AMOUNT and amount != balance:
            return ["a spend takes at least 0.1 gamma, or the deposit's whole balance"]
        if amount > int(info["spendable"]):
            return [f"the deposit can spend {_gamma_text(int(info['spendable']))} now"]
        return []

    def summary(self) -> str:
        return (
            f"spend {self.amount_gamma} gamma at {self.service_hotkey_ss58} on netuid {self.netuid}"
        )
