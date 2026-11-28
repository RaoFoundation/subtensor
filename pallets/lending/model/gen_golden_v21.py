"""Golden vectors for the v2.1 final pure core (pallets/lending/src/math), generated from model/v21.

Writes ../src/tests/golden/v21.json (or the path given as the first argument) with five sections:

  rate       rate(apr, k, cap, floor, num, den) (SPEC 1 item 2, P5): the 3,264 vectors of the upstream run,
             apr 1 / 2,000 / 10,000 x k 1..8 x cap 2,000 .. 1,000,000 (cap >= apr) x 12 operand pairs x the
             floor off and on. floor 1 is the final rate, exactly as SPEC 1 item 2 writes it with the floor
             at apr: `r = apr; if num <= den {} else repeat k { r = floor(r * num / den); if r >= cap { r = cap;
             stop } }; r = min(max(r, apr), cap)`. floor 0 is the pure curve `min(cap, apr * u^k)` with no
             floor (no debt pays 0), kept to pin the curve itself.
  rate_edge  rate with floor 1 on edge operands: zero operands for every exponent, the cap reached at each
             step (and missed by one), u up to 100 on the launch curve, small and wide random operands, and
             operands at the top of u128.
  coverage   short coverage need at the flat stress of v2.1 (SPEC P2, `stress_slope_bps` removed) on
             ref = max(spot, SMA30_rank). The TWAP median is in each row only to show that it does not move the
             need.
  accrual    due and remainder after n blocks at the final rate. A short has num = floor(q x min(TWAP, SMA30)),
             den = C + P and is charged on notional = floor(q x TWAP); a long has num = D, den =
             floor(A x max(TWAP, SMA30)) + C and is charged on notional = D (SPEC 1 item 2, the slow price).
  default    whole-model scenarios of a short default (SPEC 1 item 4, I2, I5): a short opens at block 1 on a
             pool of `tao` / `alpha` with `collateral_open`, the slow prices are then set to `twap` / `sma` and
             its collateral to `collateral_left` (as after a long bleed; written directly, the model has no
             faster way to get there), and the blocks run with no other trade. Each row lists every scheduled
             charge `[blocks after the open, amount, rate]` up to the default, and the default itself: its
             block, `residual` (q moved to ResidualAlpha), `proceeds_to_pool` (P credited to SubnetTAO), the
             deposit burned and SubnetTAO after it. Rows cover collateral that pays the charge exactly (no
             default at that charge), one rao less and one rao more, on both sides of u = 1.

Every row is computed twice: by the formulas written out in this file and by v21.core with the final switches
(`rate_floor` as the row says, `u_price_slow` on, the stress slope at 0). The script fails on any mismatch and
prints the row counts. Deterministic (seeded).

Usage: cd pallets/lending/model && python3 gen_golden_v21.py [OUT.json]
Prices are U64F64 bit patterns (rao TAO per rao alpha << 64) and amounts are rao integers, all as decimal
strings; rates, exponents, floors and block counts are JSON numbers."""
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
S = str
U64 = 2**64 - 1
U128 = 2**128 - 1
ACCRUAL_DEN = BPS * BPY
# The final launch curve (SPEC 2.1, Jacob 27 Nov): k = 1, cap 200%/yr, the floor at apr.
LAUNCH = (2_000, 1, 20_000)
# The operand pairs of the upstream run (rate_golden.json), u from 0 to 10^11 and the u64 and u128 extremes.
BASE_PAIRS = ((0, 10**9), (10**9, 0), (1, 1), (3 * 10**8, 10**9), (10**9, 10**9), (10**9 + 1, 10**9),
              (19 * 10**8, 10**9), (5 * 10**9, 10**9), (10**11, 10**9), (U64, 1), (U64, U64),
              (12345678901, 98765432109))


# ---------------------------------------------------------------------------- formulas (SPEC 1 item 2)
def curve(apr, k, cap, num, den):
    """The final rate, floor at apr, as SPEC 1 item 2 writes it. Debt against no holdings (den = 0 < num)
    pays the cap; no debt (num = 0, also with den = 0) pays the base APR."""
    if num <= den:
        r = apr
    else:
        if den <= 0:
            return cap
        r = apr
        for _ in range(k):
            r = r * num // den
            if r >= cap:
                r = cap
                break
    return min(max(r, apr), cap)


def curve_off(apr, k, cap, num, den):
    """The pure curve with no floor: the model with `rate_floor` off. No debt pays 0."""
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


def short_need(sp, q, Q, T, X, spot, sma):
    """SPEC P2 at the flat stress: ref = max(spot, SMA30_rank), the whole book bought back from the pool
    pumped to stress x ref, pro rata."""
    if Q <= 0:
        return 0
    tgt = max(spot, sma) * sp.stress_bps // BPS
    Tp, Xp = pm.pumped(T, X, tgt)
    cost = pm.tao_in_for_alpha(Tp, Xp, Q)
    return None if cost is None else pm.cdiv(cost * q, Q)


def accrue(notional, rate, blocks, rem):
    num = notional * rate * blocks + rem
    return num // ACCRUAL_DEN, num % ACCRUAL_DEN


# ---------------------------------------------------------------------------- the model
def model(apr, k, cap, T=1_000 * RAO, X=480_000 * RAO, twap=None, sma=None, floor=True):
    """One subnet with the final switches: floor at apr per `floor`, slow price on, flat stress."""
    sp = dataclasses.replace(SynthParams(), apr_bps=apr, rate_k=k, rate_cap_bps=cap, stress_slope_bps=0)
    sw = dataclasses.replace(ModelSwitches(), rate_floor=bool(floor), u_price_slow=True)
    sn = core.Subnet(T, X, X, sp=sp, lp=LongParams(), sw=sw)
    if twap is not None:
        sn.twap = [twap] * sp.twap_samples
    if sma is not None:
        sn.sma_days = [sma] * sp.rank_days
    return sn


# ---------------------------------------------------------------------------- rate rows
def base_inputs():
    for apr, k, cap in itertools.product((1, 2_000, 10_000), range(1, 9),
                                         (2_000, 10_000, 50_000, 73_000, 200_000, 1_000_000)):
        if cap < apr:
            continue
        for num, den in BASE_PAIRS:
            for floor in (0, 1):
                yield apr, k, cap, floor, num, den


def rate_rows():
    rows, bad = [], 0
    for apr, k, cap, floor, num, den in base_inputs():
        rate = curve(apr, k, cap, num, den) if floor else curve_off(apr, k, cap, num, den)
        bad += model(apr, k, cap, floor=floor).rate_from(num, den) != rate
        bad += curve(apr, k, cap, num, den) != max(curve_off(apr, k, cap, num, den), apr)
        bad += not (apr <= curve(apr, k, cap, num, den) <= cap)
        rows.append(dict(apr=apr, k=k, cap=cap, floor=floor, num=S(num), den=S(den), rate=rate))
    return rows, bad


def edge_inputs():
    rows = []
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
        rows.append((apr, rng.randrange(1, 9), rng.randrange(apr, 1_000_001), rng.randrange(0, 25),
                     rng.randrange(0, 25)))
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


def edge_rate_rows():
    rows, bad = [], 0
    for apr, k, cap, num, den in edge_inputs():
        rate = curve(apr, k, cap, num, den)
        bad += model(apr, k, cap, floor=True).rate_from(num, den) != rate
        bad += not (apr <= rate <= cap)
        rows.append(dict(apr=apr, k=k, cap=cap, floor=1, num=S(num), den=S(den), rate=rate))
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


def accrual_rows():
    rows, bad = [], 0
    for i in range(400):
        short = i % 2 == 0
        apr, k, cap = curve_params()
        twap = Q64 * rng.randrange(1, 5_000_000) // 1_000_000_000          # 1e-9 .. 5e-3 TAO per alpha
        # The SMA30 both above and below the TWAP, so the slow price takes either side.
        sma = twap * rng.randrange(5_000, 15_000) // 10_000
        blocks = rng.choice((0, 1, 360, 7_200, rng.randrange(0, 50_400)))
        rem = rng.randrange(0, ACCRUAL_DEN)
        due = rng.choice((0, 0, rng.randrange(0, 10**9)))
        sn = model(apr, k, cap, twap=twap, sma=sma)
        edge = i % 20                                                      # 0,1: num = 0; 2,3: den = 0
        if short:
            slow = min(twap, sma)
            q = 0 if edge == 0 else rng.randrange(1, 10**15)
            notional = q * twap >> 64
            held = 0 if edge == 2 else max(1, notional * rng.randrange(200, 30_000) // 10_000)
            proceeds = rng.randrange(0, held + 1)
            collateral = held - proceeds
            num, den = q * slow >> 64, collateral + proceeds
            pos = core.Short('a', q, collateral, proceeds, 0, 0, 0, 0, 0, due=due, rem=rem)
            legs = dict(q=S(q), collateral=S(collateral), proceeds=S(proceeds))
        else:
            slow = max(twap, sma)
            debt = 0 if edge == 1 else rng.randrange(1, 10**13)
            held = 0 if edge == 3 else max(1, debt * rng.randrange(200, 30_000) // 10_000)
            alpha_value = rng.randrange(0, held + 1)
            alpha = alpha_value * Q64 // twap
            collateral = held - (alpha * twap >> 64)
            notional = debt
            num, den = debt, (alpha * slow >> 64) + collateral
            pos = core.Long('a', debt, alpha, collateral, 0, 0, 0, due=due, rem=rem)
            legs = dict(debt=S(debt), locked_alpha=S(alpha), collateral=S(collateral))
        rate = curve(apr, k, cap, num, den)
        bad += (sn.short_u(pos) if short else sn.long_u(pos)) != (num, den)
        # The model accrues at its own rate (the final switches): it must be the row's rate.
        sn.block = blocks
        bad += sn._accrue_pos(pos, 'S' if short else 'L') != rate
        owed, rem_out = accrue(notional, rate, blocks, rem)
        if blocks == 0:
            # The model does not accrue a zero span; the formula leaves due and remainder unchanged.
            bad += (owed, rem_out) != (0, rem)
        bad += (pos.due, pos.rem) != (due + owed, rem_out)
        rows.append(dict(side='short' if short else 'long', apr=apr, k=k, cap=cap, twap=S(twap), sma=S(sma),
                         blocks=blocks, due_in=S(due), rem_in=S(rem), notional=S(notional), num=S(num),
                         den=S(den), rate=rate, due_out=S(due + owed), rem_out=S(rem_out), **legs))
    return rows, bad


# ---------------------------------------------------------------------------- default rows
DEFAULT_POOLS = ((5_000 * RAO, 50_000 * RAO), (25_000 * RAO, 9_000_000 * RAO), (2_000 * RAO, 480_000 * RAO),
                 (123_456_789_012_345, 9_876_543_210_987_654))
DEFAULT_HORIZON = 6                                                        # charges; the TWAP median holds


def default_formula(apr, k, cap, q, twap, sma, C, P, interval):
    """SPEC 1 items 2 and 4 written out: each scheduled charge reads the rate, accrues, takes min(due, C);
    the short defaults at the first charge that leaves C == 0 with interest due."""
    due = rem = 0
    charges = []
    for n in range(1, DEFAULT_HORIZON + 1):
        rate = curve(apr, k, cap, q * min(twap, sma) >> 64, C + P)
        owed, rem = accrue(q * twap >> 64, rate, interval, rem)
        due += owed
        c = min(due, C)
        C, due = C - c, due - c
        if c:
            charges.append([n * interval, S(c), rate])
        if C == 0 and due > 0:
            return charges, n * interval
    return charges, None


def default_run(apr, k, cap, T, X, q, c_open, twap, sma, c_left):
    """The same scenario through v21.core, block by block. Returns the opened position's proceeds and
    deposit, the charges, and the default: (blocks after the open, residual, proceeds_to_pool, burned,
    SubnetTAO after), or None if it did not default within the horizon."""
    sp = dataclasses.replace(SynthParams(), apr_bps=apr, rate_k=k, rate_cap_bps=cap, stress_slope_bps=0)
    sw = dataclasses.replace(ModelSwitches(), rate_floor=True, u_price_slow=True)
    sn = core.Subnet(T, X, X, sp=sp, lp=LongParams(), sw=sw, trace=True)
    sn.credit('a', c_open + sp.deposit)
    sn.begin_block()
    sn.open_short('a', q, c_open)
    sn.end_block()
    pos = sn.shorts['a']
    opened, proceeds, deposit, issuance = sn.block, pos.P, pos.deposit, sn.issuance
    sn.twap = [twap] * sp.twap_samples
    sn.sma_days = [sma] * sp.rank_days
    sn.custody -= pos.C - c_left
    pos.C = c_left
    charges, default = [], None
    for _ in range(DEFAULT_HORIZON * sp.charge_interval):
        sn.begin_block()
        seen, t_before, burned = len(sn.events), sn.T, sn.burned
        sn.end_block()
        paid = 0
        for ev in sn.events[seen:]:
            if ev[1] == 'interest':
                charges.append([sn.block - opened, S(ev[4]), ev[5]])
                paid += ev[4]
            elif ev[1] == 'short_default':
                assert ev[3] == q and sn.residual == q and sn.synth == 0 and sn.issuance == issuance
                assert 'a' not in sn.shorts and sn.custody == 0
                default = (sn.block - opened, sn.residual, sn.T - t_before - paid, sn.burned - burned, sn.T)
        if default:
            break
    return proceeds, deposit, charges, default


def default_rows():
    rows, bad = [], 0
    for i in range(48):
        T, X = DEFAULT_POOLS[i % len(DEFAULT_POOLS)]
        apr, k, cap = LAUNCH if i % 3 == 0 else curve_params()
        q = X * rng.randrange(60, 99) // 10_000                            # over min_notional, under the open impact cap
        spot = pm.price(T, X)
        c_open = 5 * (q * spot >> 64)
        # Slow prices after the open: below the open price (u < 1, the base APR whatever C is) for two rows
        # in three, above it (u > 1, the convex part) for the third.
        scale = rng.randrange(10_500, 40_000) if i % 3 == 2 else rng.randrange(3_000, 9_000)
        twap = spot * scale // BPS
        sma = twap * rng.randrange(8_000, 12_500) // BPS
        interval = SynthParams().charge_interval
        first = accrue(q * twap >> 64, apr, interval, 0)[0]
        # The collateral left: exactly the first charge at the base APR, one rao around it, nothing, or a
        # random amount worth up to a few charges.
        c_left = (first, first - 1, first + 1, 0, rng.randrange(0, 4 * first), rng.randrange(0, 3 * first))[i % 6]
        proceeds, deposit, charges, default = default_run(apr, k, cap, T, X, q, c_open, twap, sma, c_left)
        f_charges, f_default = default_formula(apr, k, cap, q, twap, sma, c_left, proceeds, interval)
        bad += default is None
        if default is None:
            continue
        at, residual, to_pool, burned, tao_after = default
        bad += (charges, at) != (f_charges, f_default)
        bad += (residual, to_pool, burned) != (q, proceeds, deposit)
        rows.append(dict(tao=S(T), alpha=S(X), apr=apr, k=k, cap=cap, q=S(q), collateral_open=S(c_open),
                         proceeds=S(proceeds), deposit=S(deposit), twap=S(twap), sma=S(sma),
                         collateral_left=S(c_left), charges=charges, default_at=at, residual=S(residual),
                         proceeds_to_pool=S(to_pool), burned=S(burned), tao_after=S(tao_after)))
    # The exact-cover rows must show both outcomes of the first charge.
    firsts = [r['default_at'] for r in rows]
    bad += not (360 in firsts and 720 in firsts)
    return rows, bad


if __name__ == '__main__':
    here = os.path.dirname(os.path.abspath(__file__))
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, '..', 'src', 'tests', 'golden', 'v21.json')
    sections = dict(rate=rate_rows(), rate_edge=edge_rate_rows(), coverage=coverage_rows(),
                    accrual=accrual_rows(), default=default_rows())
    sp = SynthParams()
    out = dict(source='model/v21', seed=SEED, q64=S(Q64), blocks_per_year=BPY, stress_bps=sp.stress_bps,
               **{name: rows for name, (rows, _) in sections.items()})
    with open(path, 'w') as f:
        json.dump(out, f, indent=0, sort_keys=True)
        f.write('\n')
    mismatches = {name: bad for name, (_, bad) in sections.items()}
    print('v21 golden rows', {name: len(rows) for name, (rows, _) in sections.items()})
    print('mismatches vs v21.core', mismatches)
    print('rate rows with the floor binding', sum(r['rate'] != curve_off(r['apr'], r['k'], r['cap'],
                                                                          int(r['num']), int(r['den']))
                                                  for r in sections['rate'][0] if r['floor']))
    sys.exit(1 if any(mismatches.values()) else 0)
