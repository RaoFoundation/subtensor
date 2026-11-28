"""Lending intents: short positions and the root parameter setters.

A short mints alpha on a subnet, sells it into the pool, and holds the TAO
proceeds with the holder's collateral in the position's custody account until
the alpha is bought back. Longs are pending their specification and have no
intent here: their calls are reachable only through raw ``btcli call``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, ClassVar, Optional

from .._generated import calls
from .._generated import storage as st
from .._generated.runtime_apis import LendingRuntimeApi
from ..balance import Balance
from ..hyperparams import U64_MAX
from ..result import BittensorError, chain_error_from_dispatch
from ._money import ALL, UNBOUNDED, Money, Spend, alpha_amount, tao_amount
from .base import Intent, IntentPreflight
from .registry import register
from .staking import DEFAULT_RATE_TOLERANCE, _check_rate_tolerance

SHORT_OPEN_TOLERANCE_HELP = (
    "Maximum fall of the sale's TAO proceeds below the quote, as a fraction (0.05 = 5%)."
)
SHORT_CLOSE_TOLERANCE_HELP = (
    "Maximum rise of the buy-back's TAO cost above the quote, as a fraction (0.05 = 5%)."
)

# The fields of each parameter group, in the order the chain declares them. Amounts
# (``min_pool_tao``, ``min_notional``, ``deposit``) are in rao.
PARAM_GROUPS: dict[str, tuple[str, ...]] = {
    "lending": (
        "apr_bps",
        "rate_k",
        "rate_cap_bps",
        "rate_delay_blocks",
        "charge_interval_blocks",
        "min_pool_tao",
        "min_notional",
        "deposit",
        "twap_samples",
        "twap_interval_blocks",
    ),
    "short": ("stress_bps", "open_limit_bps"),
    "long": ("stress_bps", "stress_floor_bps", "lift_cap_bps", "open_limit_bps"),
    "dereg": (
        "zone_bps",
        "pool_tao_ema_blocks",
        "rank_window_days",
        "max_long_positions_per_subnet",
    ),
}

_PENDING = {
    "lending": st.Lending.PendingLendingParams,
    "short": st.Lending.PendingShortParams,
    "long": st.Lending.PendingLongParams,
    "dereg": st.Lending.PendingDeregParams,
}

CHANGES_HELP = (
    "Fields to change, by name; every other field keeps its value. The group is replaced "
    "whole from the next block, and a change the chain's bounds refuse changes nothing."
)


async def _quote(substrate, method: tuple, params: list) -> dict:
    """A short quote; raises the chain error the call would fail with."""
    quote = await substrate.runtime_call(*method, params)
    if quote is None:
        raise BittensorError("this chain has no lending runtime API")
    if "Err" in quote:
        raise chain_error_from_dispatch(quote["Err"])
    return quote["Ok"]


def _min_out(proceeds: int, tolerance: float) -> int:
    return int(proceeds * (1 - tolerance))


def _max_in(cost: int, tolerance: float) -> int:
    return min(U64_MAX, -int(-cost * (1 + tolerance) // 1))


def _tao(rao: Any) -> Balance:
    return Balance.from_rao(int(rao))


def _grow_preflight(
    intent, preview: IntentPreflight, quote: dict, *, opening: bool
) -> IntentPreflight:
    """A short open's or increase's preview: its quote and the collateral it must bring."""
    proceeds = int(quote["proceeds"])
    minimum = _min_out(proceeds, intent.rate_tolerance)
    need = int(quote["min_collateral"])
    if intent.collateral_tao.rao < need:
        preview.blocks.append(
            f"the short needs at least {_tao(need)} of collateral, and this brings "
            f"{intent.collateral_tao}"
        )
    preview.facts = [
        ("proceeds", f"~{_tao(proceeds)}"),
        ("minimum proceeds", str(_tao(minimum))),
        ("minimum collateral", str(_tao(need))),
        ("coverage need", str(_tao(quote["coverage_need"]))),
        (
            ("deposit", str(_tao(quote["deposit"])))
            if opening
            else ("interest settled", str(_tao(quote["interest"])))
        ),
        *preview.facts,
    ]
    preview.facts_title = "Quote"
    preview.preview = {
        "proceeds_tao": format(_tao(proceeds).decimal, "f"),
        "minimum_proceeds_tao": format(_tao(minimum).decimal, "f"),
        "min_collateral_tao": format(_tao(need).decimal, "f"),
        "coverage_need_tao": format(_tao(quote["coverage_need"]).decimal, "f"),
        "deposit_tao": format(_tao(quote["deposit"]).decimal, "f"),
        "interest_tao": format(_tao(quote["interest"]).decimal, "f"),
    }
    return preview


@register
@dataclass
class OpenShort(Intent):
    """Open a short on a subnet's alpha.

    Mints ``amount_alpha`` of alpha, sells it into the subnet's pool, and holds
    the TAO proceeds in the position's custody account with ``collateral_tao``
    from your free balance. The chain also takes a refundable deposit on top
    of the collateral. Collateral and proceeds together must reach the
    position's coverage need: the ``quote_open_short`` read reports the least
    collateral that does. You hold at most one short per subnet; add to it
    with ``increase_short``. Interest accrues on the proceeds and is charged
    to the collateral.
    Fails rather than selling for less than the current quote less
    ``rate_tolerance``. A plan's ``preview`` holds the quote's
    ``proceeds_tao``, ``minimum_proceeds_tao``, ``min_collateral_tao``,
    ``coverage_need_tao``, ``deposit_tao`` and ``interest_tao``.
    """

    op = "open_short"
    signer = "coldkey"
    wraps = (("Lending", "open_short"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet whose alpha is shorted."})
    amount_alpha: Money = field(metadata={"help": "Alpha minted and sold."})
    collateral_tao: Money = field(
        metadata={"help": "TAO moved from your free balance into the position as collateral."}
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": SHORT_OPEN_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid)
        self.collateral_tao = tao_amount(self.collateral_tao)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    def _quote(self, substrate):
        return _quote(
            substrate, LendingRuntimeApi.quote_open_short, [self.netuid, self.amount_alpha.rao]
        )

    async def build(self, substrate, wallet: Any):
        quote = await self._quote(substrate)
        return await substrate.compose(
            calls.Lending.open_short(
                netuid=self.netuid,
                q=self.amount_alpha.rao,
                collateral=self.collateral_tao.rao,
                min_tao_out=_min_out(int(quote["proceeds"]), self.rate_tolerance),
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        return _grow_preflight(self, preview, await self._quote(substrate), opening=True)

    def summary(self) -> str:
        return (
            f"open a short of {self.amount_alpha} on netuid {self.netuid} with "
            f"{self.collateral_tao} of collateral"
            f" (fails if the proceeds fall >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        # The collateral plus the chain's refundable deposit, which only a read knows.
        return UNBOUNDED


@register
@dataclass
class IncreaseShort(Intent):
    """Add to your open short on a subnet.

    Mints and sells ``amount_alpha`` more alpha into the position, with
    ``collateral_tao`` more collateral from your free balance (0 when the
    position already covers the larger size). Interest due is settled from the
    position's collateral first. The ``quote_increase_short`` read reports the
    least collateral the larger position needs.
    Fails rather than selling for less than the current quote less
    ``rate_tolerance``. A plan's ``preview`` holds the same quote keys as
    ``open_short``.
    """

    op = "increase_short"
    signer = "coldkey"
    wraps = (("Lending", "increase_short"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet of the short."})
    amount_alpha: Money = field(metadata={"help": "Alpha minted and sold on top of the short."})
    collateral_tao: Money = field(
        default=0,
        metadata={"help": "TAO moved from your free balance into the position as collateral."},
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": SHORT_OPEN_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid)
        self.collateral_tao = tao_amount(self.collateral_tao)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    def _quote(self, substrate, holder: str):
        return _quote(
            substrate,
            LendingRuntimeApi.quote_increase_short,
            [holder, self.netuid, self.amount_alpha.rao],
        )

    async def build(self, substrate, wallet: Any):
        quote = await self._quote(substrate, self.coldkey_address(wallet))
        return await substrate.compose(
            calls.Lending.increase_short(
                netuid=self.netuid,
                q=self.amount_alpha.rao,
                collateral=self.collateral_tao.rao,
                min_tao_out=_min_out(int(quote["proceeds"]), self.rate_tolerance),
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        quote = await self._quote(substrate, dispatch_origin)
        return _grow_preflight(self, preview, quote, opening=False)

    def summary(self) -> str:
        return (
            f"increase the short on netuid {self.netuid} by {self.amount_alpha} with "
            f"{self.collateral_tao} of collateral"
            f" (fails if the proceeds fall >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        return self.collateral_tao


@register
@dataclass
class CloseShort(Intent):
    """Buy back some or all of your short on a subnet.

    Buys ``amount_alpha`` of alpha from the pool and retires it, paying from
    the position's proceeds and then its collateral. ``all`` closes the whole
    position, which returns the remaining collateral and the deposit to your
    free balance. Interest due is settled from the collateral first.
    Fails rather than paying more than the current quote plus
    ``rate_tolerance``. A plan's ``preview`` holds the quote's ``alpha``,
    ``cost_tao``, ``maximum_cost_tao``, ``from_proceeds_tao``,
    ``from_collateral_tao``, ``interest_tao`` and whether the close is
    ``full``.
    """

    op = "close_short"
    signer = "coldkey"
    wraps = (("Lending", "close_short"),)
    mev_shield_default = True
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_alpha",)

    netuid: int = field(metadata={"help": "Subnet of the short."})
    amount_alpha: Money = field(
        default=ALL,
        metadata={"help": "Alpha bought back, or ``all`` to close the whole short."},
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": SHORT_CLOSE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid, allow_all=True)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    def _alpha(self) -> int:
        # The chain caps the request at the position's size.
        return U64_MAX if self.amount_alpha == ALL else self.amount_alpha.rao

    def _quote(self, substrate, holder: str):
        return _quote(
            substrate, LendingRuntimeApi.quote_close_short, [holder, self.netuid, self._alpha()]
        )

    async def build(self, substrate, wallet: Any):
        quote = await self._quote(substrate, self.coldkey_address(wallet))
        return await substrate.compose(
            calls.Lending.close_short(
                netuid=self.netuid,
                alpha=self._alpha(),
                max_tao_in=_max_in(int(quote["cost"]), self.rate_tolerance),
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        quote = await self._quote(substrate, dispatch_origin)
        cost = int(quote["cost"])
        maximum = _max_in(cost, self.rate_tolerance)
        alpha = Balance.from_rao(int(quote["alpha"]), self.netuid)
        preview.facts = [
            ("alpha bought back", str(alpha)),
            ("cost", f"~{_tao(cost)}"),
            ("maximum cost", str(_tao(maximum))),
            ("from proceeds", str(_tao(quote["from_proceeds"]))),
            ("from collateral", str(_tao(quote["from_collateral"]))),
            ("interest settled", str(_tao(quote["interest"]))),
            ("closes the position", "yes" if quote["full"] else "no"),
            *preview.facts,
        ]
        preview.facts_title = "Quote"
        preview.preview = {
            "alpha": format(alpha.decimal, "f"),
            "cost_tao": format(_tao(cost).decimal, "f"),
            "maximum_cost_tao": format(_tao(maximum).decimal, "f"),
            "from_proceeds_tao": format(_tao(quote["from_proceeds"]).decimal, "f"),
            "from_collateral_tao": format(_tao(quote["from_collateral"]).decimal, "f"),
            "interest_tao": format(_tao(quote["interest"]).decimal, "f"),
            "full": bool(quote["full"]),
        }
        return preview

    def summary(self) -> str:
        amount = (
            "the whole short" if self.amount_alpha == ALL else f"{self.amount_alpha} of the short"
        )
        return (
            f"buy back {amount} on netuid {self.netuid}"
            f" (fails if the cost rises >{self.rate_tolerance:.2%})"
        )


@register
@dataclass
class ShortAddCollateral(Intent):
    """Add collateral to your short on a subnet.

    Moves ``amount_tao`` from your free balance into the position's custody.
    More collateral raises the position's equity and the interest it can
    cover. Collateral leaves a position only when the short is closed: there
    is no withdrawal.
    """

    op = "short_add_collateral"
    signer = "coldkey"
    wraps = (("Lending", "add_collateral"),)

    netuid: int = field(metadata={"help": "Subnet of the short."})
    amount_tao: Money = field(metadata={"help": "TAO added to the short's collateral."})

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Lending.add_collateral(netuid=self.netuid, amount=self.amount_tao.rao)
        )

    def summary(self) -> str:
        return f"add {self.amount_tao} of collateral to the short on netuid {self.netuid}"

    def spend(self) -> Spend:
        return self.amount_tao


@register
@dataclass
class SetLendingEnabled(Intent):
    """Turn lending on or off (root only).

    While off, no short may be opened or increased; open positions still
    accrue interest and can be closed. Turning it on re-seeds every subnet's
    rank price ring from the emission moving price. Verify with the
    ``lending_params`` read.
    """

    op = "set_lending_enabled"
    signer = "coldkey"
    origin = "root"
    verify = "lending_params"
    wraps = (("Lending", "set_enabled"), ("Sudo", "sudo"))

    enabled: bool = field(metadata={"help": "True to turn lending on, false to turn it off."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Lending.set_enabled(enabled=self.enabled))

    def summary(self) -> str:
        return f"turn lending {'on' if self.enabled else 'off'}"

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class SetLendingLongsEnabled(Intent):
    """Turn lending longs on or off (root only).

    Longs are pending their specification and stay off on mainnet. Verify with
    the ``lending_params`` read.
    """

    op = "set_lending_longs_enabled"
    signer = "coldkey"
    origin = "root"
    verify = "lending_params"
    wraps = (("Lending", "set_longs_enabled"), ("Sudo", "sudo"))

    enabled: bool = field(metadata={"help": "True to turn longs on, false to turn them off."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Lending.set_longs_enabled(enabled=self.enabled))

    def summary(self) -> str:
        return f"turn lending longs {'on' if self.enabled else 'off'}"

    def affects_all_subnets(self) -> bool:
        return True


def _checked_changes(group: str, changes: Optional[dict]) -> dict[str, int]:
    """``changes`` as integer values of the group's own fields."""
    if not changes:
        raise BittensorError(f"name at least one {group} parameter to change")
    fields = PARAM_GROUPS[group]
    unknown = sorted(set(changes) - set(fields))
    if unknown:
        raise BittensorError(
            f"unknown {group} parameter(s) {', '.join(unknown)}; the group has {', '.join(fields)}"
        )
    checked = {}
    for name, value in changes.items():
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise BittensorError(f"{name} must be a non-negative integer, got {value!r}")
        checked[name] = value
    return checked


async def _merged_params(substrate, group: str, changes: dict[str, int]) -> dict[str, int]:
    """The group the setter submits: ``changes`` over the value the next block would hold.

    That is the group already queued when there is one, so two setters compose, and the
    live group otherwise. A queued lending group is ``(params, apply_at)``: a tightening
    waiting out its delay, which the next call replaces, so the fields this call leaves
    alone keep the queued values.
    """
    current = await substrate.query(*_PENDING[group])
    if current is not None and group == "lending":
        current = current[0]
    if current is None:
        view = await substrate.runtime_call(*LendingRuntimeApi.params, [])
        if view is None:
            raise BittensorError("this chain has no lending runtime API")
        current = view[group]
    merged = {name: int(current[name]) for name in PARAM_GROUPS[group]}
    return merged | changes


def _changes_text(changes: dict[str, int]) -> str:
    return ", ".join(f"{name}={value}" for name, value in changes.items())


@register
@dataclass
class SetLendingParams(Intent):
    """Change the lending parameters shared by shorts and longs (root only).

    ``changes`` names the fields to change among ``apr_bps``, ``rate_k``,
    ``rate_cap_bps``, ``rate_delay_blocks``, ``charge_interval_blocks``,
    ``min_pool_tao``, ``min_notional``, ``deposit``, ``twap_samples`` and
    ``twap_interval_blocks``; amounts are in rao. The rest keep their value.
    ``apr_bps``, ``rate_k`` and ``rate_cap_bps`` set the interest rate
    ``min(rate_cap_bps, apr_bps * max(1, u^rate_k))`` a year, where ``u`` is a
    position's debt over its holdings at the slow price. The chain checks the
    whole group against its bounds and refuses it whole on any breach; the cap
    may not be below the base APR, in the live group or in the new one.
    A change that tightens the rate (a higher ``apr_bps`` or ``rate_cap_bps``, any
    change of ``rate_k``, ``twap_samples`` or ``twap_interval_blocks``, or a shorter
    ``rate_delay_blocks``) waits the ``rate_delay_blocks`` in force at the call
    (3 days at launch); the read's ``pending_lending_params`` shows it until then.
    Any other change, a longer delay included, applies from the next block. Each
    call replaces the waiting group: fields left out of ``changes`` keep their
    waiting value, and the delay starts again if that value still tightens.
    Verify with the ``lending_params`` read.
    """

    op = "set_lending_params"
    signer = "coldkey"
    origin = "root"
    verify = "lending_params"
    wraps = (("Lending", "set_lending_params"), ("Sudo", "sudo"))

    changes: dict = field(metadata={"help": CHANGES_HELP})

    def __post_init__(self):
        self.changes = _checked_changes("lending", self.changes)

    async def build(self, substrate, wallet: Any):
        params = await _merged_params(substrate, "lending", self.changes)
        return await substrate.compose(calls.Lending.set_lending_params(params=params))

    def summary(self) -> str:
        return f"set lending parameters {_changes_text(self.changes)}"

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class SetLendingShortParams(Intent):
    """Change the short-side lending parameters (root only).

    ``changes`` names the fields to change among ``stress_bps`` and
    ``open_limit_bps``; the rest keep their value.
    The chain refuses the group whole on any bound breach and applies it from
    the next block. Verify with the ``lending_params`` read.
    """

    op = "set_lending_short_params"
    signer = "coldkey"
    origin = "root"
    verify = "lending_params"
    wraps = (("Lending", "set_short_params"), ("Sudo", "sudo"))

    changes: dict = field(metadata={"help": CHANGES_HELP})

    def __post_init__(self):
        self.changes = _checked_changes("short", self.changes)

    async def build(self, substrate, wallet: Any):
        params = await _merged_params(substrate, "short", self.changes)
        return await substrate.compose(calls.Lending.set_short_params(params=params))

    def summary(self) -> str:
        return f"set short lending parameters {_changes_text(self.changes)}"

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class SetLendingLongParams(Intent):
    """Change the long-side lending parameters (root only).

    ``changes`` names the fields to change among ``stress_bps``,
    ``stress_floor_bps``, ``lift_cap_bps`` and ``open_limit_bps``; the rest
    keep their value. The chain refuses the group whole on any bound breach
    and applies it from the next block. Verify with the ``lending_params``
    read.
    """

    op = "set_lending_long_params"
    signer = "coldkey"
    origin = "root"
    verify = "lending_params"
    wraps = (("Lending", "set_long_params"), ("Sudo", "sudo"))

    changes: dict = field(metadata={"help": CHANGES_HELP})

    def __post_init__(self):
        self.changes = _checked_changes("long", self.changes)

    async def build(self, substrate, wallet: Any):
        params = await _merged_params(substrate, "long", self.changes)
        return await substrate.compose(calls.Lending.set_long_params(params=params))

    def summary(self) -> str:
        return f"set long lending parameters {_changes_text(self.changes)}"

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class SetLendingDeregParams(Intent):
    """Change the lending parameters tied to subnet deregistration (root only).

    ``changes`` names the fields to change among ``zone_bps``,
    ``pool_tao_ema_blocks``, ``rank_window_days`` and
    ``max_long_positions_per_subnet``; the rest keep their value. Changing
    ``rank_window_days`` re-seeds every subnet's rank price ring from its
    published average. The chain refuses the group whole on any bound breach
    and applies it from the next block. Verify with the ``lending_params``
    read.
    """

    op = "set_lending_dereg_params"
    signer = "coldkey"
    origin = "root"
    verify = "lending_params"
    wraps = (("Lending", "set_dereg_params"), ("Sudo", "sudo"))

    changes: dict = field(metadata={"help": CHANGES_HELP})

    def __post_init__(self):
        self.changes = _checked_changes("dereg", self.changes)

    async def build(self, substrate, wallet: Any):
        params = await _merged_params(substrate, "dereg", self.changes)
        return await substrate.compose(calls.Lending.set_dereg_params(params=params))

    def summary(self) -> str:
        return f"set dereg lending parameters {_changes_text(self.changes)}"

    def affects_all_subnets(self) -> bool:
        return True
