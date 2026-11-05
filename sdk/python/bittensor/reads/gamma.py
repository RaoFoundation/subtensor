"""Gamma reads: deposits, mint quotes, and the TAO/USD price."""

from __future__ import annotations

from typing import Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..intents._money import gamma_from_units
from ..result import chain_error_from_dispatch
from .base import read

# Most the accepted TAO/USD price rises per block, in parts per billion of itself
# (`TAO_USD_PRICE_MAX_STEP`).
TAO_USD_PRICE_MAX_STEP = 66_000


def _deposit_record(info: dict) -> dict:
    deposit = info["deposit"]
    cap = deposit.get("daily_spend_cap")
    cap = None if cap is None else cap["cap"]
    return {
        "netuid": int(info["netuid"]),
        "hotkey": str(info["hotkey"]),
        "coldkey": str(deposit["coldkey"]),
        "spender": str(deposit["spender"]),
        "balance_gamma": gamma_from_units(deposit["balance"]),
        "daily_spend_cap_gamma": None if cap is None else gamma_from_units(cap),
        "spendable_gamma": gamma_from_units(info["spendable"]),
    }


def _mint_quote(view, quote: dict, netuid: int) -> dict:
    if "Err" in quote:
        raise chain_error_from_dispatch(quote["Err"])
    alpha, tao, gamma = quote["Ok"]
    return {
        "netuid": netuid,
        "alpha_burned": view.balance(int(alpha), netuid),
        "tao_value": view.balance(int(tao), 0),
        "gamma": gamma_from_units(gamma),
    }


@read(
    "gamma_deposit",
    {"netuid": "integer", "hotkey_ss58": "string"},
    category="Gamma",
    param_docs={"netuid": "Subnet of the deposit.", "hotkey_ss58": "Hotkey of the deposit."},
)
async def gamma_deposit(view, netuid: int, hotkey_ss58: str) -> Optional[dict]:
    """The gamma deposit at a hotkey on a subnet, or None.

    Its `netuid`, `hotkey`, `coldkey`, `spender`, and `balance_gamma`,
    `daily_spend_cap_gamma` and `spendable_gamma` as exact decimal strings of
    gamma. A `daily_spend_cap_gamma` of None is no cap, and "0" pauses the
    deposit. `spendable_gamma` is the balance within what its daily spend cap
    allows now.
    """
    info = await view.runtime(api.GammaRuntimeApi.get_gamma_deposit, [netuid, hotkey_ss58])
    return _deposit_record(info) if info else None


@read(
    "gamma_deposits",
    {"coldkey_ss58": "string"},
    category="Gamma",
    param_docs={"coldkey_ss58": "Coldkey owning the deposits."},
)
async def gamma_deposits(view, coldkey_ss58: str) -> list[dict]:
    """Every gamma deposit of a coldkey, on every subnet."""
    infos = await view.runtime(api.GammaRuntimeApi.get_gamma_deposits_for_coldkey, [coldkey_ss58])
    return [_deposit_record(info) for info in infos or []]


@read(
    "quote_mint_gamma",
    {"netuid": "integer", "amount_tao": "number"},
    category="Gamma",
    param_docs={"netuid": "Subnet to mint on.", "amount_tao": "TAO spent buying alpha."},
)
async def quote_mint_gamma(view, netuid: int, amount_tao: float) -> dict:
    """The alpha burned, its TAO value, and the gamma `mint_gamma` would mint now.

    Raises the chain error that stops the mint, such as `TaoUsdPriceUnavailable`
    when the TAO/USD price is missing or stale, or `AmountTooLow` below 0.1 gamma.
    """
    rao = Balance.from_tao(amount_tao).rao
    quote = await view.runtime(api.GammaRuntimeApi.sim_mint_gamma, [netuid, rao])
    return _mint_quote(view, quote, netuid)


@read(
    "quote_mint_gamma_with_stake",
    {"netuid": "integer", "amount_alpha": "number"},
    category="Gamma",
    param_docs={"netuid": "Subnet to mint on.", "amount_alpha": "Alpha of stake burned."},
)
async def quote_mint_gamma_with_stake(view, netuid: int, amount_alpha: float) -> dict:
    """The alpha burned, its TAO value, and the gamma `mint_gamma_with_stake` would mint now.

    Raises the chain error that stops the mint, such as `TaoUsdPriceUnavailable`
    when the TAO/USD price is missing or stale, or `AmountTooLow` below 0.1 gamma.
    """
    rao = Balance.from_amount(amount_alpha, netuid).rao
    quote = await view.runtime(api.GammaRuntimeApi.sim_mint_gamma_with_stake, [netuid, rao])
    return _mint_quote(view, quote, netuid)


@read("tao_usd_price", {}, category="Gamma")
async def tao_usd_price(view) -> dict:
    """The TAO/USD price oracle, its last feed, and the accepted price at this block.

    The accepted price falls to the feed at once and rises toward it by at most
    0.0066% (66 ppm) of itself per block, and mints value TAO at it. Minting needs a
    feed at most 300 blocks old, so no new mint succeeds once `blocks_since_last_feed`
    exceeds 300.
    """
    view = await view.at()
    oracle = await view.query(st.SubtensorModule.TaoUsdPriceOracle)
    state = await view.query(st.SubtensorModule.TaoUsdPrice)
    blocks = view.block - int(state["fed_at"]) if state else None
    return {
        "oracle": str(oracle) if oracle else None,
        "accepted_usd": _accepted_price(state, blocks) / 10**9 if state else None,
        "feed_usd": state["feed"] / 10**9 if state else None,
        "fed_at": int(state["fed_at"]) if state else None,
        "blocks_since_last_feed": blocks,
    }


def _accepted_price(state: dict, blocks: int) -> int:
    """The accepted price `blocks` after its feed: raised by `TAO_USD_PRICE_MAX_STEP` of
    itself, rounded up, per block, up to the feed, as the chain's `tao_usd_price_at`."""
    accepted = int(state["accepted"])
    step = -(-accepted * TAO_USD_PRICE_MAX_STEP // 10**9)
    return min(int(state["feed"]), accepted + step * blocks)
