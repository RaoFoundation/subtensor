"""Root dividend / beta basket reads.

Root dividends accrue as shares of each validator's beta basket — an escrowed
per-validator index fund of subnet alpha, built each epoch from the
validator's root dividends per its root weights (``set_root_weights``) and
redeemed by stakers with ``claim_root``. These reads wrap the
``BetaBasketRuntimeApi`` runtime APIs plus the claim-threshold storage entry.
"""

from __future__ import annotations

import asyncio
from typing import Any

from .._generated import runtime_apis as api
from .._generated import storage as st
from .._generated.storage import Item
from ..balance import Balance
from .base import read

# TODO(codegen): switch to `st.SubtensorModule.RootClaimableThreshold` once the
# storage registry is regenerated against spec >= 438.
_ROOT_CLAIM_THRESHOLD = Item("SubtensorModule", "RootClaimableThreshold", "I96F32")

_ROOT_NETUID = 0


def _i96f32_rao(value: Any) -> int:
    """Decode an I96F32 fixed-point chain value (``{'bits': ...}`` or raw int)
    to whole rao."""
    if isinstance(value, dict):
        return int(value.get("bits") or 0) >> 32
    return int(value or 0) >> 32


@read(
    "root_basket_owed",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose pending root dividends to value."},
)
async def root_basket_owed(view, coldkey_ss58: str) -> Balance:
    """Total TAO a coldkey would realize by claiming its root dividends now.

    Marks the coldkey's owed beta-basket shares across every validator it
    root-stakes to at current pool prices (the same slippage-aware valuation
    the chain uses to size redemptions). This is the "pending TAO" figure
    behind `claim_root`; per-validator amounts below the claim threshold are
    still included here even though a claim would skip them.
    """
    value = await view.runtime(api.BetaBasketRuntimeApi.get_root_basket_owed, [coldkey_ss58])
    return view.balance(int(value or 0), _ROOT_NETUID)


@read(
    "root_basket_owed_breakdown",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose pending root dividends to break down."},
)
async def root_basket_owed_breakdown(view, coldkey_ss58: str) -> list[dict]:
    """A coldkey's pending root dividends, itemized per validator hotkey.

    For each hotkey the coldkey stakes to, the TAO its owed beta-basket
    shares would realize if claimed now. Zero-owed validators are omitted.
    """
    view = await view.at()
    staked = await view.query(st.SubtensorModule.StakingHotkeys, [coldkey_ss58])
    hotkeys = [str(hotkey) for hotkey in staked or []]
    owed = await asyncio.gather(
        *(
            view.runtime(api.BetaBasketRuntimeApi.get_basket_payout, [hotkey, coldkey_ss58])
            for hotkey in hotkeys
        )
    )
    records = [
        {"hotkey": hotkey, "owed_tao": view.balance(int(value or 0), _ROOT_NETUID)}
        for hotkey, value in zip(hotkeys, owed)
        if int(value or 0) > 0
    ]
    records.sort(key=lambda entry: -entry["owed_tao"].rao)
    return records


@read(
    "validator_basket",
    {"hotkey_ss58": "string"},
    category="Staking",
    param_docs={"hotkey_ss58": "Validator hotkey whose basket to list."},
)
async def validator_basket(view, hotkey_ss58: str) -> list[dict]:
    """A validator's beta basket holdings: per subnet, the alpha held and its TAO value.

    The netuid-0 entry is the basket's TAO cash slot (held as root stake, so
    alpha and value coincide). Values are realizable quotes — what selling
    the holding would actually fetch at current pool depth — not spot marks.
    """
    rows = await view.runtime(api.BetaBasketRuntimeApi.get_validator_basket, [hotkey_ss58])
    return [
        {
            "netuid": int(netuid),
            "alpha": view.balance(int(alpha), int(netuid)),
            "value_tao": view.balance(int(tao), _ROOT_NETUID),
        }
        for netuid, alpha, tao in rows or []
    ]


@read(
    "validator_basket_nav",
    {"hotkey_ss58": "string"},
    category="Staking",
    param_docs={"hotkey_ss58": "Validator hotkey whose basket to value."},
)
async def validator_basket_nav(view, hotkey_ss58: str) -> Balance:
    """A validator's beta basket net asset value in TAO (realizable quote)."""
    value = await view.runtime(api.BetaBasketRuntimeApi.get_validator_basket_nav, [hotkey_ss58])
    return view.balance(int(value or 0), _ROOT_NETUID)


@read(
    "root_basket_total_nav",
    {},
    category="Staking",
)
async def root_basket_total_nav(view) -> Balance:
    """Network-wide total beta basket NAV across all validators, in TAO.

    Sampling this over time yields the TAO/day flowing to root stakers.
    """
    value = await view.runtime(api.BetaBasketRuntimeApi.get_root_basket_total_nav, [])
    return view.balance(int(value or 0), _ROOT_NETUID)


@read(
    "validator_root_weights",
    {"hotkey_ss58": "string"},
    category="Staking",
    param_docs={"hotkey_ss58": "Validator hotkey whose root weights to read."},
)
async def validator_root_weights(view, hotkey_ss58: str) -> list[dict]:
    """A validator's root dividend distribution vector (beta basket weights).

    The `(netuid, weight)` pairs its root dividends are deployed into each
    epoch, exactly as stored (u16, max-upscaled), plus each destination's
    normalized `share` of the total. Netuid 0 means "hold as TAO / root
    stake". An empty list means the validator has no root weights set and
    its root dividends are recycled.
    """
    rows = await view.runtime(api.BetaBasketRuntimeApi.get_validator_weights, [hotkey_ss58])
    pairs = [(int(netuid), int(weight)) for netuid, weight in rows or []]
    total = sum(weight for _, weight in pairs)
    return [
        {
            "netuid": netuid,
            "weight": weight,
            "share": weight / total if total else 0.0,
        }
        for netuid, weight in pairs
    ]


@read(
    "root_claim_threshold",
    {},
    category="Staking",
)
async def root_claim_threshold(view) -> Balance:
    """The minimum TAO payout for a root dividend claim.

    `claim_root` silently skips any per-validator basket redemption whose
    estimated payout falls below this threshold; the shares keep accruing
    and pay out once they clear it. Set by root via `set_root_claim_threshold`.
    """
    value = await view.query(_ROOT_CLAIM_THRESHOLD, [_ROOT_NETUID])
    return view.balance(_i96f32_rao(value), _ROOT_NETUID)
