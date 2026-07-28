"""Price and swap-simulation reads."""

from __future__ import annotations

from dataclasses import dataclass

from .._generated import runtime_apis as api
from ..balance import Balance
from ..settings import RAO_PER_TAO
from .base import read


@dataclass
class SwapQuote:
    """Simulated swap result: what you'd receive, the fee, and slippage."""

    tao: Balance  # TAO-side amount
    alpha: Balance  # alpha-side amount (tagged with the subnet)
    tao_fee: Balance
    alpha_fee: Balance
    tao_slippage: Balance
    alpha_slippage: Balance


def _swap_quote(view, r: dict, netuid: int) -> SwapQuote:
    return SwapQuote(
        tao=Balance.from_rao(int(r["tao_amount"])),
        alpha=view.balance(int(r["alpha_amount"]), netuid),
        tao_fee=Balance.from_rao(int(r["tao_fee"])),
        alpha_fee=view.balance(int(r["alpha_fee"]), netuid),
        tao_slippage=Balance.from_rao(int(r["tao_slippage"])),
        alpha_slippage=view.balance(int(r["alpha_slippage"]), netuid),
    )


@read(
    "quote_stake",
    {"netuid": "integer", "amount_tao": "number"},
    category="Prices & swaps",
    param_docs={
        "netuid": "Subnet to simulate staking into.",
        "amount_tao": "TAO amount to simulate staking.",
    },
)
async def quote_stake(view, netuid: int, amount_tao: float) -> SwapQuote:
    """Simulate staking ``amount_tao`` TAO into a subnet: alpha out, fee, and slippage.

    Use this to compute a safe ``limit_price_rao`` for the ``add_stake_limit`` intent.
    """
    rao = Balance.from_tao(amount_tao).rao
    r = await view.runtime(
        api.SwapRuntimeApi.sim_swap_tao_for_alpha, {"netuid": netuid, "tao": rao}
    )
    return _swap_quote(view, r, netuid)


@read(
    "quote_unstake",
    {"netuid": "integer", "amount_alpha": "number"},
    category="Prices & swaps",
    param_docs={
        "netuid": "Subnet to simulate unstaking from.",
        "amount_alpha": "Alpha amount to simulate unstaking.",
    },
)
async def quote_unstake(view, netuid: int, amount_alpha: float) -> SwapQuote:
    """Simulate unstaking ``amount_alpha`` alpha from a subnet: TAO out, fee, and slippage."""
    rao = Balance.from_amount(amount_alpha, netuid).rao
    r = await view.runtime(
        api.SwapRuntimeApi.sim_swap_alpha_for_tao, {"netuid": netuid, "alpha": rao}
    )
    return _swap_quote(view, r, netuid)


@read("alpha_prices", {}, category="Prices & swaps")
async def alpha_prices(view) -> dict[int, float]:
    """Spot alpha price for every subnet, as TAO per alpha keyed by netuid."""
    records = await view.runtime(api.SwapRuntimeApi.current_alpha_price_all, [])
    return {int(r["netuid"]): int(r["price"]) / RAO_PER_TAO for r in records or []}


@read(
    "alpha_price",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet whose alpha price to read."},
)
async def alpha_price(view, netuid: int) -> dict:
    """Current spot alpha price for a subnet, as TAO per alpha."""
    price_rao = await view.runtime(api.SwapRuntimeApi.current_alpha_price, [netuid])
    return {
        "netuid": netuid,
        "tao_per_alpha": int(price_rao) / RAO_PER_TAO,
        "price_rao": int(price_rao),
    }
