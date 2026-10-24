"""Gamma intents: minting, deposits, spenders and daily spend caps, and spending."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, ClassVar, Optional

from .._generated import calls
from .._generated.runtime_apis import GammaRuntimeApi
from ..hyperparams import U64_MAX
from ..result import BittensorError
from ._money import (
    ALL,
    UNBOUNDED,
    Money,
    Spend,
    alpha_amount,
    gamma_amount,
    gamma_units,
    tao_amount,
)
from .base import Intent
from .registry import register
from .staking import DEFAULT_RATE_TOLERANCE, _check_rate_tolerance, _staked_rao

# Most deposits one `spend_gamma` call spends from (`MAX_GAMMA_SPENDS`).
MAX_GAMMA_SPENDS = 512

RATE_TOLERANCE_HELP = "Maximum fall of the minted gamma below the quote, as a fraction (0.05 = 5%)."


async def _min_gamma_out(substrate, method: tuple, netuid: int, amount: int, tolerance: float):
    """The quoted gamma of a mint, less ``tolerance``; raises when gamma cannot be minted."""
    quote = await substrate.runtime_call(*method, [netuid, amount])
    if not quote:
        raise BittensorError(
            f"gamma cannot be minted on netuid {netuid} now: the subnet does not exist, "
            "minting or its subtoken is disabled, the TAO/USD price is missing or stale, the pool "
            "lacks liquidity, or the amount is below the minimum"
        )
    return int(int(quote[2]) * (1 - tolerance))


@register
@dataclass
class MintGamma(Intent):
    """Mint gamma with TAO into your deposit at an API-key hotkey.

    Buys the subnet's alpha with ``amount_tao``, burns it, and credits its value
    in gamma, one per US dollar, to the deposit at ``hotkey_ss58``, opening it
    when absent. The alpha is valued at the lower of the subnet's gamma price
    and its spot price before the buy.
    A new deposit's spender is the subnet owner, or a leased subnet's
    beneficiary, until you name another. Fails rather than minting less than
    the current quote less ``rate_tolerance``.
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

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def build(self, substrate, wallet: Any):
        min_out = await _min_gamma_out(
            substrate,
            GammaRuntimeApi.sim_mint_gamma,
            self.netuid,
            self.amount_tao.rao,
            self.rate_tolerance,
        )
        return await substrate.compose(
            calls.SubtensorModule.mint_gamma(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                amount=self.amount_tao.rao,
                min_amount_out=min_out,
            )
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

    Burns up to ``amount_alpha`` of your stake on ``origin_hotkey_ss58`` and
    credits its value in gamma to the deposit at ``hotkey_ss58``, valued as
    ``mint_gamma`` values bought alpha. The stake is destroyed, not moved.
    Fails rather than minting less than the current quote less
    ``rate_tolerance``.
    """

    op = "mint_gamma_with_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "mint_gamma_with_stake"),)

    netuid: int = field(metadata={"help": "Subnet of the stake burned."})
    hotkey_ss58: str = field(
        metadata={"help": "Hotkey of the deposit credited: an API key, not a staking hotkey."}
    )
    amount_alpha: Money = field(metadata={"help": "Alpha of stake burned."})
    origin_hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey the stake is on. Defaults to the wallet hotkey."},
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def build(self, substrate, wallet: Any):
        origin_hotkey = self.hotkey_address(wallet, self.origin_hotkey_ss58)
        # The chain burns at most the stake held, so quote no more than that.
        staked = await _staked_rao(
            substrate, wallet, origin_hotkey, self.netuid, action="burn into gamma"
        )
        min_out = await _min_gamma_out(
            substrate,
            GammaRuntimeApi.sim_mint_gamma_with_stake,
            self.netuid,
            min(self.amount_alpha.rao, staked),
            self.rate_tolerance,
        )
        return await substrate.compose(
            calls.SubtensorModule.mint_gamma_with_stake(
                origin_hotkey=origin_hotkey,
                destination_hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                alpha_amount=self.amount_alpha.rao,
                min_amount_out=min_out,
            )
        )

    def summary(self) -> str:
        return (
            f"burn {self.amount_alpha} of stake into gamma on netuid {self.netuid}"
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
    ``destination_hotkey_ss58``. A destination with no deposit is opened with
    the origin's spender and daily spend cap, and a move that empties the
    origin closes it. A move that opens a deposit needs at least 0.1 gamma
    (``MIN_GAMMA_AMOUNT``).
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
        self.amount_gamma = gamma_amount(self.amount_gamma, allow_all=True)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.move_gamma(
                origin_hotkey=self.origin_hotkey_ss58,
                destination_hotkey=self.destination_hotkey_ss58,
                netuid=self.netuid,
                amount=U64_MAX if self.amount_gamma == ALL else gamma_units(self.amount_gamma),
            )
        )

    def summary(self) -> str:
        amount = "all" if self.amount_gamma == ALL else self.amount_gamma
        return (
            f"move {amount} gamma on netuid {self.netuid} from {self.origin_hotkey_ss58}"
            f" to {self.destination_hotkey_ss58}"
        )


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

    def summary(self) -> str:
        return f"close the gamma deposit at {self.hotkey_ss58} on netuid {self.netuid}"


@register
@dataclass
class SetGammaDepositSpender(Intent):
    """Set who may spend your gamma deposit.

    Takes effect at once; a deposit opened by a move inherits it.
    """

    op = "set_gamma_deposit_spender"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_gamma_deposit_spender"),)

    netuid: int = field(metadata={"help": "Subnet of the deposit."})
    hotkey_ss58: str = field(metadata={"help": "Hotkey of the deposit."})
    spender_ss58: str = field(metadata={"help": "The account that spends the deposit."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_gamma_deposit_spender(
                hotkey=self.hotkey_ss58, netuid=self.netuid, spender=self.spender_ss58
            )
        )

    def summary(self) -> str:
        return (
            f"let {self.spender_ss58} spend the gamma deposit at {self.hotkey_ss58}"
            f" on netuid {self.netuid}"
        )


@register
@dataclass
class SetGammaDepositDailySpendCap(Intent):
    """Set the daily spend cap of your gamma deposit.

    What the spender can spend refills evenly by the cap every 7200 blocks, up
    to the cap, and a cap of zero pauses the deposit. Omitting
    ``daily_spend_cap_gamma`` removes the cap.
    """

    op = "set_gamma_deposit_daily_spend_cap"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_gamma_deposit_daily_spend_cap"),)

    netuid: int = field(metadata={"help": "Subnet of the deposit."})
    hotkey_ss58: str = field(metadata={"help": "Hotkey of the deposit."})
    daily_spend_cap_gamma: Optional[Money] = field(
        default=None, metadata={"help": "The cap, in gamma. Omit to remove it."}
    )

    def __post_init__(self):
        if self.daily_spend_cap_gamma is not None:
            self.daily_spend_cap_gamma = gamma_amount(self.daily_spend_cap_gamma)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_gamma_deposit_daily_spend_cap(
                hotkey=self.hotkey_ss58,
                netuid=self.netuid,
                daily_spend_cap=None
                if self.daily_spend_cap_gamma is None
                else gamma_units(self.daily_spend_cap_gamma),
            )
        )

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
    """Spend gamma from the deposits you are the spender of.

    Spends, in order, up to each entry's ``amount_gamma`` from the deposit at
    its ``hotkey_ss58``, within the deposit's balance and daily spend cap.
    Entries whose hotkey has no deposit, or whose deposit you are not the
    spender of, are skipped. Spent gamma is destroyed. Sign as the spender, or
    as its ``GammaSpend`` proxy.
    """

    op = "spend_gamma"
    signer = "coldkey"
    wraps = (("SubtensorModule", "spend_gamma"),)

    netuid: int = field(metadata={"help": "Subnet of the deposits."})
    spends: list[dict] = field(
        metadata={
            "help": f"One to {MAX_GAMMA_SPENDS} objects of hotkey_ss58 and amount_gamma, "
            "spent in order."
        }
    )

    def __post_init__(self):
        if not 1 <= len(self.spends) <= MAX_GAMMA_SPENDS:
            raise BittensorError(f"spend_gamma takes 1 to {MAX_GAMMA_SPENDS} spends")
        self.spends = [
            {
                "hotkey_ss58": str(spend["hotkey_ss58"]),
                "amount_gamma": gamma_amount(spend["amount_gamma"]),
            }
            for spend in self.spends
        ]

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.spend_gamma(
                netuid=self.netuid,
                spends=[
                    (spend["hotkey_ss58"], gamma_units(spend["amount_gamma"]))
                    for spend in self.spends
                ],
            )
        )

    def summary(self) -> str:
        return f"spend gamma from {len(self.spends)} deposits on netuid {self.netuid}"
