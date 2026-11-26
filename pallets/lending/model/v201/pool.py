"""Integer constant-product pool (all live finney pools are 50/50 balancer pools), fee 33/65535 on input, fee kept in reserves.
Rounding always favours the pool: outputs floor, required inputs ceil."""
from math import isqrt
from .params import Q64, FEE_NUM, FEE_DEN


def cdiv(a, b):
    return -((-a) // b)


def fee_of(amount):
    return cdiv(amount * FEE_NUM, FEE_DEN)


def out_alpha_for_tao(T, X, tin):
    eff = tin - fee_of(tin)
    return X * eff // (T + eff)


def out_tao_for_alpha(T, X, ain):
    eff = ain - fee_of(ain)
    return T * eff // (X + eff)


def tao_in_for_alpha(T, X, a):
    """Smallest TAO input whose swap returns >= a alpha. None if impossible (a >= X)."""
    if a <= 0:
        return 0
    if a >= X:
        return None
    eff = cdiv(a * T, X - a)
    tin = cdiv(eff * FEE_DEN, FEE_DEN - FEE_NUM)
    while out_alpha_for_tao(T, X, tin) < a:
        tin += 1
    while tin > 0 and out_alpha_for_tao(T, X, tin - 1) >= a:
        tin -= 1
    return tin


def buy_cost_nofee(T, X, a):
    """Fee-free TAO to buy a alpha (spec health: `buy_cost_on`, live pool). Ceil. None if a >= X."""
    if a <= 0:
        return 0
    if a >= X:
        return None
    return cdiv(a * T, X - a)


def sell_proceeds_nofee(T, X, a):
    """Fee-free TAO for selling a alpha (spec long health). Floor."""
    return T * a // (X + a) if a > 0 else 0


def price(T, X):
    return T * Q64 // X if X > 0 else 0


def pumped(T, X, p_fp):
    """Hypothetical pool after buying until spot == p_fp (no fee, k preserved). Unchanged if already >= p_fp."""
    if price(T, X) >= p_fp:
        return T, X
    k = T * X
    Xs = isqrt(k * Q64 // p_fp)
    Xs = max(Xs, 1)
    return cdiv(k, Xs), Xs


def dumped(T, X, p_fp):
    """Hypothetical pool after selling until spot == p_fp (no fee, k preserved). Unchanged if already <= p_fp."""
    if p_fp <= 0 or price(T, X) <= p_fp:
        return T, X
    k = T * X
    Xs = cdiv(isqrt(k * Q64 // p_fp), 1)
    return k // Xs, Xs


def alpha_for_up_impact(T, X, p_cap):
    """Largest alpha a bought by one swap such that the post-swap spot <= p_cap."""
    lo, hi = 0, X - 1
    while lo < hi:
        mid = (lo + hi + 1) // 2
        c = tao_in_for_alpha(T, X, mid)
        if c is not None and price(T + c - fee_of(c), X - mid) <= p_cap:
            lo = mid
        else:
            hi = mid - 1
    return lo
