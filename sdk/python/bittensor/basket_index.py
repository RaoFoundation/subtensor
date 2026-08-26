"""Index-spliced display pricing for validator basket funds.

Raw on-chain beta prices (``nav / beta supply``) carry arbitrary historical
baselines: a fund seeded when pools were cheap shows a high price forever,
and a fund launched yesterday starts at 1.0 regardless of skill. Levels are
therefore not comparable across funds of different ages.

This module fixes that at the display layer, the way a new share class of a
fund launches at the master fund's current NAV rather than at $1:

- The **basket index** is the chained NAV-weighted price of all live funds.
  Flows in and out don't move it; only aggregate fund performance does.
- Each fund's **baseline** is a frozen per-fund divisor stamped at first
  sighting so its display price starts exactly at the index level of that
  block. A fund's display price is then ``raw_price / baseline``.

What a display level means, for every fund of every age: the wealth of τ1
invested in the average basket at the epoch block, switched into this fund
at its launch. A mediocre fund sits *on* the index line whether it is three
days or three years old; above the line = beating the market. Beta balances
are rescaled inversely (``beta * baseline``) so
``display_beta * display_price = value_tao`` still holds exactly.

Baselines and the index series are frozen data shipped with the SDK
(:mod:`bittensor.basket_index_data`, rebuilt by
``scripts/build_basket_index.py``). A fund not yet in the table gets a
*provisional* baseline pinned to the latest index level: it displays at the
market until the table is regenerated and its real first sighting is frozen.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from .basket_index_data import BASELINES, EPOCH_BLOCK, GENERATED_BLOCK, INDEX


def index_level(block: Optional[int] = None) -> float:
    """The basket index level at ``block`` (latest sample when omitted).

    Uses the most recent sample at or before ``block``; the index only moves
    with aggregate fund returns, so half-daily sampling is dense enough for
    display. Level is 1.0 at the epoch block and before any data.
    """
    if not INDEX:
        return 1.0
    if block is None:
        return INDEX[-1][1]
    level = 1.0
    for sample_block, sample_level in INDEX:
        if sample_block > block:
            break
        level = sample_level
    return level


@dataclass(frozen=True)
class DisplayPricing:
    """One fund's index-spliced display pricing."""

    baseline: float  # divisor applied to the raw beta price
    first_block: Optional[int]  # first sighting on the sample grid; None if provisional
    provisional: bool  # True until the baseline table is regenerated with this fund


def fund_pricing(hotkey: str, raw_price: float) -> DisplayPricing:
    """The frozen baseline for ``hotkey``, or a provisional one pinned to the index.

    ``raw_price`` (fund NAV over outstanding raw units, τ per beta token)
    is only used for the provisional fallback, which anchors an unknown fund
    at the latest index level until the table is rebuilt.
    """
    frozen = BASELINES.get(hotkey)
    if frozen is not None:
        baseline, first_block = frozen
        return DisplayPricing(baseline=baseline, first_block=first_block, provisional=False)
    level = index_level()
    baseline = raw_price / level if level else raw_price
    return DisplayPricing(baseline=baseline or 1.0, first_block=None, provisional=True)


def normalize_position(record: dict) -> dict:
    """Add index-spliced display fields to a basket read record.

    Works on any record carrying ``beta_price_tao`` (and optionally ``beta``).
    Adds, in place and returned:

    - ``display_price_tao``: index-spliced beta price (comparable across funds).
    - ``display_beta``: beta balance rescaled so
      ``display_beta * display_price_tao`` still equals ``value_tao``.
    - ``vs_index``: display price over the current index level, minus one —
      the fund's cumulative out/under-performance vs. the average basket.
    - ``index_first_block``: the fund's first sighting on the sample grid
      (``None`` while provisional).
    - ``index_provisional``: True when the fund has no frozen baseline yet.
    """
    pricing = fund_pricing(record["hotkey"], record["beta_price_tao"])
    level = index_level()
    display_price = record["beta_price_tao"] / pricing.baseline
    record["display_price_tao"] = display_price
    if "beta" in record:
        record["display_beta"] = record["beta"] * pricing.baseline
    record["vs_index"] = display_price / level - 1.0 if level else 0.0
    record["index_first_block"] = pricing.first_block
    record["index_provisional"] = pricing.provisional
    return record


def age_days(first_block: Optional[int], current_block: int) -> Optional[float]:
    """Days since a fund's first sighting (12s blocks); ``None`` when unknown."""
    if first_block is None:
        return None
    return (current_block - first_block) / 7200


__all__ = [
    "EPOCH_BLOCK",
    "GENERATED_BLOCK",
    "DisplayPricing",
    "age_days",
    "fund_pricing",
    "index_level",
    "normalize_position",
]
