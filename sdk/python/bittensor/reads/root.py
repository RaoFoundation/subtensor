"""Root dividend / basket reads.

Root dividends accrue inside each validator's basket — an escrowed
per-validator index fund of subnet alpha, built each epoch from the
validator's root dividends per its root weights (``set_root_weights``) and
redeemed by stakers with ``claim_root_with_hotkey`` (or coldkey-wide
``claim_root``). Every figure these reads return is
TAO-denominated (or the actual per-subnet alpha holdings); the fund's
internal share accounting is never exposed. They wrap the
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
    """Shape one decoded chain ``BasketSummary`` into the read's output record.

    Everything is TAO-denominated (plus the per-subnet alpha holdings); the
    fund's internal share accounting is never exposed.
    """
    nav = int(summary.get("nav_tao") or 0)
    spot_nav = int(summary.get("spot_nav_tao") or 0)
    deposited = int(summary.get("deposited_tao") or 0)
    redeemed = int(summary.get("redeemed_tao") or 0)
    weights = [(int(netuid), int(weight)) for netuid, weight in summary.get("weights") or []]
    weight_total = sum(weight for _, weight in weights)
    return {
        "hotkey": str(summary.get("hotkey")),
        "nav_tao": view.balance(nav, _ROOT_NETUID),
        "spot_nav_tao": view.balance(spot_nav, _ROOT_NETUID),
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

    Marks the coldkey's accrued basket entitlement across every validator it
    root-stakes to at current pool prices (the same slippage-aware valuation
    the chain uses to size redemptions). This is the "pending TAO" figure
    behind coldkey-wide `claim_root`; for a per-validator breakdown use
    `root_basket_owed_breakdown` before `claim_root_with_hotkey`.
    Per-validator amounts below the claim threshold are still included here
    even though a claim would skip them.
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

    For each hotkey the coldkey stakes to: the TAO its accrued entitlement
    would realize if claimed now with ``claim_root_with_hotkey``. Zero-owed
    validators are omitted. Use this to pick a validator before claiming.
    """
    rows = await view.runtime(api.BetaBasketRuntimeApi.get_root_basket_positions, [coldkey_ss58])
    records = [
        {
            "hotkey": str(hotkey),
            "owed_tao": view.balance(int(payout), _ROOT_NETUID),
        }
        for hotkey, _shares, payout in rows or []
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
    """A validator's basket holdings: per subnet, the alpha held and its TAO value.

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
    """Everything about one validator's basket, in a single call.

    Valuation (realizable NAV and spot NAV), lifetime deposited/redeemed TAO
    and the lifetime return multiple `(nav + redeemed) / deposited`, the
    validator's root weight vector, and the per-subnet alpha holdings each
    valued at spot and at realizable depth. All figures are TAO (or alpha
    for the holdings themselves).
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
    """Basket summaries for every validator with an active basket.

    The network-wide leaderboard: one `validator_basket_summary` record per
    validator with an active fund, sorted by NAV descending. Compare
    `lifetime_return` across validators to rank basket performance.
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
    """A validator's basket net asset value in TAO (realizable quote)."""
    value = await view.runtime(api.BetaBasketRuntimeApi.get_validator_basket_nav, [hotkey_ss58])
    return view.balance(int(value or 0), _ROOT_NETUID)


@read(
    "root_basket_total_nav",
    {},
    category="Staking",
)
async def root_basket_total_nav(view) -> Balance:
    """Network-wide total basket NAV across all validators, in TAO.

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
    """A validator's root dividend distribution vector (basket weights).

    The `(netuid, weight)` pairs its root dividends are deployed into each
    epoch, exactly as stored (u16, max-upscaled), plus each destination's
    normalized `share` of the total.     Netuid 0 means "hold as TAO / root
    stake". An empty list means no custom weights are set; the fund is
    uncurated and each subnet's dividend accumulates in place on that
    subnet, trade-free (no sell, no redeploy).
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

    `claim_root` and `claim_root_with_hotkey` silently skip any per-validator
    basket redemption whose estimated payout falls below this threshold; the
    entitlement keeps accruing and pays out once it clears. Set by root via
    `set_root_claim_threshold`.
    """
    value = await view.query(_ROOT_CLAIM_THRESHOLD, [_ROOT_NETUID])
    return view.balance(_i96f32_rao(value), _ROOT_NETUID)
