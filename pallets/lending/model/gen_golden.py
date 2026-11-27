"""Golden vectors for the parts of the Rust pure core (pallets/lending/src/math) that v2.1 keeps from
v2.0.1, generated from model/v201: the pool curve, the long coverage need and the accrual formula.
The v2.1 rate curve, short coverage reference and accrual at the curve rate are in gen_golden_v21.py.
Deterministic (seeded). Usage: python3 gen_golden.py OUT.json
Prices are U64F64 bit patterns (rao TAO per rao alpha << 64) as decimal strings; amounts are rao integers."""
import json, random, sys
from v201 import pool as pm
from v201.core import Subnet
from v201.params import RAO, Q64, BPS, BPY, SynthParams, LongParams

rng = random.Random(20261126)
S = str

def pools(n):
    out = [(1_000 * RAO, 480_000 * RAO), (5_000 * RAO, 2_400_000 * RAO), (25_000 * RAO, 9_000_000 * RAO),
           (1_000 * RAO + 1, 1_000 * RAO + 7), (123_456_789_012, 98_765_432_109_876)]
    while len(out) < n:
        T = rng.randrange(100 * RAO, 200_000 * RAO); X = T * rng.randrange(50, 5_000) // 10 + rng.randrange(1, 10**9)
        out.append((T, X))
    return out

def curve_cases():
    rows = []
    for T, X in pools(40):
        for frac in (0, 1, 37, 1_000, 10_000, 250_000, 600_000, 999_999):
            a = X * frac // 1_000_000
            tin = T * frac // 1_000_000
            rows.append(dict(tao=S(T), alpha=S(X), a=S(a), tin=S(tin),
                out_alpha_for_tao=S(pm.out_alpha_for_tao(T, X, tin)),
                out_tao_for_alpha=S(pm.out_tao_for_alpha(T, X, a)),
                tao_in_for_alpha=None if pm.tao_in_for_alpha(T, X, a) is None else S(pm.tao_in_for_alpha(T, X, a)),
                buy_cost_nofee=None if pm.buy_cost_nofee(T, X, a) is None else S(pm.buy_cost_nofee(T, X, a)),
                sell_proceeds_nofee=S(pm.sell_proceeds_nofee(T, X, a))))
        p = pm.price(T, X)
        for mul in (5_000, 10_000, 12_500, 30_000, 90_000):
            tgt = p * mul // BPS
            Tp, Xp = pm.pumped(T, X, tgt); Td, Xd = pm.dumped(T, X, tgt)
            rows.append(dict(tao=S(T), alpha=S(X), price=S(tgt), pumped=[S(Tp), S(Xp)], dumped=[S(Td), S(Xd)],
                             spot=S(p)))
    return rows

def long_coverage():
    """The long need (SPEC L2, unchanged in v2.1). The random draws keep the order of the v2.0.1 generator,
    so the rows are the long halves of its coverage rows."""
    rows = []
    for T, X in pools(30):
        sn = Subnet(T, X, X)
        spot = sn.spot()
        for qf, Qf in ((1, 1), (10, 100), (50, 180), (100, 300), (200, 600), (1, 900)):
            twap = spot * rng.randrange(7_000, 14_000) // BPS
            sma = spot * rng.randrange(7_000, 14_000) // BPS
            # long: D lifted against a book of A
            A = X * Qf // 10_000; a = A * qf // max(Qf, 1); D = pm.out_tao_for_alpha(T, X, a) + rng.randrange(0, RAO)
            lneed = sn.long_need(D, a, A, T, X, spot, twap)
            rows.append(dict(tao=S(T), alpha=S(X), spot=S(spot), twap=S(twap), sma=S(sma),
                             long=dict(a=S(a), book=S(A), debt=S(D), need=S(lneed))))
    return rows

def interest_cases():
    rows = []
    for _ in range(200):
        notional = rng.randrange(0, 10**15); apr = rng.choice((1, 2_000, 5_000, 10_000)); n = rng.randrange(0, 50_400)
        rem = rng.randrange(0, BPS * BPY)
        num = notional * apr * n + rem
        rows.append(dict(notional=S(notional), apr_bps=apr, blocks=n, rem_in=S(rem),
                         owed=S(num // (BPS * BPY)), rem_out=S(num % (BPS * BPY)),
                         day=S(notional * apr // (BPS * 365))))
    return rows

if __name__ == '__main__':
    out = dict(source='model/v201', seed=20261126, q64=S(Q64), fee=[pm.FEE_NUM, pm.FEE_DEN] if hasattr(pm, 'FEE_NUM') else [33, 65535],
               curve=curve_cases(), long_coverage=long_coverage(), interest=interest_cases())
    json.dump(out, open(sys.argv[1], 'w'), indent=0, sort_keys=True)
    print('golden', {k: len(v) for k, v in out.items() if isinstance(v, list)})
