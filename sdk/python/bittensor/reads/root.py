"""Root dividend / basket reads.

Root dividends accrue inside each validator's basket — an escrowed
per-validator index fund of subnet alpha, built each epoch from the
validator's root dividends per its root weights (``set_root_weights``) and
redeemed by stakers with ``claim_root_with_hotkey`` (or coldkey-wide
``claim_root``). Most figures these reads return are TAO-denominated (or the
actual per-subnet alpha holdings); the beta-denominated position reads
(``basket_position`` / ``root_basket_portfolio``) additionally expose the
fund's beta tokens: your beta balance is the stable product-facing number
that only grows with accruals and shrinks with claims, while its TAO value
moves with pool prices. They wrap the ``BetaBasketRuntimeApi`` runtime APIs
plus the claim-threshold storage entry.
"""

from __future__ import annotations

from typing import Any, Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from .._generated.runtime_apis import Method
from .._generated.storage import Item
from ..balance import Balance
from ..settings import RAO_PER_TAO
from .base import read

# TODO(codegen): switch to `st.SubtensorModule.RootClaimableThreshold` once the
# storage registry is regenerated against spec >= 438.
_ROOT_CLAIM_THRESHOLD = Item("SubtensorModule", "RootClaimableThreshold", "I96F32")

# TODO(codegen): switch to `api.BetaBasketRuntimeApi.*` once the runtime-API
# registry is regenerated against a spec that includes these v2 methods.
_GET_BASKET_POSITION = Method("BetaBasketRuntimeApi", "get_basket_position")
_GET_ROOT_BASKET_PORTFOLIO = Method("BetaBasketRuntimeApi", "get_root_basket_portfolio")

# TODO(codegen): switch to `api.BetaBasketRuntimeApi.*` once the runtime-API
# registry is regenerated against a spec that includes these v3 methods.
_GET_BETA_PRICING = Method("BetaBasketRuntimeApi", "get_beta_pricing")
_GET_ALL_BETA_PRICING = Method("BetaBasketRuntimeApi", "get_all_beta_pricing")

_ROOT_NETUID = 0

# Raw chain units per beta token. Chain units mint at par (1 per rao of TAO
# value at fund inception), so one beta token had a par value of exactly τ1
# and its price tracks fund performance from 1.0.
_RAW_PER_BETA = RAO_PER_TAO


def _i96f32_rao(value: Any) -> int:
    """Decode an I96F32 fixed-point chain value (``{'bits': ...}`` or raw int)
    to whole rao."""
    if isinstance(value, dict):
        return int(value.get("bits") or 0) >> 32
    return int(value or 0) >> 32


def _i96f32_float(value: Any) -> float:
    """Decode an I96F32 fixed-point chain value to a float, fraction kept.

    Used for ``BasketRate``, whose values are small ratios (β raw units
    minted per rao of root stake) that whole-rao truncation would zero out.
    """
    bits = int(value.get("bits") or 0) if isinstance(value, dict) else int(value or 0)
    return bits / 2**32


def _u64f64_float(value: Any) -> float:
    """Decode a U64F64 fixed-point chain value (``{'bits': ...}`` or raw int)
    to a float."""
    bits = int(value.get("bits") or 0) if isinstance(value, dict) else int(value or 0)
    return bits / 2**64


def _chain_pricing_record(row: Any) -> dict:
    """Shape one decoded chain ``BetaPricing`` row into the plain-float fields
    :func:`bittensor.basket_index.normalize_positions` passes through."""
    return {
        "hotkey": str(row.get("hotkey")),
        "spot_price": _u64f64_float(row.get("spot_price")),
        "display_price": _u64f64_float(row.get("display_price")),
        "stake_price": _u64f64_float(row.get("stake_price")),
        "staker_yield": _u64f64_float(row.get("staker_yield")),
        "staker_twr": _u64f64_float(row.get("staker_twr")),
        "bag_index": _u64f64_float(row.get("bag_index")),
        "stake_index": _u64f64_float(row.get("stake_index")),
        "first_block": int(row.get("first_block") or 0),
        "provisional": bool(row.get("provisional")),
        "display_shares": _u64f64_float(row.get("display_shares")),
    }


async def _attach_chain_pricing(view, records: list[dict]) -> None:
    """Attach the runtime's standardized pricing to board records.

    On nodes exposing ``BetaBasketRuntimeApi`` v3+, each record gains a
    ``chain_pricing`` dict and the display layer becomes a pass-through of
    the chain's canonical numbers. Silently a no-op on older nodes (and at
    pre-upgrade historical blocks), where the SDK's local index math remains
    the display source.
    """
    if not records:
        return
    try:
        rows = await view.runtime(_GET_ALL_BETA_PRICING, [])
    except Exception:  # pre-v3 node: method absent from metadata
        return
    pricing = {str(row.get("hotkey")): _chain_pricing_record(row) for row in rows or []}
    for record in records:
        chain = pricing.get(record["hotkey"])
        if chain is not None:
            record["chain_pricing"] = chain


def _summary_record(view, summary: Any) -> dict:
    """Shape one decoded chain ``BasketSummary`` into the read's output record.

    Everything is TAO-denominated (plus the per-subnet alpha holdings), with
    the fund's beta supply alongside: ``beta_price_tao`` is NAV over
    outstanding raw units (τ per beta token, par 1.0 at inception).
    """
    nav = int(summary.get("nav_tao") or 0)
    spot_nav = int(summary.get("spot_nav_tao") or 0)
    deposited = int(summary.get("deposited_tao") or 0)
    redeemed = int(summary.get("redeemed_tao") or 0)
    beta_raw = int(summary.get("shares") or 0)  # chain field name predates the beta branding
    weights = [(int(netuid), int(weight)) for netuid, weight in summary.get("weights") or []]
    weight_total = sum(weight for _, weight in weights)
    return {
        "hotkey": str(summary.get("hotkey")),
        "beta_total_raw": beta_raw,
        "beta_total": beta_raw / _RAW_PER_BETA,
        "beta_price_tao": nav / beta_raw if beta_raw else 0.0,
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
        for hotkey, _beta, payout in rows or []
    ]
    records.sort(key=lambda entry: -entry["owed_tao"].rao)
    return records


def _position_record(view, position: Any) -> dict:
    """Shape one decoded chain ``BasketPosition`` into the read's output record.

    Raw chain integers are kept (``beta_raw`` / ``beta_total_raw``) next to
    display-scaled floats: one beta token is 10^9 raw units, so a beta had a
    par value of exactly τ1 at fund inception and ``beta_price_tao`` tracks
    fund performance from 1.0.
    """
    beta_raw = int(position.get("beta") or 0)
    beta_total_raw = int(position.get("beta_total") or 0)
    nav = int(position.get("nav_tao") or 0)
    return {
        "hotkey": str(position.get("hotkey")),
        "beta_raw": beta_raw,
        "beta_total_raw": beta_total_raw,
        "beta": beta_raw / _RAW_PER_BETA,
        "fund_fraction": beta_raw / beta_total_raw if beta_total_raw else 0.0,
        # τ per beta token == rao per raw unit; par 1.0 at inception.
        "beta_price_tao": nav / beta_total_raw if beta_total_raw else 0.0,
        "nav_tao": view.balance(nav, _ROOT_NETUID),
        "value_tao": view.balance(int(position.get("value_tao") or 0), _ROOT_NETUID),
        "spot_value_tao": view.balance(int(position.get("spot_value_tao") or 0), _ROOT_NETUID),
    }


@read(
    "basket_position",
    {"hotkey_ss58": "string", "coldkey_ss58": "string"},
    category="Staking",
    param_docs={
        "hotkey_ss58": "Validator whose basket the position is in.",
        "coldkey_ss58": "Staker whose position to read.",
    },
)
async def basket_position(view, hotkey_ss58: str, coldkey_ss58: str) -> Optional[dict]:
    """A staker's beta token position in one validator's basket.

    ``beta`` is the staker's beta token balance — the product-facing number:
    it never moves with market prices, only grows as root dividends accrue
    and shrinks when you claim. One beta is 10^9 raw chain units and had a
    par value of τ1 at fund inception, so ``beta_price_tao`` (fund NAV over
    outstanding beta) tracks the fund's performance from 1.0. ``value_tao``
    is the realizable TAO a claim would pay right now — exactly what
    ``claim_root_with_hotkey`` would redeem; ``spot_value_tao`` marks the
    same slice at spot prices (display only). Returns ``None`` when the
    staker holds no beta there.
    """
    position = await view.runtime(
        _GET_BASKET_POSITION, {"hotkey": hotkey_ss58, "coldkey": coldkey_ss58}
    )
    if position is None:
        return None
    return _position_record(view, position)


@read(
    "root_basket_portfolio",
    {"coldkey_ss58": "string"},
    category="Staking",
    param_docs={"coldkey_ss58": "Coldkey whose basket portfolio to list."},
)
async def root_basket_portfolio(view, coldkey_ss58: str) -> list[dict]:
    """A coldkey's basket portfolio: beta tokens held per validator, with values.

    One ``basket_position`` record per validator on which the coldkey holds
    beta, sorted by ``value_tao`` descending. ``beta`` is the stable holding
    count (grows with accruals, shrinks with claims), and ``value_tao`` is
    what claiming would realize right now at current pool depth. Multiply
    ``value_tao`` by an off-chain TAO/USD price for fiat value — the chain
    has no USD oracle.
    """
    rows = await view.runtime(_GET_ROOT_BASKET_PORTFOLIO, [coldkey_ss58])
    records = [_position_record(view, position) for position in rows or []]
    records.sort(key=lambda entry: -entry["value_tao"].rao)
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
    validator's root weight vector, the per-subnet alpha holdings each
    valued at spot and at realizable depth, and `basket_rate` — the
    cumulative β raw units minted per rao of root stake (a lifetime
    accumulator that includes migration-seeded history; subtract a
    period-start rate before valuing a period's yield). All figures are
    TAO (or alpha for the holdings themselves).
    """
    view = await view.at()
    summary = await view.runtime(
        api.BetaBasketRuntimeApi.get_validator_basket_summary, [hotkey_ss58]
    )
    record = _summary_record(view, summary or {})
    rate = await view.query(st.SubtensorModule.BasketRate, [hotkey_ss58])
    record["basket_rate"] = _i96f32_float(rate)
    try:
        row = await view.runtime(_GET_BETA_PRICING, [hotkey_ss58])
    except Exception:  # pre-v3 node: method absent from metadata
        row = None
    if row:
        record["chain_pricing"] = _chain_pricing_record(row)
    return record


@read(
    "root_baskets",
    {},
    category="Staking",
)
async def root_baskets(view) -> list[dict]:
    """Basket summaries for every validator with an active basket.

    The network-wide leaderboard: one `validator_basket_summary` record per
    validator with an active fund, sorted by NAV descending, each including
    its `basket_rate` (the cumulative staker-yield accumulator: β raw units
    minted per rao of root stake, migration-seeded history included).
    Compare `lifetime_return` across validators to rank basket performance.
    """
    view = await view.at()
    rows = await view.runtime(api.BetaBasketRuntimeApi.get_all_validator_baskets, [])
    records = [_summary_record(view, summary) for summary in rows or []]
    if records:
        rates = await view.query_batch(
            st.SubtensorModule.BasketRate,
            [[record["hotkey"]] for record in records],
        )
        for record, rate in zip(records, rates):
            record["basket_rate"] = _i96f32_float(rate)
    await _attach_chain_pricing(view, records)
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
