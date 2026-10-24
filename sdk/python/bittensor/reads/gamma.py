"""Gamma reads: deposits, mint quotes, the TAO/USD price, and service urls."""

from __future__ import annotations

from typing import Any, Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..settings import GAMMA_UNITS
from .base import read, utf8_text


def _gamma(units: Any) -> float:
    """Chain units as gamma."""
    return int(units) / GAMMA_UNITS


def _deposit_record(info: dict) -> dict:
    deposit = info["deposit"]
    cap = deposit.get("daily_spend_cap")
    return {
        "netuid": int(info["netuid"]),
        "hotkey": str(info["hotkey"]),
        "coldkey": str(deposit["coldkey"]),
        "spender": str(deposit["spender"]),
        "balance_gamma": _gamma(deposit["balance"]),
        "daily_spend_cap_gamma": None if cap is None else _gamma(cap),
        "spendable_gamma": _gamma(info["spendable"]),
    }


def _mint_quote(view, quote: Any, netuid: int) -> Optional[dict]:
    if not quote:
        return None
    alpha, tao, gamma = quote
    return {
        "netuid": netuid,
        "alpha_burned": view.balance(int(alpha), netuid),
        "tao_value": view.balance(int(tao), 0),
        "gamma": _gamma(gamma),
    }


@read(
    "gamma_deposit",
    {"netuid": "integer", "hotkey_ss58": "string"},
    category="Gamma",
    param_docs={"netuid": "Subnet of the deposit.", "hotkey_ss58": "Hotkey of the deposit."},
)
async def gamma_deposit(view, netuid: int, hotkey_ss58: str) -> Optional[dict]:
    """The gamma deposit at a hotkey on a subnet, or None.

    `spendable_gamma` is the balance within what its daily spend cap allows now.
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
async def quote_mint_gamma(view, netuid: int, amount_tao: float) -> Optional[dict]:
    """The alpha burned, its TAO value, and the gamma `mint_gamma` would mint now.

    None when gamma cannot be minted: the subnet does not exist, minting or its
    subtoken is disabled, the TAO/USD price is missing or stale, the pool lacks liquidity,
    or the amount is below the minimum.
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
async def quote_mint_gamma_with_stake(view, netuid: int, amount_alpha: float) -> Optional[dict]:
    """The alpha burned, its TAO value, and the gamma `mint_gamma_with_stake` would mint now.

    None when gamma cannot be minted, as for `quote_mint_gamma`.
    """
    rao = Balance.from_amount(amount_alpha, netuid).rao
    quote = await view.runtime(api.GammaRuntimeApi.sim_mint_gamma_with_stake, [netuid, rao])
    return _mint_quote(view, quote, netuid)


@read("tao_usd_price", {}, category="Gamma")
async def tao_usd_price(view) -> dict:
    """The TAO/USD price oracle, its last feed, and the accepted price at that feed.

    The accepted price falls to the feed at once and rises toward it by at most
    0.0066% (66 ppm) of itself per block. Minting needs a feed at most 50
    blocks old.
    """
    oracle = await view.query(st.SubtensorModule.TaoUsdPriceOracle)
    state = await view.query(st.SubtensorModule.TaoUsdPrice)
    return {
        "oracle": str(oracle) if oracle else None,
        "accepted_usd": state["accepted"] / 10**9 if state else None,
        "feed_usd": state["feed"] / 10**9 if state else None,
        "fed_at": int(state["fed_at"]) if state else None,
    }


@read(
    "service_urls",
    {"netuid": "integer"},
    category="Gamma",
    param_docs={"netuid": "Subnet whose service urls to list."},
)
async def service_urls(view, netuid: int) -> dict[str, str]:
    """Every service url published on a subnet, by hotkey.

    Each points to its hotkey's own document of its services and the spender
    to name on gamma deposits; the chain does not check what it says.
    """
    rows = await view.query_map(st.SubtensorModule.ServiceUrls, [netuid])
    return {str(hotkey): utf8_text(info["url"]) for hotkey, info in rows if info["url"]}
