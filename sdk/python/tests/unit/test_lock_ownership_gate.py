"""The SDK's ownership-gate predicate must mirror the runtime exactly.

The runtime admits a takeover only when the leading hotkey's own conviction is
strictly more than 18% of eligible alpha (integer form:
``conviction * 100 > eligible_alpha * 18``). These tests pin the strict
boundary — the same 1,799/1,800/1,801 cases as the pallet's boundary test —
and prove the crossing-time search uses the strict predicate too.
"""

from bittensor.reads.locks import (
    _blocks_until_conviction,
    _clears_ownership_gate,
    _conviction_at,
)

_ELIGIBLE = 10_000
_RATE = 1_000.0


def test_gate_is_strictly_above_18_percent() -> None:
    assert not _clears_ownership_gate(1_799, _ELIGIBLE)
    assert not _clears_ownership_gate(1_800, _ELIGIBLE)  # exactly 18% is not enough
    assert _clears_ownership_gate(1_801, _ELIGIBLE)


def test_conviction_pinned_at_exactly_18_percent_never_clears() -> None:
    # A perpetual owner bucket holds conviction equal to its locked mass
    # forever, so a mass of exactly 18% of eligible alpha sits on the
    # boundary at every block. The old >= comparison reported 0 here.
    boundary_bucket = [(1_800.0, 1_800.0, True, True)]
    assert _blocks_until_conviction(boundary_bucket, _ELIGIBLE, _RATE, _RATE) is None

    above_bucket = [(1_801.0, 1_801.0, True, True)]
    assert _blocks_until_conviction(above_bucket, _ELIGIBLE, _RATE, _RATE) == 0


def test_crossing_block_is_first_strict_clearance() -> None:
    # A maturing perpetual lock (conviction grows toward its mass) crosses the
    # gate at some future block; the block returned must be the first one that
    # strictly clears the gate.
    buckets = [(3_600.0, 0.0, True, False)]
    crossing = _blocks_until_conviction(buckets, _ELIGIBLE, _RATE, _RATE)
    assert crossing is not None and crossing > 0
    assert _clears_ownership_gate(_conviction_at(buckets, crossing, _RATE, _RATE), _ELIGIBLE)
    assert not _clears_ownership_gate(
        _conviction_at(buckets, crossing - 1, _RATE, _RATE), _ELIGIBLE
    )


def test_zero_eligible_alpha_never_clears() -> None:
    assert _blocks_until_conviction([(500.0, 500.0, True, True)], 0, _RATE, _RATE) is None
