#!/usr/bin/env python3
"""Evaluate μ's September F and Cycle-A attacker return on SN29-like pools.

Companion to pallets/lending/docs/no-arbitrage.tex (increment 1).
Convention: μ's F is pool/cost factor (profit when F<1).
Cycle-A F_cyc is attacker terminal/initial TAO (profit when F_cyc>1).
"""

from __future__ import annotations

import json
import math
from pathlib import Path

PHI_SWAP = 33 / 65535  # production default
W2_HALF = 0.5


def mu_F(f_h: float, phi: float = PHI_SWAP, w2: float = W2_HALF) -> float:
    """μ's September factor (eqs. (1)–(2) in no-arbitrage.tex)."""
    if f_h <= 0:
        return 1.0
    if not (0 < w2 < 1):
        raise ValueError("w2 must be in (0,1)")
    one_m = (1.0 - phi) ** (1.0 / w2)
    base = one_m / (one_m + f_h - phi)
    inner = 1.0 - (1.0 - one_m) / (f_h - phi + 1.0)
    if inner <= 0:
        return float("nan")
    exp = (f_h + 1.0) / ((f_h + 1.0 + phi) * (inner ** (w2 / (1.0 - w2))))
    return (f_h + 1.0) * (base**exp)


def cycle_A_lending_extra(
    y: float,
    x: float,
    P: float,
    *,
    fee: float = 0.005,
    dump_frac: float = 0.0,
    keep_weights: bool = False,
) -> dict:
    """Open-long → wallet repay → sell A. Returns lending-leg F_cyc ≈ (dy_sell)/P.

    Uses Balancer 50/50 closed forms. `fee` is a simple multiplicative tax on
    each swap leg (order-of-magnitude; production is 33/65535 per swap).
    """
    w1 = w2 = 0.5
    if dump_frac > 0:
        da = x * dump_frac
        dy = y * (1.0 - (x / (x + da)) ** (w1 / w2)) * (1.0 - fee)
        y -= dy
        x += da
    g = P / y
    aL = x * g
    y1, x1 = y - P, x - aL
    A = x1 * (1.0 - (y1 / (y1 + P)) ** (w2 / w1)) * (1.0 - fee)
    y2, x2 = y1 + P, x1 - A
    p_post = (w1 / w2) * (y2 / x2)
    y3, x3 = y2 + P, x2 + aL
    if keep_weights:
        w1p, w2p = w1, w2
    else:
        ratio = p_post * x3 / y3
        w1p = ratio / (1.0 + ratio)
        w2p = 1.0 - w1p
    dy_sell = y3 * (1.0 - (x3 / (x3 + A)) ** (w1p / w2p)) * (1.0 - fee)
    L0 = (x**w1) * (y**w2)
    L3 = (x3**w1p) * (y3**w2p)
    return {
        "F_cyc": dy_sell / P,
        "profit_tao": dy_sell - P,
        "w1_after": w1p,
        "L_ratio": L3 / L0 if L0 else float("nan"),
        "A": A,
        "keep_weights": keep_weights,
        "dump_frac": dump_frac,
    }


def main() -> None:
    rows_mu = []
    for f_h in (0.01, 0.05, 0.075, 0.10, 0.165, 0.25):
        F = mu_F(f_h)
        rows_mu.append({"f_h": f_h, "F": F, "F_lt_1": F < 1})

    y = 100_000.0
    x = y / 0.00279
    P = 0.165 * y
    rows_A = []
    for dump in (0.0, 0.20, 0.35, 0.50):
        for keep in (False, True):
            r = cycle_A_lending_extra(y, x, P, dump_frac=dump, keep_weights=keep)
            rows_A.append(r)

    out = {
        "phi_swap": PHI_SWAP,
        "mu_F": rows_mu,
        "cycle_A_sn29_like": {
            "y_tao": y,
            "price": 0.00279,
            "P_tao": P,
            "rows": rows_A,
        },
    }
    text = json.dumps(out, indent=2)
    print(text)
    dest = Path(__file__).resolve().parent / "no_arbitrage_F.json"
    dest.write_text(text + "\n")
    print(f"wrote {dest}", flush=True)

    print("\nμ F summary (profit in μ's convention when F<1):")
    for r in rows_mu:
        print(f"  f_h={r['f_h']:.3f}  F={r['F']:.6f}  F<1={r['F_lt_1']}")

    print("\nCycle A F_cyc (attacker profit when >1), P=0.165 y:")
    for r in rows_A:
        tag = "keep_w" if r["keep_weights"] else "update_w"
        print(
            f"  dump={r['dump_frac']:.0%} {tag}: "
            f"F_cyc={r['F_cyc']:.4f} profit={r['profit_tao']:+.1f}τ "
            f"w1={r['w1_after']:.4f} L×={r['L_ratio']:.4f}"
        )


if __name__ == "__main__":
    main()
