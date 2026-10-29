#!/usr/bin/env python3
"""Bounded cycle search: design (a) vs (b) vs buggy weight-update readd.

Companion to no-arbitrage.tex §balancer-coherence / §designs.

Stake/unstake are always Balancer swaps at fixed (w1, w2). Only lending
readd differs across designs:

  buggy — credit (dy,dx) then rewrite w via update_weights_for_added_liquidity
  (a)    — credit exact lift deltas, weights untouched
  (b)    — restore invariant units ΔL with a proportional join at the
           *current* ratio (price-preserving), mark-to-market the custody
           residual against the pool (one Balancer swap)

Search: open short | open long → optional pump/dump → close
(buyback | wallet repay) → unwind residual alpha. Report max F_cyc.
"""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, replace
from pathlib import Path

PHI = 33 / 65535


@dataclass
class Pool:
    y: float  # TAO
    x: float  # alpha
    w1: float  # base (alpha) weight
    w2: float  # quote (TAO) weight

    def L(self) -> float:
        return (self.x**self.w1) * (self.y**self.w2)

    def price(self) -> float:
        return (self.w1 / self.w2) * (self.y / self.x)


def buy_alpha(pool: Pool, tao_in: float, fee: float = PHI) -> tuple[Pool, float]:
    """Stake: TAO → alpha. Fee on input."""
    if tao_in <= 0:
        return pool, 0.0
    dy = tao_in * (1.0 - fee)
    # (x - dx)^w1 * (y + dy)^w2 = x^w1 y^w2
    # dx = x * (1 - (y/(y+dy))^(w2/w1))
    ratio = pool.y / (pool.y + dy)
    dx = pool.x * (1.0 - ratio ** (pool.w2 / pool.w1))
    return replace(pool, y=pool.y + dy, x=pool.x - dx), dx


def sell_alpha(pool: Pool, alpha_in: float, fee: float = PHI) -> tuple[Pool, float]:
    """Unstake: alpha → TAO. Fee on input."""
    if alpha_in <= 0:
        return pool, 0.0
    dx = alpha_in * (1.0 - fee)
    ratio = pool.x / (pool.x + dx)
    dy = pool.y * (1.0 - ratio ** (pool.w1 / pool.w2))
    return replace(pool, y=pool.y - dy, x=pool.x + dx), dy


def update_weights_for_add(pool: Pool, dy: float, dx: float) -> Pool:
    """Production Balancer::update_weights_for_added_liquidity (float)."""
    y1, x1 = pool.y + dy, pool.x + dx
    q1 = pool.w1 * pool.y * x1
    q2 = pool.w2 * pool.x * y1
    s = q1 + q2
    if s <= 0:
        return replace(pool, y=y1, x=x1)
    w2 = q2 / s
    w1 = 1.0 - w2
    # clamp away from 0/1 like MIN_WEIGHT
    w2 = min(max(w2, 1e-6), 1.0 - 1e-6)
    w1 = 1.0 - w2
    return Pool(y=y1, x=x1, w1=w1, w2=w2)


def credit_keep_w(pool: Pool, dy: float, dx: float) -> Pool:
    """Design (a): exact deltas, weights untouched."""
    return replace(pool, y=pool.y + dy, x=pool.x + dx)


def credit_invariant_units(
    pool: Pool, dy_cust: float, dx_cust: float, dL: float, fee: float = PHI
) -> tuple[Pool, float]:
    """Design (b): proportional join restoring dL, then MTM residual.

    Returns (pool_after, tao_mtm) where tao_mtm is the mark-to-market of the
    custody residual (positive = protocol/attacker receives TAO).
    """
    L = pool.L()
    if L <= 0 or dL <= 0:
        return credit_keep_w(pool, dy_cust, dx_cust), 0.0
    s = dL / L
    dy_prop = pool.y * s
    dx_prop = pool.x * s
    # Proportional join: price unchanged, L *= (1+s)
    pool2 = replace(pool, y=pool.y + dy_prop, x=pool.x + dx_prop)
    # Custody residual vs what the join consumed.
    ry = dy_cust - dy_prop
    rx = dx_cust - dx_prop
    tao_mtm = 0.0
    # Excess TAO in custody → leave in pool already counted? We compare to prop.
    # If custody has more TAO than prop needs, surplus TAO is free (to attacker
    # via escrow refund path) — count as +tao_mtm.
    # If short TAO, attacker must supply (negative mtm).
    tao_mtm += ry
    if rx > 0:
        # Excess alpha: sell into pool for TAO
        pool2, dy = sell_alpha(pool2, rx, fee)
        tao_mtm += dy
    elif rx < 0:
        # Need more alpha: buy with TAO
        need = -rx
        # Approximate TAO cost via local price / CP quote
        # Solve buy_alpha for dx=need by binary search on tao_in
        lo, hi = 0.0, pool2.y * 10 + 1.0
        for _ in range(60):
            mid = 0.5 * (lo + hi)
            p_try, got = buy_alpha(pool2, mid, fee)
            if got < need:
                lo = mid
            else:
                hi = mid
                pool2 = p_try
        tao_mtm -= hi
    return pool2, tao_mtm


@dataclass
class Result:
    design: str
    side: str
    close: str
    h: float
    pump_h: float
    F_cyc: float
    profit: float
    w1_after: float
    L_ratio: float


def run_short_cycle(
    pool0: Pool,
    *,
    principal_frac: float,
    pump_frac: float,
    close: str,
    design: str,
    fee: float = PHI,
) -> Result | None:
    """Short: lift g=P/x, force-sell P, optional pump, close, unwind."""
    y0, x0 = pool0.y, pool0.x
    P = principal_frac * x0  # alpha principal
    if P <= 0 or P >= x0 * 0.5:
        return None
    g = P / x0
    lift_tao = g * y0
    lift_alpha = P
    L0 = pool0.L()
    dL = g * L0

    # Lift
    pool = replace(pool0, y=y0 - lift_tao, x=x0 - lift_alpha)
    # Force-sell principal into remaining pool → escrow proceeds
    pool, proceeds = sell_alpha(pool, P, fee)
    # Collateral sized at 25% LTV of principal value at open spot (approx)
    coll = 0.25 * (pool0.price() * P)
    # Attacker free starts as coll + pump budget; we track delta only on cycle TAO
    free = -coll  # posted collateral
    alpha_inv = 0.0

    # Pump (foreign or self): buy alpha with pump_frac * y0
    pump_tao = pump_frac * y0
    if pump_tao > 0:
        free -= pump_tao
        pool, got = buy_alpha(pool, pump_tao, fee)
        alpha_inv += got

    if close == "buyback":
        # Escrow buyback: spend proceeds to buy P alpha (may fail if underwater)
        # Sim: try to buy P with proceeds; if short, top up from free
        need = P
        # binary search cost
        lo, hi = 0.0, max(proceeds, pool.y) * 5 + 1.0
        pool_bb = pool
        for _ in range(60):
            mid = 0.5 * (lo + hi)
            p_try, got = buy_alpha(pool, mid, fee)
            if got < need:
                lo = mid
            else:
                hi = mid
                pool_bb = p_try
        cost = hi
        if cost > proceeds:
            free -= cost - proceeds
        # else leftover proceeds refunded
        else:
            free += proceeds - cost
        pool = pool_bb
        # collateral returned
        free += coll
        dy_c, dx_c = lift_tao, lift_alpha
    elif close == "wallet":
        # Buy P alpha from free, repay, get proceeds+coll
        lo, hi = 0.0, pool.y * 5 + 1.0
        pool_w = pool
        for _ in range(60):
            mid = 0.5 * (lo + hi)
            p_try, got = buy_alpha(pool, mid, fee)
            if got < P:
                lo = mid
            else:
                hi = mid
                pool_w = p_try
        free -= hi
        pool = pool_w
        free += proceeds + coll
        dy_c, dx_c = lift_tao, lift_alpha
        # principal alpha burned from wallet (already bought)
    else:
        return None

    # Readd
    if design == "buggy":
        pool = update_weights_for_add(pool, dy_c, dx_c)
        mtm = 0.0
    elif design == "a":
        pool = credit_keep_w(pool, dy_c, dx_c)
        mtm = 0.0
    elif design == "b":
        pool, mtm = credit_invariant_units(pool, dy_c, dx_c, dL, fee)
    else:
        return None
    free += mtm

    # Unwind leftover alpha
    if alpha_inv > 0:
        pool, dy = sell_alpha(pool, alpha_inv, fee)
        free += dy

    # Budget at risk ≈ coll + pump (and buy costs already in free)
    # F_cyc = 1 + free / V0 with V0 = coll + max(pump, eps)
    V0 = coll + max(pump_tao, 1e-9)
    F = 1.0 + free / V0
    return Result(
        design=design,
        side="short",
        close=close,
        h=principal_frac,
        pump_h=pump_frac,
        F_cyc=F,
        profit=free,
        w1_after=pool.w1,
        L_ratio=pool.L() / L0 if L0 else float("nan"),
    )


def run_long_cycle(
    pool0: Pool,
    *,
    principal_frac: float,
    dump_frac: float,
    close: str,
    design: str,
    fee: float = PHI,
) -> Result | None:
    """Long / Cycle A: optional dump, lift g=P/y, force-buy, wallet/buyback close, sell A."""
    y0, x0 = pool0.y, pool0.x
    pool = pool0
    # Optional pre-dump of alpha inventory (attacker holds alpha from elsewhere)
    dump_alpha = dump_frac * x0
    free = 0.0
    if dump_alpha > 0:
        # Assume attacker bought dump_alpha earlier at start price (cost)
        cost0 = pool0.price() * dump_alpha  # rough
        free -= cost0
        pool, dy = sell_alpha(pool, dump_alpha, fee)
        free += dy

    P = principal_frac * pool.y  # TAO principal
    if P <= 0 or P >= pool.y * 0.5:
        return None
    g = P / pool.y
    lift_tao = P
    lift_alpha = g * pool.x
    L0 = pool0.L()
    dL = g * pool.L()

    pool = replace(pool, y=pool.y - lift_tao, x=pool.x - lift_alpha)
    # Force-buy escrow A with P
    pool, A = buy_alpha(pool, P, fee)
    # Collateral is alpha; skip explicit C tracking — Cycle A wallet path
    # pays P TAO, receives A back, then sells A.
    if close == "wallet":
        free -= P  # repay principal TAO
        free_alpha = A  # receive escrow
        dy_c, dx_c = lift_tao, lift_alpha
    elif close == "buyback":
        # Sell-path: sell A back into pool to recover TAO toward P
        pool, recovered = sell_alpha(pool, A, fee)
        free += recovered - P  # net vs principal
        free_alpha = 0.0
        dy_c, dx_c = lift_tao, lift_alpha
    else:
        return None

    if design == "buggy":
        pool = update_weights_for_add(pool, dy_c, dx_c)
        mtm = 0.0
    elif design == "a":
        pool = credit_keep_w(pool, dy_c, dx_c)
        mtm = 0.0
    elif design == "b":
        pool, mtm = credit_invariant_units(pool, dy_c, dx_c, dL, fee)
    else:
        return None
    free += mtm

    if free_alpha > 0:
        pool, dy = sell_alpha(pool, free_alpha, fee)
        free += dy

    V0 = max(P, 1e-9)
    F = 1.0 + free / V0
    return Result(
        design=design,
        side="long",
        close=close,
        h=principal_frac,
        pump_h=dump_frac,
        F_cyc=F,
        profit=free,
        w1_after=pool.w1,
        L_ratio=pool.L() / L0 if L0 else float("nan"),
    )


def search() -> dict:
    # SN29-like
    y = 8_000.0
    p = 0.00279
    w1, w2 = 0.47, 0.53
    x = (w1 / w2) * (y / p)
    pool0 = Pool(y=y, x=x, w1=w1, w2=w2)

    hs = [0.01, 0.03, 0.05, 0.075, 0.10, 0.125, 0.165]
    pumps = [0.0, 0.25, 0.5, 0.75, 1.0, 1.25]
    dumps = [0.0, 0.1, 0.2, 0.35, 0.5]
    designs = ["buggy", "a", "b"]
    closes = ["buyback", "wallet"]

    rows: list[Result] = []
    for design in designs:
        for h in hs:
            for pump in pumps:
                for close in closes:
                    r = run_short_cycle(
                        pool0,
                        principal_frac=h,
                        pump_frac=pump,
                        close=close,
                        design=design,
                    )
                    if r is not None and math.isfinite(r.F_cyc):
                        rows.append(r)
            for dump in dumps:
                for close in closes:
                    r = run_long_cycle(
                        pool0,
                        principal_frac=h,
                        dump_frac=dump,
                        close=close,
                        design=design,
                    )
                    if r is not None and math.isfinite(r.F_cyc):
                        rows.append(r)

    def best(design: str) -> dict:
        subset = [r for r in rows if r.design == design]
        if not subset:
            return {}
        top = max(subset, key=lambda r: r.F_cyc)
        profitable = [r for r in subset if r.F_cyc > 1.0 + 1e-9]
        return {
            "n": len(subset),
            "max_F_cyc": top.F_cyc,
            "max_profit": top.profit,
            "at": {
                "side": top.side,
                "close": top.close,
                "h": top.h,
                "pump_or_dump": top.pump_h,
                "w1_after": top.w1_after,
                "L_ratio": top.L_ratio,
            },
            "n_profitable": len(profitable),
            "profitable_examples": [
                {
                    "side": r.side,
                    "close": r.close,
                    "h": r.h,
                    "pump_or_dump": r.pump_h,
                    "F_cyc": r.F_cyc,
                    "profit": r.profit,
                    "w1_after": r.w1_after,
                    "L_ratio": r.L_ratio,
                }
                for r in sorted(profitable, key=lambda r: -r.F_cyc)[:8]
            ],
        }

    # Sanity: a13-like point
    a13 = {
        d: run_short_cycle(
            pool0, principal_frac=0.09, pump_frac=1.125, close="wallet", design=d
        )
        for d in designs
    }
    a15 = {
        d: run_short_cycle(
            pool0, principal_frac=0.075, pump_frac=0.875, close="buyback", design=d
        )
        for d in designs
    }

    def ser(r: Result | None) -> dict | None:
        if r is None:
            return None
        return {
            "F_cyc": r.F_cyc,
            "profit": r.profit,
            "w1_after": r.w1_after,
            "L_ratio": r.L_ratio,
        }

    out = {
        "pool": {"y": y, "price": p, "w1": w1, "w2": w2, "fee": PHI},
        "note": (
            "Stake/unstake always Balancer at fixed w. Designs differ only on "
            "lending readd. F_cyc>1 = attacker profit."
        ),
        "by_design": {d: best(d) for d in designs},
        "a13_like": {d: ser(a13[d]) for d in designs},
        "a15_like": {d: ser(a15[d]) for d in designs},
    }
    return out


def main() -> None:
    out = search()
    text = json.dumps(out, indent=2)
    dest = Path(__file__).resolve().parent / "no_arbitrage_cycle_search.json"
    dest.write_text(text + "\n")
    print(text)
    print(f"wrote {dest}", flush=True)
    print("\n=== verdict ===")
    for d, s in out["by_design"].items():
        print(
            f"  {d}: max F_cyc={s.get('max_F_cyc', float('nan')):.6f} "
            f"profitable={s.get('n_profitable', 0)}/{s.get('n', 0)} "
            f"at {s.get('at')}"
        )


if __name__ == "__main__":
    main()
