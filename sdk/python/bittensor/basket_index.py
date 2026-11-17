"""Index-spliced display pricing for validator basket funds.

Raw on-chain beta prices (``nav / beta supply``) carry arbitrary historical
baselines: a fund seeded when pools were cheap shows a high price forever,
and a fund launched yesterday starts at 1.0 regardless of skill. Levels are
therefore not comparable across funds of different ages.

This module fixes that at the display layer, the way a new share class of a
fund launches at the master fund's current NAV rather than at $1:

- The **basket index** is the chained NAV-weighted price of all live funds.
  Flows in and out don't move it; only aggregate fund performance does.
- Live ``vs_index`` marks every fund at **spot** (the same zero-size
  haircut). Realizable NAV — a full-fund dump — is not used for ranking,
  because it punishes large books for pool impact a normal buyer never pays.
- Each fund's **baseline** is a frozen per-fund divisor stamped at first
  sighting so its display price starts exactly at the index level of that
  block. A fund's display price is then ``raw_price / baseline``.

What a display level means, for every fund of every age: the wealth of τ1
invested in the average basket at the epoch block, switched into this fund
at its launch. A mediocre fund sits *on* the index line whether it is three
days or three years old; above the line = beating the market. Live ranking
uses spot τ/β so fund size does not change the mark.

Since runtime API v3 the chain computes this convention itself
(``BetaBasketRuntimeApi.get_all_beta_pricing``): records carrying a
``chain_pricing`` snapshot (attached by the ``root_baskets`` /
``validator_basket_summary`` reads on v3+ nodes) pass the canonical
numbers straight through, and everything below is the *fallback* for
pre-v3 nodes and pre-upgrade historical blocks. The local math should
shrink toward that fallback role, not grow.

Fallback baselines and the index series are frozen data shipped with the
SDK (:mod:`bittensor.basket_index_data`, rebuilt by
``scripts/build_basket_index.py``). A fund not yet in the table gets a
*provisional* baseline pinned to the latest index level: it displays at the
market until the table is regenerated and its real first sighting is frozen.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from ._policy import evaluate_policy
from .basket_index_data import BASELINES, EPOCH_BLOCK, GENERATED_BLOCK, INDEX, TR_INDEX

# Funds below this spot NAV are too shallow to price the live index; they
# still get a display price, but they do not pull the average.
_MIN_INDEX_NAV_TAO = 0.1


def _as_tao(value) -> float:
    """TAO amount from a Balance or a raw number."""
    if value is None:
        return 0.0
    tao = getattr(value, "tao", None)
    if tao is not None:
        return float(tao)
    return float(value)


def spot_beta_price(record: dict) -> float:
    """τ per β at spot — the same mark for every fund, regardless of size.

    Realizable ``beta_price_tao`` is NAV after dumping the whole book. That
    haircut grows with fund size, so a large and a small fund with the same
    bag print different prices. Scaling up to spot NAV removes that.
    """
    return evaluate_policy("financial.basket.spot", {"record": _policy_record(record)})


def _series_level(series: tuple[tuple[int, float], ...], block: Optional[int]) -> float:
    """Most recent sample of a frozen index series at or before ``block``
    (latest sample when omitted); 1.0 before any data."""
    return evaluate_policy("financial.basket.level", {"series": series, "block": block})


def index_level(block: Optional[int] = None) -> float:
    """The basket index level at ``block`` (latest sample when omitted).

    Uses the most recent sample at or before ``block``; the index only moves
    with aggregate fund returns, so half-daily sampling is dense enough for
    display. Level is 1.0 at the epoch block and before any data.
    """
    return _series_level(INDEX, block)


def tr_index_level(block: Optional[int] = None) -> float:
    """The staker total-return index level at ``block`` (latest when omitted).

    The NAV-weighted wealth of τ1 of root stake earning the average fund's
    dividends since the epoch block. Bag prices only enter through the value
    of accrued β, so this moves with dividend flow, not with mix.
    """
    return _series_level(TR_INDEX, block)


@dataclass(frozen=True)
class DisplayPricing:
    """One fund's index-spliced display pricing."""

    baseline: float  # divisor applied to the raw beta price
    first_block: Optional[int]  # first sighting on the sample grid; None if provisional
    provisional: bool  # True until the baseline table is regenerated with this fund
    baseline_rate: float  # the fund's BasketRate at first sighting


def fund_pricing(hotkey: str, raw_price: float) -> DisplayPricing:
    """The frozen baseline for ``hotkey``, or a provisional one pinned to the index.

    ``raw_price`` (fund NAV over outstanding raw units, τ per beta token)
    is only used for the provisional fallback, which anchors an unknown fund
    at the latest index level until the table is rebuilt. A provisional
    fund's baseline rate is 0.0: a genuinely new fund starts there, and the
    real first-sighting rate is frozen on the next table rebuild.
    """
    return DisplayPricing(
        **evaluate_policy(
            "financial.basket.pricing", {"data": _policy_data(), "hotkey": hotkey, "raw": raw_price}
        )
    )


def staker_yield(record: dict) -> Optional[float]:
    """Pending-entitlement mark: what the β minted to stakers on this fund
    since its first sighting is worth now, per τ1 staked.

    ``(basket_rate − rate at first sighting) × spot τ/β``: the β entitlement
    minted per τ staked over the tracked period, valued at today's price.
    This is a *mark*, not a return series — it re-values with pool prices
    even when no dividend lands; the canonical staker-return series is the
    chain's ``staker_twr``. Subtracting the frozen baseline rate matters for
    migrated funds, whose ``BasketRate`` was seeded with legacy pre-period
    history that no current-period staker earned. ``None`` when the record
    carries no ``basket_rate``.
    """
    return evaluate_policy(
        "financial.basket.yield", {"data": _policy_data(), "record": _policy_record(record)}
    )


def stake_value(record: dict) -> Optional[float]:
    """The fund's total-return stake price: the wealth of τ1 staked here.

    Local *approximation* for pre-v3 nodes and pre-upgrade history:
    ``(1 + staker_yield) × TR-index level at first sighting``. On v3+ nodes
    the chain's ``stake_price`` is the canonical series — the ``BasketTwr``
    accumulator (each dividend locked at deposit-time pricing) spliced onto
    the stake index — and records carrying ``chain_pricing`` pass it through
    instead of calling this. The accumulator does not exist at historical
    blocks, so this fallback estimates it from the primitives that do:
    the rate delta marked at today's spot rate. ``None`` when the record
    carries no ``basket_rate``.
    """
    return evaluate_policy(
        "financial.basket.stake_value", {"data": _policy_data(), "record": _policy_record(record)}
    )


def _apply_chain_pricing(record: dict, chain: dict) -> None:
    """Write display fields from the chain's standardized pricing snapshot.

    Field-for-field the same outputs as :func:`_apply_pricing`, but computed
    by the runtime — the canonical convention every consumer shares. The
    divisor implied by the chain (``spot_price / display_price``) rescales a
    position's beta balance the same way the local baseline would, so
    ``display_beta * display_price_tao`` still equals the position's value.
    """
    record.update(
        evaluate_policy(
            "financial.basket.normalize",
            {"data": _policy_data(), "record": {**_policy_record(record), "chain_pricing": chain}},
        )
    )


def _apply_pricing(record: dict, raw_price: float, level: float, tr_level: float) -> DisplayPricing:
    """Write display fields on ``record`` against the index levels; return the pricing."""
    record.update(
        evaluate_policy(
            "financial.basket.apply",
            {
                "data": _policy_data(),
                "record": _policy_record(record),
                "level": level,
                "tr_level": tr_level,
            },
        )
    )
    return fund_pricing(record["hotkey"], raw_price)


def normalize_position(record: dict) -> dict:
    """Add index-spliced display fields to a basket read record.

    Prices the fund at spot when ``spot_nav_tao`` is present, so a large book
    and a small book with the same holdings print the same τ/β. ``vs_index``
    uses the frozen sample index; prefer :func:`normalize_positions` when a
    full board is in hand so the live average is also spot-marked.

    Adds, in place and returned:

    - ``display_price_tao``: index-spliced beta price (comparable across funds).
    - ``display_beta``: beta balance rescaled so
      ``display_beta * display_price_tao`` still equals ``value_tao``.
    - ``vs_index``: display price over the index level, minus one —
      the fund's cumulative out/under-performance vs. the average basket.
    - ``index_first_block``: the fund's first sighting on the sample grid
      (``None`` while provisional).
    - ``index_provisional``: True when the fund has no frozen baseline yet.
    - ``basket_index``: the index level used for ``vs_index``.
    - ``staker_yield``: the pending-entitlement mark — what the β minted per
      τ of root stake since the fund's first sighting is worth today (see
      :func:`staker_yield`), or ``None`` when the record carries no
      ``basket_rate``. Root-staker dividend pipe only (allocating — buying β
      — does not earn it), and a mark, not a return series.
    - ``stake_value`` / ``stake_vs_index`` / ``stake_index``: the fund's
      total-return stake price (see :func:`stake_value`), its out/under-
      performance vs the total-return index, and the level used. For window
      returns use ``staker_twr`` ratios — ``stake_value`` is the same series
      on v3+ nodes.

    A record carrying a ``chain_pricing`` snapshot (v3+ node) passes the
    chain's canonical numbers straight through instead of computing locally.
    """
    record.update(
        evaluate_policy(
            "financial.basket.normalize", {"data": _policy_data(), "record": _policy_record(record)}
        )
    )
    return record


def normalize_positions(records: list[dict]) -> float:
    """Normalize a full board against live spot-weighted indexes.

    Every fund is marked at spot (same haircut). The bag index is the
    spot-NAV weighted average of the display prices, and the total-return
    index the same average of the stake values, both excluding dust.
    Returns the bag level and writes the same fields as
    :func:`normalize_position` on each record, including ``basket_index``
    and ``stake_index``.

    Chain-first: when records carry the runtime's standardized pricing
    (``chain_pricing``, attached by the reads on v3+ nodes), every number —
    prices, yields, and both index levels — is a pass-through of the chain's
    one sweep, so btcli shows exactly what every other consumer shows. The
    local math below is the fallback for older nodes and pre-upgrade
    historical blocks.
    """
    result = evaluate_policy(
        "financial.basket.normalize_many",
        {"data": _policy_data(), "records": [_policy_record(record) for record in records]},
    )
    for record, values in zip(records, result["records"]):
        record.update(values)
    return result["level"]


def age_days(first_block: Optional[int], current_block: int) -> Optional[float]:
    """Days since a fund's first sighting (12s blocks); ``None`` when unknown."""
    return evaluate_policy("financial.basket.age", {"first": first_block, "current": current_block})


__all__ = [
    "EPOCH_BLOCK",
    "GENERATED_BLOCK",
    "DisplayPricing",
    "age_days",
    "fund_pricing",
    "index_level",
    "normalize_position",
    "normalize_positions",
    "spot_beta_price",
    "stake_value",
    "staker_yield",
    "tr_index_level",
]


def _policy_record(record):
    return {
        key: _as_tao(value) if key in {"nav_tao", "spot_nav_tao"} else value
        for key, value in record.items()
        if key
        in {
            "hotkey",
            "nav_tao",
            "spot_nav_tao",
            "beta_price_tao",
            "beta",
            "basket_rate",
            "chain_pricing",
        }
    }


def _policy_data():
    return {"baselines": BASELINES, "series": INDEX, "tr_series": TR_INDEX}
