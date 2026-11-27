"""Golden vectors for the v2.1 pure core (pallets/lending/src/math), generated from model/v21.

Writes ../src/tests/golden/v21.json (or the path given as the first argument) with three sections:

  rate      curve_bps(apr, k, cap, num, den) (SPEC 1 item 2): the 1344 base vectors of the spec's evidence run
            plus edge rows (cap reached at each step, u up to 100 on the launch curve, small and wide operands).
            Each row also has a floor and rate = rate_bps(apr, k, cap, floor, num, den), the curve lifted to
            the floor for a position with debt and holdings.
  coverage  short coverage need with the v2.1 reference max(spot, SMA30_rank) (SPEC P2); the TWAP median
            is in each row only to show that it does not move the need.
  accrual   due and remainder after n blocks at the rate, for a short (num = floor(q x TWAP_median),
            den = C + P) and a long (num = D, den = floor(A x TWAP_median) + C).

Every row is computed twice: by the formulas written out in this file and by v21.core. The script fails on
any mismatch and prints the row counts. Deterministic (seeded).

The minimum-rate floor (rate_floor_bps, an approved addition beyond SPEC-v2.1) is not in the model: v21.core
computes the curve. The curve of every rate and accrual row is checked against v21.core, and so is the
accrual at the curve rate; the floor is applied on top by the formula in this file, and the accrual rows
where it does not bind are checked against the model's accrual as they are.

Usage: cd pallets/lending/model && python3 gen_golden_v21.py [OUT.json]
Prices are U64F64 bit patterns (rao TAO per rao alpha << 64) and amounts are rao integers, both as decimal
strings; rates, exponents and block counts are JSON numbers."""
import dataclasses
import itertools
import json
import os
import random
import sys

import v21.core as core
from v21 import pool as pm
from v21.params import BPS, BPY, RAO, Q64, LongParams, ModelSwitches, SynthParams

SEED = 20261127
rng = random.Random(SEED)
floor_rng = random.Random(SEED + 1)     # its own stream: the floors do not move the other inputs
S = str
U64 = 2**64 - 1
U128 = 2**128 - 1
ACCRUAL_DEN = BPS * BPY
# The spec's launch curve (SPEC 2.1; k and cap are provisional, SPEC 10.3). The model's own defaults differ
# (k = 4, cap = 200,000), so every row carries its parameters.
LAUNCH = (2_000, 2, 10_000)
# The launch floor (provisional, pending model results): the launch base APR.
LAUNCH_FLOOR = 2_000


# ---------------------------------------------------------------------------- formulas (SPEC 1 item 2)
def curve_bps(apr, k, cap, num, den):
    if num <= 0:
        return 0
    if den <= 0:
        return cap
    r = apr
    for _ in range(k):
        r = r * num // den
        if r >= cap:
            return cap
    return min(r, cap)


def rate_bps(apr, k, cap, floor, num, den):
    """The curve lifted to the floor (1 <= floor <= cap). No debt pays 0 and no holdings pay the cap."""
    if num <= 0:
        return 0
    if den <= 0:
        return cap
    return max(floor, curve_bps(apr, k, cap, num, den))


def pick_floor(apr, cap):
    """A floor within its bounds 1..cap: the least, the base APR, the cap or any value."""
    return floor_rng.choice((1, 1, apr, cap, LAUNCH_FLOOR if LAUNCH_FLOOR <= cap else 1, floor_rng.randrange(1, cap + 1)))


def accrue(notional, rate, blocks, rem):
    num = notional * rate * blocks + rem
    return num // ACCRUAL_DEN, num % ACCRUAL_DEN


def short_need(sp, q, Q, T, X, spot, sma):
    """SPEC P2 with ref = max(spot, SMA30_rank)."""
    if Q <= 0:
        return 0
    s_bps = sp.stress_bps * (BPS * X + sp.stress_slope_bps * Q) // (BPS * X)
    tgt = max(spot, sma) * s_bps // BPS
    Tp, Xp = pm.pumped(T, X, tgt)
    cost = pm.tao_in_for_alpha(Tp, Xp, Q)
    return None if cost is None else pm.cdiv(cost * q, Q)


# ---------------------------------------------------------------------------- the model
def model(apr, k, cap, T=1_000 * RAO, X=480_000 * RAO, twap=None):
    sp = dataclasses.replace(SynthParams(), apr_bps=apr, rate_k=k, rate_cap_bps=cap)
    sn = core.Subnet(T, X, X, sp=sp, lp=LongParams(), sw=ModelSwitches())
    if twap is not None:
        sn.twap = [twap] * sp.twap_samples
    return sn


# ---------------------------------------------------------------------------- rate rows
def rate_inputs():
    rows = []
    # The spec's evidence set (results_v21/rate_golden.json): 1344 vectors.
    base = ((0, 10**9), (10**9, 0), (1, 1), (3 * 10**8, 10**9), (10**9, 10**9), (10**9 + 1, 10**9),
            (19 * 10**8, 10**9), (5 * 10**9, 10**9), (10**11, 10**9), (U64, 1), (U64, U64),
            (12345678901, 98765432109))
    for apr, k, cap in itertools.product((1, 2_000, 10_000), range(1, 9), (2_000, 50_000, 73_000, 200_000, 1_000_000)):
        if cap < apr:
            continue
        rows += [(apr, k, cap, num, den) for num, den in base]
    assert len(rows) == 1344
    # Zero operands for every exponent, including both zero (num = 0 wins).
    for k in range(1, 9):
        rows += [(2_000, k, 10_000, 0, 0), (2_000, k, 10_000, 0, 1), (2_000, k, 10_000, 1, 0),
                 (2_000, k, 10_000, 0, U128), (2_000, k, 10_000, U128, 0)]
    # The cap reached exactly at step j of k, and missed by one at that step.
    den = 10**9
    for k, (un, ud) in itertools.product(range(1, 9), ((3, 2), (2, 1), (5, 2), (10, 1), (100, 1))):
        num, r = den * un // ud, 2_000
        for _ in range(k):
            r = r * num // den
            for cap in (r, r + 1):
                if 2_000 <= cap <= 1_000_000:
                    rows.append((2_000, k, cap, num, den))
    # The launch curve over u = 0..100 (fine steps up to 3, coarse above).
    apr, k, cap = LAUNCH
    for i in range(0, 301):
        rows.append((apr, k, cap, i * 10**7, 10**9))
    for i in range(13, 401):
        rows.append((apr, k, cap, i * 25 * 10**7, 10**9))
    # Every exponent at u = 100, and at u just under and over 1.
    for k in range(1, 9):
        rows += [(2_000, k, 1_000_000, 100 * 10**9, 10**9), (2_000, k, 1_000_000, 10**9 - 1, 10**9),
                 (2_000, k, 1_000_000, 10**9 + 1, 10**9), (10_000, k, 1_000_000, 100, 1)]
    # Small operands, where each floor matters.
    for _ in range(300):
        apr = rng.choice((1, 7, 2_000, 10_000))
        rows.append((apr, rng.randrange(1, 9), rng.randrange(apr, 1_000_001), rng.randrange(0, 25), rng.randrange(0, 25)))
    # Random u64 operands with u up to 100.
    for _ in range(500):
        apr = rng.randrange(1, 10_001)
        den = rng.randrange(1, 2**63)
        num = min(U64, den * rng.randrange(0, 100_000) // 1_000)
        rows.append((apr, rng.randrange(1, 9), rng.randrange(apr, 1_000_001), num, den))
    # Operands at the top of u128: no step may overflow.
    for k in range(1, 9):
        rows += [(10_000, k, 1_000_000, U128, U128), (10_000, k, 1_000_000, U128, 2**127),
                 (10_000, k, 1_000_000, U128 - 1, U128), (10_000, k, 1_000_000, U128, U64),
                 (2_000, k, 10_000, 2**127 + 12345, 2**126 + 999)]
    return rows


def rate_rows():
    rows, bad = [], 0
    for apr, k, cap, num, den in rate_inputs():
        curve = curve_bps(apr, k, cap, num, den)
        bad += model(apr, k, cap).rate_from(num, den) != curve
        floor = pick_floor(apr, cap)
        rate = rate_bps(apr, k, cap, floor, num, den)
        bad += not (rate == curve or (rate == floor > curve and num > 0 and den > 0))
        bad += num > 0 and not floor <= rate <= cap
        rows.append(dict(apr=apr, k=k, cap=cap, floor=floor, num=S(num), den=S(den), curve=curve, rate=rate))
    return rows, bad


# ---------------------------------------------------------------------------- short coverage rows
def pools(n):
    out = [(1_000 * RAO, 480_000 * RAO), (5_000 * RAO, 2_400_000 * RAO), (25_000 * RAO, 9_000_000 * RAO),
           (1_000 * RAO + 1, 1_000 * RAO + 7), (123_456_789_012, 98_765_432_109_876)]
    while len(out) < n:
        T = rng.randrange(100 * RAO, 200_000 * RAO)
        X = T * rng.randrange(50, 5_000) // 10 + rng.randrange(1, 10**9)
        out.append((T, X))
    return out


def coverage_rows():
    rows, bad = [], 0
    for T, X in pools(30):
        sn = model(*LAUNCH, T=T, X=X)
        spot = sn.spot()
        for qf, Qf in ((1, 1), (10, 100), (50, 180), (100, 300), (200, 600), (1, 900)):
            Q = X * Qf // 1000
            q = Q * qf // max(Qf, 1)
            sma = spot * rng.randrange(7_000, 14_000) // BPS
            # A TWAP median above, between and below the other two: it must not move the need.
            for twap in (max(spot, sma) * rng.randrange(10_500, 30_000) // BPS,
                         (spot + sma) // 2,
                         min(spot, sma) * rng.randrange(3_000, 9_500) // BPS):
                need = short_need(sn.sp, q, Q, T, X, spot, sma)
                bad += sn.short_need(q, Q, T, X, spot, twap, sma) != need
                bad += sn.short_cov_ref(spot, twap, sma) != max(spot, sma)
                rows.append(dict(tao=S(T), alpha=S(X), spot=S(spot), twap=S(twap), sma=S(sma),
                                 q=S(q), book=S(Q), need=None if need is None else S(need)))
    return rows, bad


# ---------------------------------------------------------------------------- accrual rows
def curve_params():
    if rng.randrange(3) == 0:
        return LAUNCH
    apr = rng.choice((1, 500, 2_000, 10_000))
    return apr, rng.randrange(1, 9), rng.choice((apr, 10_000, 73_000, 200_000, 1_000_000))


ACCRUAL_FLOORED = []     # accrual rows where the floor binds


def accrual_rows():
    rows, bad = [], 0
    for i in range(400):
        short = i % 2 == 0
        apr, k, cap = curve_params()
        twap = Q64 * rng.randrange(1, 5_000_000) // 1_000_000_000          # 1e-9 .. 5e-3 TAO per alpha
        blocks = rng.choice((0, 1, 360, 7_200, rng.randrange(0, 50_400)))
        rem = rng.randrange(0, ACCRUAL_DEN)
        due = rng.choice((0, 0, rng.randrange(0, 10**9)))
        sn = model(apr, k, cap, twap=twap)
        edge = i % 20                                                      # 0,1: num = 0; 2,3: den = 0
        if short:
            q = 0 if edge == 0 else rng.randrange(1, 10**15)
            notional = q * twap >> 64
            held = 0 if edge == 2 else max(1, notional * rng.randrange(200, 30_000) // 10_000)
            proceeds = rng.randrange(0, held + 1)
            collateral = held - proceeds
            num, den = notional, collateral + proceeds
            pos = core.Short('a', q, collateral, proceeds, 0, 0, 0, 0, 0, due=due, rem=rem)
            legs = dict(q=S(q), collateral=S(collateral), proceeds=S(proceeds))
        else:
            debt = 0 if edge == 1 else rng.randrange(1, 10**13)
            held = 0 if edge == 3 else max(1, debt * rng.randrange(200, 30_000) // 10_000)
            alpha_value = rng.randrange(0, held + 1)
            alpha = alpha_value * Q64 // twap
            collateral = held - (alpha * twap >> 64)
            notional = debt
            num, den = debt, (alpha * twap >> 64) + collateral
            pos = core.Long('a', debt, alpha, collateral, 0, 0, 0, due=due, rem=rem)
            legs = dict(debt=S(debt), locked_alpha=S(alpha), collateral=S(collateral))
        # The model accrues at the curve rate: check the curve and the accrual at it.
        curve = curve_bps(apr, k, cap, num, den)
        owed, rem_out = accrue(notional, curve, blocks, rem)
        side = 'S' if short else 'L'
        bad += (sn.short_u(pos) if short else sn.long_u(pos)) != (num, den)
        sn.block = blocks
        bad += sn._accrue_pos(pos, side) != curve
        if blocks == 0:
            # The model does not accrue a zero span; the formula leaves due and remainder unchanged.
            bad += (owed, rem_out) != (0, rem)
        bad += (pos.due, pos.rem) != (due + owed, rem_out)
        # The row is the accrual at the rate with the floor; where the floor does not bind it is the model's.
        floor = pick_floor(apr, cap)
        rate = rate_bps(apr, k, cap, floor, num, den)
        if rate != curve:
            ACCRUAL_FLOORED.append(i)
            owed, rem_out = accrue(notional, rate, blocks, rem)
        rows.append(dict(side='short' if short else 'long', apr=apr, k=k, cap=cap, floor=floor, twap=S(twap),
                         blocks=blocks, due_in=S(due), rem_in=S(rem), notional=S(notional), num=S(num), den=S(den),
                         curve=curve, rate=rate, due_out=S(due + owed), rem_out=S(rem_out), **legs))
    return rows, bad


if __name__ == '__main__':
    here = os.path.dirname(os.path.abspath(__file__))
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, '..', 'src', 'tests', 'golden', 'v21.json')
    sections = dict(rate=rate_rows(), coverage=coverage_rows(), accrual=accrual_rows())
    sp = SynthParams()
    out = dict(source='model/v21', seed=SEED, q64=S(Q64), blocks_per_year=BPY,
               stress_bps=sp.stress_bps, stress_slope_bps=sp.stress_slope_bps,
               **{name: rows for name, (rows, _) in sections.items()})
    with open(path, 'w') as f:
        json.dump(out, f, indent=0, sort_keys=True)
        f.write('\n')
    mismatches = {name: bad for name, (_, bad) in sections.items()}
    print('v21 golden rows', {name: len(rows) for name, (rows, _) in sections.items()})
    print('mismatches vs v21.core', mismatches)
    rate = sections['rate'][0]
    print('rows where the floor binds', dict(rate=sum(r['rate'] != r['curve'] for r in rate),
                                             accrual=len(ACCRUAL_FLOORED)))
    sys.exit(1 if any(mismatches.values()) else 0)
