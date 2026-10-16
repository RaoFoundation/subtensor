"""Property-based tests for the Balance money type.

Balance is the unit-safety boundary for real money: these invariants
(exactness, closure, unit segregation) must hold for *all* amounts, not just
the ones example tests happen to pick — which is what hypothesis explores.
"""

from __future__ import annotations

from decimal import ROUND_DOWN, ROUND_HALF_EVEN, ROUND_UP, Decimal, localcontext

import pytest
from hypothesis import given
from hypothesis import strategies as st

from bittensor.balance import Balance, UnitMismatchError, tao

# Chain amounts are u64 rao in practice; give headroom beyond that anyway.
RAO = st.integers(min_value=0, max_value=2**80)
SIGNED_RAO = st.integers(min_value=-(2**80), max_value=2**80)
NETUID = st.integers(min_value=0, max_value=4095)
SUBNET = st.integers(min_value=1, max_value=4095)


@given(rao=SIGNED_RAO, netuid=NETUID)
def test_rao_roundtrip_is_exact(rao: int, netuid: int):
    assert Balance.from_rao(rao, netuid).rao == rao


@given(rao=RAO)
def test_decimal_serialization_roundtrip_is_exact(rao: int):
    """rao -> exact decimal string -> rao loses nothing (the intent
    serialization path for money fields)."""
    balance = Balance.from_rao(rao)
    encoded = format(balance.decimal, "f")
    assert Balance.from_tao(encoded).rao == rao


@given(rao=RAO)
def test_from_tao_decimal_string_is_exact_where_float_is_not(rao: int):
    exact = Decimal(rao) / 10**9
    assert Balance.from_tao(str(exact)).rao == rao


@given(a=SIGNED_RAO, b=SIGNED_RAO, netuid=NETUID)
def test_addition_is_integer_arithmetic_on_rao(a: int, b: int, netuid: int):
    left, right = Balance.from_rao(a, netuid), Balance.from_rao(b, netuid)
    total = left + right
    assert total.rao == a + b
    assert total.netuid == netuid
    assert (total - right).rao == a


@given(a=RAO, netuid_a=NETUID, netuid_b=NETUID)
def test_cross_unit_arithmetic_always_raises(a: int, netuid_a: int, netuid_b: int):
    if netuid_a == netuid_b:
        return
    x, y = Balance.from_rao(a, netuid_a), Balance.from_rao(a, netuid_b)
    with pytest.raises(UnitMismatchError):
        _ = x + y
    with pytest.raises(UnitMismatchError):
        _ = x - y
    with pytest.raises(UnitMismatchError):
        _ = x < y
    # Equality never raises — different currency is simply not equal.
    assert x != y


@given(a=SIGNED_RAO, b=SIGNED_RAO, netuid=NETUID)
def test_ordering_matches_rao(a: int, b: int, netuid: int):
    x, y = Balance.from_rao(a, netuid), Balance.from_rao(b, netuid)
    assert (x < y) == (a < b)
    assert (x <= y) == (a <= b)
    assert (x == y) == (a == b)


@given(rao=SIGNED_RAO, netuid=NETUID)
def test_eq_hash_consistency(rao: int, netuid: int):
    x, y = Balance.from_rao(rao, netuid), Balance.from_rao(rao, netuid)
    assert x == y
    assert hash(x) == hash(y)
    assert x != rao  # raw ints never compare equal (currency-blind)
    assert (-x).rao == -rao


@given(rao=RAO, netuid=SUBNET)
def test_unit_accessors_are_segregated(rao: int, netuid: int):
    alpha_balance = Balance.from_rao(rao, netuid)
    tao_balance = Balance.from_rao(rao, 0)
    with pytest.raises(UnitMismatchError):
        _ = alpha_balance.tao
    with pytest.raises(UnitMismatchError):
        _ = tao_balance.alpha
    assert alpha_balance.alpha == tao_balance.tao  # same rao, own currency


@given(rao=RAO)
def test_float_comparison_is_rejected(rao: int):
    balance = Balance.from_rao(rao)
    with pytest.raises(TypeError):
        _ = balance > 0.5
    with pytest.raises(TypeError):
        _ = balance > True
    # The documented alternative works.
    assert (balance > tao(0.5)) == (rao > 500_000_000)


@given(rao=RAO, netuid=SUBNET)
def test_from_alpha_rejects_tao_netuid(rao: int, netuid: int):
    amount = Decimal(rao) / 10**9
    assert Balance.from_alpha(str(amount), netuid).netuid == netuid
    with pytest.raises(UnitMismatchError):
        Balance.from_alpha(str(amount), 0)


@pytest.mark.parametrize(
    ("netuid", "symbol", "prefix", "suffix"),
    [(0, None, "τ", ""), (42, None, "", "α₄₂"), (2, "β", "", "β")],
)
@pytest.mark.parametrize(
    ("rao", "amount"),
    [
        (0, "0.000000000"),
        (1, "0.000000001"),
        (-1, "-0.000000001"),
        (10**9, "1.000000000"),
        (-(10**9 + 1), "-1.000000001"),
        (16_777_216_000_000_001, "16,777,216.000000001"),
        (2**64 - 1, "18,446,744,073.709551615"),
        (-(2**64 - 1), "-18,446,744,073.709551615"),
    ],
)
def test_balance_display_preserves_every_rao(rao, amount, netuid, symbol, prefix, suffix):
    balance = Balance.from_rao(rao, netuid, symbol)
    # Display must not depend on the caller's decimal precision either.
    with localcontext() as ctx:
        ctx.prec = 6
        assert str(balance) == f"{prefix}{amount}{suffix}"
        assert repr(balance) == str(balance)


@given(rao=SIGNED_RAO)
def test_balance_display_roundtrip_is_exact(rao: int):
    rendered = str(Balance.from_rao(rao)).removeprefix("τ").replace(",", "")
    assert Decimal(rendered) == Decimal(f"{rao}e-9")


@given(
    rao=st.integers(min_value=-(2**128), max_value=2**128),
    precision=st.sampled_from([1, 6, 28]),
    netuid=NETUID,
)
def test_decimal_accessor_ignores_context_precision(rao: int, precision: int, netuid: int):
    with localcontext() as ctx:
        ctx.prec = precision
        assert Balance.from_rao(rao, netuid).decimal == Decimal(f"{rao}e-9")
        assert ctx.prec == precision


@given(
    rao=st.integers(min_value=-(2**128), max_value=2**128),
    precision=st.sampled_from([1, 6, 28]),
    netuid=NETUID,
)
def test_decimal_input_ignores_context_precision(rao: int, precision: int, netuid: int):
    amount = Decimal(f"{rao}e-9")
    with localcontext() as ctx:
        ctx.prec = precision
        assert Balance.from_amount(amount, netuid).rao == rao
        assert Balance.from_amount(str(amount), netuid).rao == rao
        assert ctx.prec == precision


@pytest.mark.parametrize(
    ("rounding", "amount", "expected"),
    [
        (ROUND_HALF_EVEN, "1.2345678905", 1_234_567_890),
        (ROUND_HALF_EVEN, "1.2345678915", 1_234_567_892),
        (ROUND_DOWN, "1.2345678909", 1_234_567_890),
        (ROUND_UP, "1.2345678901", 1_234_567_891),
        (ROUND_HALF_EVEN, "-1.2345678915", -1_234_567_892),
        (ROUND_DOWN, "-1.2345678909", -1_234_567_890),
        (ROUND_UP, "-1.2345678901", -1_234_567_891),
    ],
)
def test_fractional_rao_rounds_once_with_callers_rounding_mode(rounding, amount, expected):
    with localcontext() as ctx:
        ctx.prec = 6
        ctx.rounding = rounding
        assert Balance.from_tao(amount).rao == expected
