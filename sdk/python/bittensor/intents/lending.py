"""Synthetic-mint shorts: mint subnet alpha, sell it, and lock the proceeds."""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Any, Optional, cast

from .._generated import calls
from .._generated import storage as st
from .._generated.runtime_apis import LendingRuntimeApi, SwapRuntimeApi
from ..balance import Balance
from ..result import BittensorError, ChainError, chain_error_from_dispatch
from ..settings import ALPHA_SYMBOL, TAO_SYMBOL
from ._money import Money, Spend, alpha_amount, tao_amount
from .base import Intent, IntentPreflight
from .registry import register
from .staking import _check_rate_tolerance

# Default tolerance on the sale floor and the buy-back ceiling.
DEFAULT_SHORT_TOLERANCE = 0.005
RATE_TOLERANCE_HELP = (
    "Tolerance applied to the quote when the floor or ceiling is omitted, as a fraction "
    "(0.005 = 0.5%)."
)
OUTCOME_OPS = frozenset({"open_short", "increase_short", "close_short"})


def _ok(raw: Any) -> dict:
    """Unwrap a lending runtime-API ``Result``, raising the chain error it carries."""
    if not isinstance(raw, dict):
        raise BittensorError("lending quote unavailable; use a node with the shorts runtime")
    if "Err" in raw:
        raise chain_error_from_dispatch(raw["Err"])
    result = raw.get("Ok", raw)
    if not isinstance(result, dict):
        raise BittensorError("invalid lending quote response")
    return result


def _check_netuid(netuid: int) -> None:
    if not 1 <= netuid <= 65535:
        raise BittensorError("shorts require a subnet netuid from 1 to 65535")


def _optional_tao(value: Optional[Money], label: str) -> Optional[Balance]:
    if value is None:
        return None
    amount = cast(Balance, tao_amount(value))
    if amount.rao <= 0:
        raise BittensorError(f"{label} must be greater than zero")
    return amount


def _margin(rao: int, tolerance: float) -> int:
    numerator, denominator = Decimal(str(tolerance)).as_integer_ratio()
    return (rao * numerator + denominator - 1) // denominator


def floor_rao(rao: int, tolerance: float) -> int:
    """``rao`` less ``tolerance``, rounded down."""
    return max(rao - _margin(rao, tolerance), 0)


def ceiling_rao(rao: int, tolerance: float) -> int:
    """``rao`` plus ``tolerance``, rounded up."""
    return rao + _margin(rao, tolerance)


async def _position(substrate, owner: str, netuid: int) -> dict:
    position = await substrate.query(*st.Lending.Positions, [owner, netuid])
    if not isinstance(position, dict):
        raise BittensorError(f"no short position on netuid {netuid}")
    return position


async def _sale_floor(substrate, netuid: int, alpha_rao: int, tolerance: float) -> int:
    """Simulated TAO out for selling ``alpha_rao``, less ``tolerance``."""
    sim = await substrate.runtime_call(
        *SwapRuntimeApi.sim_swap_alpha_for_tao, {"netuid": netuid, "alpha": alpha_rao}
    )
    floor = floor_rao(int(sim["tao_amount"]), tolerance)
    if floor <= 0:
        raise BittensorError("the pool quotes no TAO for this sale; pass min_tao_out")
    return floor


def _fmt_outcome_amount(rao: int, netuid: int) -> str:
    """τ / α_N money text, matching CLI result cards."""
    amount = rao / 1_000_000_000
    if netuid == 0:
        return f"{TAO_SYMBOL}{amount:,.9f}"
    return f"{amount:,.9f}{ALPHA_SYMBOL}_{netuid}"


def _fmt(amount: Balance) -> str:
    return _fmt_outcome_amount(amount.rao, amount.netuid)


@register
@dataclass
class OpenShort(Intent):
    """Open a synthetic short: mint subnet alpha, sell it, and lock the proceeds.

    Mints ``mint_alpha`` into the position's custody and sells it into the
    pool. The sale proceeds stay locked in custody beside ``collateral`` TAO
    and a refundable deposit (``SynthParams.deposit``, 0.1 TAO by default)
    taken from the caller. Collateral plus proceeds must cover the position's
    pro-rata share of buying the whole subnet book back after a stressed pump;
    ``lending_open_quote`` returns the smallest collateral that does.

    Interest accrues in TAO on ``minted × EMA`` at a flat APR (20% by
    default) and is charged from collateral. A position whose median-TWAP
    equity turns negative deleverages through paced protocol buy-backs. There
    is no term, expiry or spot liquidation. Alpha minted while the subnet is in
    the prune zone settles at its own sale proceeds on deregistration.

    ``min_tao_out`` floors the sale and the chain refuses zero. Omit it to use
    the simulated sale less ``rate_tolerance``. One position per coldkey and
    subnet; grow it with ``increase_short``.
    """

    op = "open_short"
    signer = "coldkey"
    wraps = (("Lending", "open_short"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet whose alpha is minted and sold."})
    mint_alpha: Money = field(metadata={"help": "Alpha to mint and sell."})
    collateral: Money = field(metadata={"help": "TAO collateral locked beside the proceeds."})
    min_tao_out: Optional[Money] = field(
        default=None,
        metadata={"help": "Least TAO the sale may return. Omit to quote it."},
    )
    rate_tolerance: float = field(
        default=DEFAULT_SHORT_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.mint_alpha = alpha_amount(self.mint_alpha, self.netuid)
        if cast(Balance, self.mint_alpha).rao <= 0:
            raise BittensorError("mint_alpha must be greater than zero")
        self.collateral = tao_amount(self.collateral)
        self.min_tao_out = _optional_tao(self.min_tao_out, "min_tao_out")
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def build(self, substrate, wallet: Any):
        mint = cast(Balance, self.mint_alpha).rao
        floor = (
            cast(Balance, self.min_tao_out).rao
            if self.min_tao_out is not None
            else await _sale_floor(substrate, self.netuid, mint, self.rate_tolerance)
        )
        return await substrate.compose(
            calls.Lending.open_short(
                netuid=self.netuid,
                mint_alpha=mint,
                collateral=cast(Balance, self.collateral).rao,
                min_tao_out=floor,
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        """Quote the open and stop when collateral misses the coverage minimum."""
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        try:
            quote = _ok(
                await substrate.runtime_call(
                    *LendingRuntimeApi.quote_open_short,
                    [self.netuid, cast(Balance, self.mint_alpha).rao],
                )
            )
        except BittensorError as error:
            name = error.name if isinstance(error, ChainError) else None
            preview.blocks.append(f"the chain refuses this short: {name or error}")
            return preview
        required = Balance.from_rao(int(quote["required_collateral"]))
        collateral = cast(Balance, self.collateral)
        preview.facts_title = "Quote"
        preview.facts = [
            ("sale proceeds", _fmt(Balance.from_rao(int(quote["proceeds"])))),
            ("required collateral", _fmt(required)),
            ("deposit", _fmt(Balance.from_rao(int(quote["deposit"])))),
            ("APR", f"{int(quote['apr_bps']) / 100:g}%"),
            ("annual interest", _fmt(Balance.from_rao(int(quote["annual_interest"])))),
            *preview.facts,
        ]
        if collateral.rao < required.rao:
            preview.blocks.append(
                f"collateral {_fmt(collateral)} is below the required {_fmt(required)}"
            )
        if quote.get("in_prune_zone"):
            preview.warnings.append(
                "the subnet is in the prune zone: this alpha settles at its own sale "
                "proceeds if the subnet deregisters"
            )
        return preview

    def summary(self) -> str:
        return (
            f"short {_fmt(cast(Balance, self.mint_alpha))} on netuid {self.netuid} "
            f"with {_fmt(cast(Balance, self.collateral))} collateral"
        )

    def spend(self) -> Spend:
        return cast(Balance, self.collateral)


@register
@dataclass
class IncreaseShort(Intent):
    """Grow an open short: mint and sell more alpha, add collateral, or both.

    The combined position must still cover its whole-book share. Refused while
    the position deleverages, while the subnet's dilution breaker is on, or
    when the block's net mint limit is spent. ``min_tao_out`` floors the sale
    of ``extra_mint``; omit it to use the simulated sale less
    ``rate_tolerance``.
    """

    op = "increase_short"
    signer = "coldkey"
    wraps = (("Lending", "increase_short"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet of the short."})
    extra_mint: Money = field(default=0, metadata={"help": "More alpha to mint and sell."})
    extra_collateral: Money = field(default=0, metadata={"help": "More TAO collateral to lock."})
    min_tao_out: Optional[Money] = field(
        default=None,
        metadata={"help": "Least TAO the extra sale may return. Omit to quote it."},
    )
    rate_tolerance: float = field(
        default=DEFAULT_SHORT_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.extra_mint = alpha_amount(self.extra_mint, self.netuid)
        self.extra_collateral = tao_amount(self.extra_collateral)
        if (
            cast(Balance, self.extra_mint).rao <= 0
            and cast(Balance, self.extra_collateral).rao <= 0
        ):
            raise BittensorError("increase needs extra_mint or extra_collateral")
        self.min_tao_out = _optional_tao(self.min_tao_out, "min_tao_out")
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def build(self, substrate, wallet: Any):
        mint = cast(Balance, self.extra_mint).rao
        floor = 0
        if self.min_tao_out is not None:
            floor = cast(Balance, self.min_tao_out).rao
        elif mint > 0:
            floor = await _sale_floor(substrate, self.netuid, mint, self.rate_tolerance)
        return await substrate.compose(
            calls.Lending.increase_short(
                netuid=self.netuid,
                extra_mint=mint,
                extra_collateral=cast(Balance, self.extra_collateral).rao,
                min_tao_out=floor,
            )
        )

    def summary(self) -> str:
        return (
            f"increase short on netuid {self.netuid}: mint "
            f"{_fmt(cast(Balance, self.extra_mint))}, add "
            f"{_fmt(cast(Balance, self.extra_collateral))} collateral"
        )

    def spend(self) -> Spend:
        return cast(Balance, self.extra_collateral)


@register
@dataclass
class AddShortCollateral(Intent):
    """Add TAO collateral to an open short.

    Works while lending is disabled and while the position deleverages.
    """

    op = "add_short_collateral"
    signer = "coldkey"
    wraps = (("Lending", "add_collateral"),)

    netuid: int = field(metadata={"help": "Subnet of the short."})
    amount: Money = field(metadata={"help": "TAO to add."})

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.amount = _optional_tao(self.amount, "amount")

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Lending.add_collateral(netuid=self.netuid, tao=cast(Balance, self.amount).rao)
        )

    def summary(self) -> str:
        return f"add {_fmt(cast(Balance, self.amount))} collateral to short on netuid {self.netuid}"

    def spend(self) -> Spend:
        return cast(Balance, self.amount)


@register
@dataclass
class WithdrawShortCollateral(Intent):
    """Withdraw TAO collateral from an open short.

    Refused unless the remaining funds still cover the position's whole-book
    share plus ``SynthParams.withdraw_buffer_days`` of interest (7 by default).
    """

    op = "withdraw_short_collateral"
    signer = "coldkey"
    wraps = (("Lending", "withdraw_collateral"),)

    netuid: int = field(metadata={"help": "Subnet of the short."})
    amount: Money = field(metadata={"help": "TAO to withdraw."})

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.amount = _optional_tao(self.amount, "amount")

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Lending.withdraw_collateral(
                netuid=self.netuid, tao=cast(Balance, self.amount).rao
            )
        )

    def summary(self) -> str:
        return (
            f"withdraw {_fmt(cast(Balance, self.amount))} collateral from short on "
            f"netuid {self.netuid}"
        )

    def spend(self) -> Spend:
        return None


@register
@dataclass
class CloseShort(Intent):
    """Buy back and burn short alpha, paying from proceeds then collateral.

    ``alpha`` defaults to the whole position. Closing the whole position
    refunds the remaining funds and the deposit to the owner; a partial close
    refunds nothing. ``max_tao_in`` caps the buy-back cost; omit it to use the
    runtime close quote plus ``rate_tolerance``. The buy-back is paid from the
    position's custody, never from the wallet. Works while lending is
    disabled.
    """

    op = "close_short"
    signer = "coldkey"
    wraps = (("Lending", "close_short"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet of the short."})
    alpha: Optional[Money] = field(
        default=None,
        metadata={"help": "Alpha to buy back and burn. Omit to close the whole position."},
    )
    max_tao_in: Optional[Money] = field(
        default=None,
        metadata={"help": "Most TAO the buy-back may cost. Omit to quote it."},
    )
    rate_tolerance: float = field(
        default=DEFAULT_SHORT_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        if self.alpha is not None:
            self.alpha = alpha_amount(self.alpha, self.netuid)
            if cast(Balance, self.alpha).rao <= 0:
                raise BittensorError("alpha must be greater than zero")
        self.max_tao_in = _optional_tao(self.max_tao_in, "max_tao_in")
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def build(self, substrate, wallet: Any):
        owner = self.coldkey_address(wallet)
        position = await _position(substrate, owner, self.netuid)
        alpha = cast(Balance, self.alpha).rao if self.alpha is not None else int(position["minted"])
        if self.max_tao_in is not None:
            ceiling = cast(Balance, self.max_tao_in).rao
        else:
            quote = _ok(
                await substrate.runtime_call(
                    *LendingRuntimeApi.quote_close_short, [owner, self.netuid, alpha]
                )
            )
            ceiling = ceiling_rao(int(quote["tao_in"]), self.rate_tolerance)
        return await substrate.compose(
            calls.Lending.close_short(netuid=self.netuid, alpha=alpha, max_tao_in=ceiling)
        )

    def summary(self) -> str:
        if self.alpha is None:
            return f"close short on netuid {self.netuid}"
        return f"buy back {_fmt(cast(Balance, self.alpha))} of short on netuid {self.netuid}"

    def spend(self) -> Spend:
        return None


@register
@dataclass
class PokeShort(Intent):
    """Run a permissionless health check on someone's short.

    Charges accrued interest, starts deleveraging a position whose
    median-TWAP equity is negative, or runs one deleverage step. Anyone may
    submit it.
    """

    op = "poke_short"
    signer = "coldkey"
    wraps = (("Lending", "poke"),)

    owner_ss58: str = field(metadata={"help": "Owner of the short to check."})
    netuid: int = field(metadata={"help": "Subnet of the short."})

    def __post_init__(self):
        _check_netuid(self.netuid)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Lending.poke(who=self.owner_ss58, netuid=self.netuid))

    def summary(self) -> str:
        return f"poke short of {self.owner_ss58} on netuid {self.netuid}"

    def spend(self) -> Spend:
        return None


def _find_lending_event(events: list, *names: str) -> Optional[dict]:
    for entry in events or []:
        record = entry.value if hasattr(entry, "value") else entry
        event = record.get("event", record) if isinstance(record, dict) else {}
        if not isinstance(event, dict) or event.get("module_id") != "Lending":
            continue
        if event.get("event_id") in names:
            attrs = event.get("attributes")
            return attrs if isinstance(attrs, dict) else None
    return None


def _amounts(attrs: dict, units: dict[str, int]) -> dict[str, Any]:
    data: dict[str, Any] = {}
    for key, unit in units.items():
        rao = int(attrs.get(key) or 0)
        data[key] = _fmt_outcome_amount(rao, unit)
        data[f"{key}_rao"] = rao
    return data


def lending_outcome_data(events: list, *, op: Optional[str] = None) -> dict[str, Any]:
    """Decoded short receipt fields for ``ExtrinsicResult.data``.

    Human strings plus ``*_rao`` ints, matching the CLI result card.
    """
    if op in (None, "open_short", "increase_short"):
        attrs = _find_lending_event(events, "ShortOpened", "ShortIncreased")
        if attrs is not None:
            net = int(attrs.get("netuid") or 0)
            return _amounts(attrs, {"minted": net, "proceeds": 0, "collateral": 0})
    if op in (None, "close_short"):
        attrs = _find_lending_event(events, "ShortClosed")
        if attrs is not None:
            net = int(attrs.get("netuid") or 0)
            return _amounts(attrs, {"burned": net, "tao_spent": 0, "refund": 0})
    return {}
