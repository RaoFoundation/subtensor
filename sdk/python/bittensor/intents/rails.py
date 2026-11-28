"""Canonical rails intents: tUSD, the tUSD/TAO pool, and payments out to a spoke."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, ClassVar, Optional

from .._generated import calls
from .._generated import storage as st
from .._generated.runtime_apis import CanonicalPoolApi, RailsApi
from ..balance import Balance
from ..result import BittensorError, chain_error_from_dispatch
from ._money import (
    ALL,
    UNBOUNDED,
    Money,
    Spend,
    tao_amount,
    tusd_amount,
    tusd_from_units,
    tusd_units,
)
from .base import Intent, IntentPreflight
from .registry import register
from .staking import DEFAULT_RATE_TOLERANCE, _check_rate_tolerance

RATE_TOLERANCE_HELP = (
    "Maximum fall of what you receive below the current quote, as a fraction (0.05 = 5%)."
)

# The assets of the rails on Bittensor, as a person names them.
TAO = "tao"
TUSD = "tusd"
# What a payment through a bridge can deliver on its spoke.
STABLE = "stable"
ALPHA = "alpha"


def _asset(value: str, label: str, allowed: tuple[str, ...]) -> str:
    asset = str(value).strip().lower()
    if asset not in allowed:
        raise BittensorError(f"{label} is one of {', '.join(allowed)}; got {value!r}.")
    return asset


def _positive_tusd(value: Money, label: str) -> str:
    amount = tusd_amount(value)
    if not tusd_units(amount):
        raise BittensorError(f"{label}: the tUSD amount must be above 0")
    return amount


def _positive_tao(value: Money, label: str) -> Balance:
    amount = tao_amount(value)
    if not amount.rao:
        raise BittensorError(f"{label}: the TAO amount must be above 0")
    return amount


def _less(amount: int, tolerance: float) -> int:
    return int(amount * (1 - tolerance))


def _tusd_text(units: int) -> str:
    return f"{tusd_from_units(units)} tUSD"


async def _pool(substrate) -> tuple[int, int, int]:
    """The pool's TAO reserve, tUSD reserve and total shares."""
    tao, tusd, shares = await asyncio.gather(
        substrate.query(*st.CanonicalPool.ReserveTao),
        substrate.query(*st.CanonicalPool.ReserveTusd),
        substrate.query(*st.CanonicalPool.TotalShares),
    )
    return int(tao or 0), int(tusd or 0), int(shares or 0)


@register
@dataclass
class TransferTusd(Intent):
    """Transfer tUSD from the coldkey to a destination address.

    tUSD is the chain's dollar: bridges mint it one for one against a
    stablecoin they lock on another chain. A transfer is irreversible. An
    account holds no tUSD or at least 0.01, so a transfer that would leave less
    than that on either side fails: send the whole balance instead.
    """

    op = "transfer_tusd"
    signer = "coldkey"
    wraps = (("Tusd", "transfer"),)

    dest_ss58: str = field(metadata={"help": "Destination account that receives the tUSD."})
    amount_tusd: Money = field(metadata={"help": "tUSD sent."})

    def __post_init__(self):
        self.amount_tusd = _positive_tusd(self.amount_tusd, self.op)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Tusd.transfer(dest=self.dest_ss58, amount=tusd_units(self.amount_tusd))
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        held = int(await substrate.query(*st.Tusd.Balances, [signer_address]) or 0)
        if tusd_units(self.amount_tusd) > held:
            return [f"the account holds {_tusd_text(held)}"]
        return []

    def summary(self) -> str:
        return f"transfer {self.amount_tusd} tUSD to {self.dest_ss58}"

    def spend(self) -> Spend:
        # tUSD leaves the signer; a TAO spend cap cannot bound it.
        return UNBOUNDED


@register
@dataclass
class SwapPool(Intent):
    """Swap between tUSD and TAO in the canonical pool.

    Sells ``amount`` of ``sell`` (``tusd`` or ``tao``) for the other asset at
    the pool's price, less its fee, which stays in the pool for its liquidity
    providers. Fails rather than receiving less than the current quote less
    ``rate_tolerance``. The pool caps one swap at a fraction of its depth and
    rate-limits each direction, so a large amount may need several swaps. A
    plan's ``preview`` holds the quote as ``receive`` and ``minimum``.
    """

    op = "swap_pool"
    signer = "coldkey"
    wraps = (("CanonicalPool", "swap_tusd_for_tao"), ("CanonicalPool", "swap_tao_for_tusd"))
    mev_shield_default = True

    sell: str = field(metadata={"help": "The asset sold: tusd or tao."})
    amount: Money = field(metadata={"help": "How much of the sold asset."})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.sell = _asset(self.sell, "sell", (TUSD, TAO))
        if self.sell == TAO:
            self.amount = _positive_tao(self.amount, self.op)
        else:
            self.amount = _positive_tusd(self.amount, self.op)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    def _amount_in(self) -> int:
        return self.amount.rao if self.sell == TAO else tusd_units(self.amount)

    async def _quote(self, substrate) -> int:
        method = (
            CanonicalPoolApi.quote_tao_for_tusd
            if self.sell == TAO
            else CanonicalPoolApi.quote_tusd_for_tao
        )
        quote = await substrate.runtime_call(*method, [self._amount_in()])
        if quote is None:
            raise BittensorError("the pool cannot make this swap: it is empty, or pays nothing")
        return int(quote)

    def _out_text(self, units: int) -> str:
        return _tusd_text(units) if self.sell == TAO else str(Balance.from_rao(units))

    async def build(self, substrate, wallet: Any):
        min_out = _less(await self._quote(substrate), self.rate_tolerance)
        call = (
            calls.CanonicalPool.swap_tao_for_tusd(amount_in=self._amount_in(), min_out=min_out)
            if self.sell == TAO
            else calls.CanonicalPool.swap_tusd_for_tao(amount_in=self._amount_in(), min_out=min_out)
        )
        return await substrate.compose(call)

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        quote = await self._quote(substrate)
        minimum = _less(quote, self.rate_tolerance)
        preview.facts = [
            ("receive", f"~{self._out_text(quote)}"),
            ("minimum", self._out_text(minimum)),
            *preview.facts,
        ]
        preview.facts_title = "Quote"
        preview.preview = {
            "receive": tusd_from_units(quote)
            if self.sell == TAO
            else format(Balance.from_rao(quote).decimal, "f"),
            "minimum": tusd_from_units(minimum)
            if self.sell == TAO
            else format(Balance.from_rao(minimum).decimal, "f"),
        }
        return preview

    def summary(self) -> str:
        sold = str(self.amount) if self.sell == TAO else f"{self.amount} tUSD"
        bought = "tUSD" if self.sell == TAO else "TAO"
        return (
            f"swap {sold} for {bought} in the canonical pool"
            f" (fails if the quote falls >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        return self.amount if self.sell == TAO else UNBOUNDED


@register
@dataclass
class AddPoolLiquidity(Intent):
    """Add TAO and tUSD to the canonical pool and receive its shares.

    Takes at most ``amount_tao`` and ``amount_tusd`` in the pool's current
    ratio, so one of the two is usually not used in full. Shares earn the
    pool's swap fees: they redeem a growing part of both reserves. The first
    deposit sets the price and must be within 2% of the TAO/USD oracle price.
    Fails rather than receiving fewer shares than the reserves now give, less
    ``rate_tolerance``.
    """

    op = "add_pool_liquidity"
    signer = "coldkey"
    wraps = (("CanonicalPool", "add_liquidity"),)

    amount_tao: Money = field(metadata={"help": "The most TAO to add."})
    amount_tusd: Money = field(metadata={"help": "The most tUSD to add."})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        self.amount_tao = _positive_tao(self.amount_tao, self.op)
        self.amount_tusd = _positive_tusd(self.amount_tusd, self.op)
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def _expected_shares(self, substrate) -> int:
        """The shares the deposit buys at the pool's reserves now. The first deposit sets
        them itself, so nothing is expected of it."""
        tao, tusd, shares = await _pool(substrate)
        if not shares or not tao or not tusd:
            return 0
        return min(
            self.amount_tao.rao * shares // tao,
            tusd_units(self.amount_tusd) * shares // tusd,
        )

    async def build(self, substrate, wallet: Any):
        expected = await self._expected_shares(substrate)
        return await substrate.compose(
            calls.CanonicalPool.add_liquidity(
                max_tao=self.amount_tao.rao,
                max_tusd=tusd_units(self.amount_tusd),
                min_shares=_less(expected, self.rate_tolerance),
            )
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        held = int(await substrate.query(*st.Tusd.Balances, [signer_address]) or 0)
        if not await substrate.query(*st.CanonicalPool.Enabled):
            return ["the pool is disabled"]
        if tusd_units(self.amount_tusd) > held:
            return [f"the account holds {_tusd_text(held)}"]
        return []

    def summary(self) -> str:
        return (
            f"add up to {self.amount_tao} and {self.amount_tusd} tUSD to the canonical pool"
            f" (fails if the shares fall >{self.rate_tolerance:.2%})"
        )

    def spend(self) -> Spend:
        return self.amount_tao


@register
@dataclass
class RemovePoolLiquidity(Intent):
    """Redeem shares of the canonical pool for TAO and tUSD.

    Burns ``shares`` (``all`` for every share you hold) and pays their part of
    both reserves, which includes the swap fees the pool earned since you
    added. Open even while the pool is disabled. Fails rather than receiving
    less of either asset than the reserves now give, less ``rate_tolerance``.
    """

    op = "remove_pool_liquidity"
    signer = "coldkey"
    wraps = (("CanonicalPool", "remove_liquidity"),)

    shares: str = field(metadata={"help": "Shares to redeem, a whole number, or all."})
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    def __post_init__(self):
        text = str(self.shares).strip().lower()
        if text != ALL and (not text.isdigit() or int(text) == 0):
            raise BittensorError(f"shares is a whole number above 0, or 'all'; got {text!r}.")
        self.shares = text
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    async def _shares(self, substrate, holder: str) -> int:
        held = int(await substrate.query(*st.CanonicalPool.Shares, [holder]) or 0)
        return held if self.shares == ALL else int(self.shares)

    async def build(self, substrate, wallet: Any):
        shares = await self._shares(substrate, self.coldkey_address(wallet))
        tao, tusd, total = await _pool(substrate)
        if not shares or not total:
            raise BittensorError("there are no shares to redeem")
        return await substrate.compose(
            calls.CanonicalPool.remove_liquidity(
                shares=shares,
                min_tao=_less(tao * shares // total, self.rate_tolerance),
                min_tusd=_less(tusd * shares // total, self.rate_tolerance),
            )
        )

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        held = int(await substrate.query(*st.CanonicalPool.Shares, [signer_address]) or 0)
        if not held:
            return ["the account holds no shares of the pool"]
        if self.shares != ALL and int(self.shares) > held:
            return [f"the account holds {held} shares"]
        return []

    def summary(self) -> str:
        shares = "all shares" if self.shares == ALL else f"{self.shares} shares"
        return (
            f"redeem {shares} of the canonical pool"
            f" (fails if the payout falls >{self.rate_tolerance:.2%})"
        )


@register
@dataclass
class BridgeOut(Intent):
    """Pay TAO or tUSD on Bittensor for an asset delivered on another chain.

    Pays ``amount`` of ``pay_with`` (``tusd`` or ``tao``) and has the bridge
    deliver ``receive`` to ``to_address`` on its chain: ``stable`` (the
    stablecoin the bridge locked, one for one against tUSD), ``tao``
    (canonical TAO, one for one against TAO the hub reserves) or ``alpha`` (the
    alpha token of ``netuid``, a share of the stake the hub holds at the
    subnet's index hotkey). TAO and tUSD trade through the canonical pool on
    the way when the route needs it. The payment settles in this extrinsic or
    fails as a whole; the asset arrives on the other chain once the bridge has
    relayed the hub's message, usually within a minute. A bridge releases only
    the stablecoin it brought in. Fails rather than delivering less than the
    current quote less ``rate_tolerance``. The coldkey also pays, in TAO and on
    top of the amount, the cost of one message to the bridge's chain: it goes
    into the bridge's budget, which pays the transport for the message. A
    plan's ``preview`` holds the quote as ``receive`` and ``minimum``, in units
    of the delivered asset, and that cost as ``message_cost_tao``.
    """

    op = "bridge_out"
    signer = "coldkey"
    wraps = (("Rails", "bridge_out"),)
    mev_shield_default = True

    bridge: int = field(metadata={"help": "Id of the bridge: 8453 is Base."})
    pay_with: str = field(metadata={"help": "The asset paid: tusd or tao."})
    amount: Money = field(metadata={"help": "How much of it."})
    receive: str = field(metadata={"help": "What is delivered: stable, tao or alpha."})
    to_address: str = field(metadata={"help": "0x address on the bridge's chain that receives it."})
    netuid: Optional[int] = field(
        default=None, metadata={"help": "Subnet of the alpha token, with --receive alpha."}
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE, metadata={"help": RATE_TOLERANCE_HELP}
    )

    all_amount_fields: ClassVar[tuple[str, ...]] = ()

    def __post_init__(self):
        self.pay_with = _asset(self.pay_with, "pay_with", (TUSD, TAO))
        self.receive = _asset(self.receive, "receive", (STABLE, TAO, ALPHA))
        if self.pay_with == TAO:
            self.amount = _positive_tao(self.amount, self.op)
        else:
            self.amount = _positive_tusd(self.amount, self.op)
        if (self.receive == ALPHA) != (self.netuid is not None):
            raise BittensorError("bridge_out: give netuid with receive=alpha, and only then")
        address = str(self.to_address).strip()
        if not (address.startswith("0x") and len(address) == 42):
            raise BittensorError(f"to_address is a 0x address of 20 bytes; got {address!r}.")
        try:
            value = int(address, 16)
        except ValueError:
            raise BittensorError(f"to_address is not hex: {address!r}.") from None
        if value == 0:
            raise BittensorError("to_address is the zero address: nothing can be paid to it.")
        self.to_address = address.lower()
        _check_rate_tolerance(self.rate_tolerance, can_disable=False)

    def _amount(self) -> int:
        return self.amount.rao if self.pay_with == TAO else tusd_units(self.amount)

    def _source(self) -> str:
        return "Tao" if self.pay_with == TAO else "Tusd"

    def _destination(self) -> dict:
        if self.receive == STABLE:
            return {"Stable": {"to": self.to_address}}
        if self.receive == TAO:
            return {"Tao": {"to": self.to_address}}
        return {"Alpha": {"netuid": self.netuid, "to": self.to_address}}

    async def _quote(self, substrate, payer: str) -> int:
        quote = await substrate.runtime_call(
            *RailsApi.quote_bridge_out,
            [payer, self.bridge, self._source(), self._amount(), self._destination()],
        )
        if "Err" in quote:
            raise chain_error_from_dispatch(quote["Err"])
        return int(quote["Ok"])

    def _out_text(self, units: int) -> str:
        if self.receive == STABLE:
            return f"{tusd_from_units(units)} of the stablecoin"
        if self.receive == TAO:
            return f"{Balance.from_rao(units)} of canonical TAO"
        return f"{tusd_from_units(units)} of the netuid {self.netuid} alpha token"

    async def build(self, substrate, wallet: Any):
        quote = await self._quote(substrate, self.coldkey_address(wallet))
        return await substrate.compose(
            calls.Rails.bridge_out(
                bridge=self.bridge,
                source=self._source(),
                amount=self._amount(),
                destination=self._destination(),
                min_out=_less(quote, self.rate_tolerance),
            )
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        preview = await super().preflight(substrate, dispatch_origin, fee_payer, call=call)
        try:
            quote = await self._quote(substrate, dispatch_origin)
        except BittensorError as error:
            preview.blocks.append(f"the hub would refuse this payment: {error}")
            return preview
        minimum = _less(quote, self.rate_tolerance)
        cost = Balance.from_rao(
            int(await substrate.query(*st.Rails.MessageCost, [self.bridge]) or 0)
        )
        preview.facts = [
            ("receive", f"~{self._out_text(quote)}"),
            ("minimum", self._out_text(minimum)),
            ("to", self.to_address),
            ("message cost", f"{cost}, into the bridge's budget for the message"),
            *preview.facts,
        ]
        preview.facts_title = "Quote"
        preview.preview = {
            "receive": tusd_from_units(quote),
            "minimum": tusd_from_units(minimum),
            "message_cost_tao": cost.tao,
        }
        if cost.rao:
            # The cost is TAO on top of what is paid: the account must hold both.
            preview.required_free = cost + self.amount if self.pay_with == TAO else cost
        return preview

    def summary(self) -> str:
        paid = str(self.amount) if self.pay_with == TAO else f"{self.amount} tUSD"
        bought = {
            STABLE: "the stablecoin",
            TAO: "canonical TAO",
            ALPHA: f"the netuid {self.netuid} alpha token",
        }[self.receive]
        return (
            f"pay {paid} for {bought} at {self.to_address} through bridge {self.bridge}"
            f" (fails if the quote falls >{self.rate_tolerance:.2%})"
        )

    def touches_netuids(self) -> list[int]:
        return [] if self.netuid is None else [self.netuid]

    def spend(self) -> Spend:
        return self.amount if self.pay_with == TAO else UNBOUNDED
