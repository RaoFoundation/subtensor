"""CLI raise must quote a sale floor even when --sell is omitted.

Skeptic fid:abef2402 — the runtime defaults an omitted sell to the newly
escrowed alpha, so leaving min_tao_out at 0 would run that sale unprotected.
"""

from __future__ import annotations

from bittensor.balance import Balance
from bittensor.cli.commands.lending import _bound, _raise_sale_needs_quoted_floor


def test_raise_sale_needs_floor_when_min_tao_out_unset_even_without_explicit_sell():
    unset = Balance.from_rao(0, 0)
    assert _raise_sale_needs_quoted_floor(unset) is True


def test_raise_sale_skips_floor_when_min_tao_out_explicit():
    explicit = Balance.from_rao(1_000_000_000, 0)
    assert _raise_sale_needs_quoted_floor(explicit) is False


def test_bound_at_zero_slippage_is_exact_quoted_floor():
    quoted = Balance.from_rao(10_000_000_000, 0)
    assert _bound(quoted, 0.0, ceiling=False).rao == quoted.rao


def test_bound_at_one_percent_floors_down():
    quoted = Balance.from_rao(10_000_000_000, 0)
    floored = _bound(quoted, 1.0, ceiling=False)
    assert floored.rao == 9_900_000_000
