"""Root dividend / beta basket reads.

Root dividends accrue as shares of each validator's beta basket — an escrowed
per-validator index fund of subnet alpha, built each epoch from the
validator's root dividends per its root weights (``set_root_weights``) and
redeemed by stakers with ``claim_root``. These reads wrap the
``BetaBasketRuntimeApi`` runtime APIs plus the claim-threshold storage entry.
"""

from __future__ import annotations

from typing import Any

from .._generated import runtime_apis as api
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


def _summary_record(view, summary: Any) -> dict:
    """Shape one decoded chain ``BasketSummary`` into the read's output record."""
    nav = int(summary.get("nav_tao") or 0)
    spot_nav = int(summary.get("spot_nav_tao") or 0)
    shares = int(summary.get("shares") or 0)
    deposited = int(summary.get("deposited_tao") or 0)
    redeemed = int(summary.get("redeemed_tao") or 0)
    weights = [(int(netuid), int(weight)) for netuid, weight in summary.get("weights") or []]
    weight_total = sum(weight for _, weight in weights)
    return {
        "hotkey": str(summary.get("hotkey")),
        "nav_tao": view.balance(nav, _ROOT_NETUID),
        "spot_nav_tao": view.balance(spot_nav, _ROOT_NETUID),
        "shares": shares,
        # TAO (rao) per fund share; 1.0 at par, grows as the fund compounds.
        "share_price": nav / shares if shares else 0.0,
        "deposited_tao": view.balance(deposited, _ROOT_NETUID),
        "redeemed_tao": view.balance(redeemed, _ROOT_NETUID),
        # Lifetime multiple on deposits: (current value + everything paid out) / paid in.
        "lifetime_return": (nav + redeemed) / deposited if deposited else None,
        "weights": [
            {
                "netuid": netuid,
                "weight": weight,
                "share": weight / weight_total if weight_total else 0.0,
            }
            for netuid, weight in weights
        ],
        "holdings": [
            {
                "netuid": int(holding["netuid"]),
                "alpha": view.balance(int(holding["alpha"]), int(holding["netuid"])),
                "spot_tao": view.balance(int(holding["spot_tao"]), _ROOT_NETUID),
                "realizable_tao": view.balance(int(holding["realizable_tao"]), _ROOT_NETUID),
            }
            for holding in summary.get("holdings") or []
        ],
    }


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

    For each hotkey the coldkey stakes to: the owed beta-basket fund shares
    and the TAO they would realize if claimed now. Zero-owed validators are
    omitted.
    """
    rows = await view.runtime(
        api.BetaBasketRuntimeApi.get_root_basket_positions, [coldkey_ss58]
    )
    records = [
        {
            "hotkey": str(hotkey),
            "shares": int(shares),
            "owed_tao": view.balance(int(payout), _ROOT_NETUID),
        }
        for hotkey, shares, payout in rows or []
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
    "validator_basket_summary",
    {"hotkey_ss58": "string"},
    category="Staking",
    param_docs={"hotkey_ss58": "Validator hotkey whose basket to summarize."},
)
async def validator_basket_summary(view, hotkey_ss58: str) -> dict:
    """Everything about one validator's beta basket, in a single call.

    Valuation (realizable NAV and spot NAV), outstanding fund shares and the
    resulting share price (1.0 at par, grows as the fund compounds), lifetime
    deposited/redeemed TAO and the lifetime return multiple
    `(nav + redeemed) / deposited`, the validator's root weight vector, and
    the per-subnet holdings each valued at spot and at realizable depth.
    """
    summary = await view.runtime(
        api.BetaBasketRuntimeApi.get_validator_basket_summary, [hotkey_ss58]
    )
    return _summary_record(view, summary or {})


@read(
    "root_baskets",
    {},
    category="Staking",
)
async def root_baskets(view) -> list[dict]:
    """Beta basket summaries for every validator with an active basket.

    The network-wide leaderboard: one `validator_basket_summary` record per
    validator with outstanding fund shares, sorted by NAV descending. Compare
    `share_price` / `lifetime_return` across validators to rank basket
    performance.
    """
    rows = await view.runtime(api.BetaBasketRuntimeApi.get_all_validator_baskets, [])
    records = [_summary_record(view, summary) for summary in rows or []]
    records.sort(key=lambda entry: -entry["nav_tao"].rao)
    return records


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
    normalized `share` of the total.     Netuid 0 means "hold as TAO / root
    stake". An empty list means no custom weights are set; dividends default
    to 100% root (TAO in the basket).
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
