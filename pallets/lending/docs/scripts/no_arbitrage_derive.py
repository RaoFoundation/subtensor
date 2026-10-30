#!/usr/bin/env python3
"""First-principles drain derivation from pallet math (companion to no-arbitrage.tex).

Notation (matches the tex notation table):
  x, y     alpha / TAO reserves (SubnetAlphaIn / SubnetTAO)
  w_b, w_q base (alpha) / quote (TAO) Balancer weights; w_b + w_q = 1
  L        x^{w_b} y^{w_q}
  p        (w_b/w_q)·(y/x)
  φ        lift fraction = P/x for a short (pallet lift_fraction)
  ε        swap fee on input (base case ε=0; production 33/65535)

REPORT's printed ``w1'' is w_q (quote). Their L ≈ x^{1-w1} y^{w1} = our L.
"""

from __future__ import annotations

import json
import math
from pathlib import Path


def invariant(x: float, y: float, w_b: float, w_q: float) -> float:
    return (x**w_b) * (y**w_q)


def spot(x: float, y: float, w_b: float, w_q: float) -> float:
    return (w_b / w_q) * (y / x)


def sell_alpha(
    x: float, y: float, w_b: float, w_q: float, d_alpha: float, fee: float
) -> tuple[float, float, float]:
    d_in = d_alpha * (1.0 - fee)
    d_tao = y * (1.0 - (x / (x + d_in)) ** (w_b / w_q))
    return x + d_in, y - d_tao, d_tao


def buy_alpha(
    x: float, y: float, w_b: float, w_q: float, d_tao: float, fee: float
) -> tuple[float, float, float]:
    d_in = d_tao * (1.0 - fee)
    d_alpha = x * (1.0 - (y / (y + d_in)) ** (w_q / w_b))
    return x - d_alpha, y + d_in, d_alpha


def update_weights_for_added_liquidity(
    x: float, y: float, w_b: float, w_q: float, d_alpha: float, d_tao: float
) -> tuple[float, float, float, float]:
    """Pallet Balancer::update_weights_for_added_liquidity (float form)."""
    x1, y1 = x + d_alpha, y + d_tao
    q_b = w_b * y * x1
    q_q = w_q * x * y1
    w_q1 = q_q / (q_b + q_q)
    w_b1 = 1.0 - w_q1
    return w_b1, w_q1, x1, y1


def short_open(
    x: float, y: float, w_b: float, w_q: float, phi: float, fee: float
) -> dict:
    """Lift φ then force-sell principal P=φx. Fee on the sell only."""
    P = phi * x
    lift_tao = phi * y
    x_l, y_l = x - P, y - lift_tao
    x_s, y_s, proceeds = sell_alpha(x_l, y_l, w_b, w_q, P, fee)
    # Fee-free identity: y_s = y · (1-φ)^{1/w_q}
    y_s_cf = y * ((1.0 - phi) ** (1.0 / w_q)) if fee == 0.0 else None
    return {
        "phi": phi,
        "P": P,
        "lift_tao": lift_tao,
        "proceeds": proceeds,
        "x": x_s,
        "y": y_s,
        "L": invariant(x_s, y_s, w_b, w_q),
        "p": spot(x_s, y_s, w_b, w_q),
        "y_s_closed_form": y_s_cf,
    }


def cycle_short_pump_wallet(
    *,
    x0: float,
    y0: float,
    w_q: float,
    phi: float,
    pump: float,
    buy_tao: float,
    coll: float,
    fee: float,
    readd: str,
) -> dict:
    """a13-shaped cycle. readd ∈ {buggy, keep}."""
    w_b = 1.0 - w_q
    L0 = invariant(x0, y0, w_b, w_q)
    op = short_open(x0, y0, w_b, w_q, phi, fee)
    x, y = op["x"], op["y"]
    x, y, a_pump = buy_alpha(x, y, w_b, w_q, pump, fee)
    x, y, a_buy = buy_alpha(x, y, w_b, w_q, buy_tao, fee)
    P, lift_tao, proceeds = op["P"], op["lift_tao"], op["proceeds"]
    if readd == "buggy":
        w_b, w_q, x, y = update_weights_for_added_liquidity(
            x, y, w_b, w_q, P, lift_tao
        )
    elif readd == "keep":
        x, y = x + P, y + lift_tao
    else:
        raise ValueError(readd)
    leftover = a_buy - P
    total_alpha = a_pump + leftover
    x, y, tao_out = sell_alpha(x, y, w_b, w_q, total_alpha, fee)
    # Combined free of attacker+pumper:
    # -coll -pump -buy + (proceeds+coll) + unstake = -pump -buy + proceeds + unstake
    free_delta = -pump - buy_tao + proceeds + tao_out
    return {
        "phi": phi,
        "P": P,
        "lift_tao": lift_tao,
        "proceeds": proceeds,
        "w_q_after": w_q,
        "L_after_readd_proxy": None,
        "y_end": y,
        "x_end": x,
        "free_delta": free_delta,
        "subnet_tao_delta": y - y0,
        "L0": L0,
        "a_pump": a_pump,
        "a_buy": a_buy,
        "tao_out": tao_out,
        "readd": readd,
        "fee": fee,
    }


def cycle_short_self_pump_buyback(
    *,
    x0: float,
    y0: float,
    w_q: float,
    phi: float,
    pump: float,
    coll: float,
    fee: float,
    readd: str,
) -> dict:
    """a15-shaped: self-pump then escrow buyback close."""
    w_b = 1.0 - w_q
    op = short_open(x0, y0, w_b, w_q, phi, fee)
    x, y = op["x"], op["y"]
    x, y, a_pump = buy_alpha(x, y, w_b, w_q, pump, fee)
    P, lift_tao, proceeds = op["P"], op["lift_tao"], op["proceeds"]
    pot = proceeds + coll
    # Cost to buy back P
    lo, hi = 0.0, max(pot, y) * 5 + 1.0
    for _ in range(80):
        mid = 0.5 * (lo + hi)
        _, _, got = buy_alpha(x, y, w_b, w_q, mid, fee)
        if got < P:
            lo = mid
        else:
            hi = mid
    cost = hi
    if cost > pot + 1e-9:
        return {"error": "InsufficientEscrow", "cost": cost, "pot": pot, "phi": phi}
    x, y, _ = buy_alpha(x, y, w_b, w_q, cost, fee)
    refund = pot - cost
    if readd == "buggy":
        w_b, w_q, x, y = update_weights_for_added_liquidity(
            x, y, w_b, w_q, P, lift_tao
        )
    else:
        x, y = x + P, y + lift_tao
    x, y, tao_out = sell_alpha(x, y, w_b, w_q, a_pump, fee)
    # free: -coll -pump + refund + tao_out; refund = proceeds+coll-cost
    free_delta = -pump + proceeds - cost + tao_out
    return {
        "phi": phi,
        "P": P,
        "lift_tao": lift_tao,
        "proceeds": proceeds,
        "cost": cost,
        "w_q_after": w_q,
        "y_end": y,
        "free_delta": free_delta,
        "subnet_tao_delta": y - y0,
        "tao_out": tao_out,
        "readd": readd,
        "fee": fee,
    }


def main() -> None:
    # --- a13 from REPORT ---
    y0, x0 = 7449.956, 3_153_861.0
    w_q = 0.471657
    w_b = 1.0 - w_q
    # Infer φ from post-short SubnetTAO (fee-free identity y_s = y(1-φ)^{1/w_q})
    y_s = 6758.050
    phi = 1.0 - (y_s / y0) ** w_q
    out = {
        "notation": {
            "phi": "lift fraction P/x (short)",
            "w_q": "quote/TAO weight (REPORT prints this as w1)",
            "w_b": "base/alpha weight",
            "fee_eps": "swap fee on input; base case 0",
        },
        "identities_fee0": {
            "y_after_short": "y * (1-phi)^(1/w_q)",
            "L_after_lift": "(1-phi) * L",
            "force_sell_preserves_L": True,
        },
        "a13": {},
        "a15": {},
    }
    for fee in (0.0, 33 / 65535):
        for readd in ("buggy", "keep"):
            r = cycle_short_pump_wallet(
                x0=x0,
                y0=y0,
                w_q=w_q,
                phi=phi,
                pump=9000.0,
                buy_tao=2363.0,
                coll=1500.0,
                fee=fee,
                readd=readd,
            )
            key = f"fee={fee:.6f}/{readd}"
            out["a13"][key] = {
                "phi": r["phi"],
                "free_delta": r["free_delta"],
                "subnet_tao_delta": r["subnet_tao_delta"],
                "w_q_after": r["w_q_after"],
                "y_end": r["y_end"],
            }
    out["a13"]["report"] = {
        "free_delta": 289.269,
        "subnet_tao_delta": -297.594,
        "w_q_after": 0.450646,
        "y_end": 7152.362,
        "phi_inferred": phi,
    }
    # Check fee0 short identity
    op0 = short_open(x0, y0, w_b, w_q, phi, 0.0)
    out["a13"]["fee0_short_identity"] = {
        "y_s_sim": op0["y"],
        "y_s_cf": op0["y_s_closed_form"],
        "L_sim": op0["L"],
        "L_cf": (1.0 - phi) * invariant(x0, y0, w_b, w_q),
    }

    # --- a15 ---
    y0b, x0b = 7152.475, 3_153_861.0
    w_qb = 0.450710
    # Fit φ so buggy fee0 profit ≈ 121.7 (Cap/LTV sized short)
    best = None
    for i in range(200, 600):
        ph = i / 10000.0
        r = cycle_short_self_pump_buyback(
            x0=x0b,
            y0=y0b,
            w_q=w_qb,
            phi=ph,
            pump=7000.0,
            coll=1200.0,
            fee=0.0,
            readd="buggy",
        )
        if "error" in r:
            continue
        err = abs(r["free_delta"] - 121.699)
        if best is None or err < best[0]:
            best = (err, ph, r)
    assert best is not None
    _, phi15, r15 = best
    out["a15"]["phi_fit"] = phi15
    for fee in (0.0, 33 / 65535):
        for readd in ("buggy", "keep"):
            r = cycle_short_self_pump_buyback(
                x0=x0b,
                y0=y0b,
                w_q=w_qb,
                phi=phi15,
                pump=7000.0,
                coll=1200.0,
                fee=fee,
                readd=readd,
            )
            key = f"fee={fee:.6f}/{readd}"
            out["a15"][key] = {
                k: r[k]
                for k in (
                    "free_delta",
                    "subnet_tao_delta",
                    "w_q_after",
                    "y_end",
                    "phi",
                )
                if k in r
            } | ({"error": r["error"]} if "error" in r else {})
    out["a15"]["report"] = {
        "free_delta": 121.699,
        "subnet_tao_delta": -127.946,
        "w_q_after": 0.438945,
    }

    # μ Sept 2026 cross-check: their (1-φ)^{1/w2} equals our y_s/y at fee=0
    out["mu_cross_check"] = {
        "phi_meaning": "lift fraction (NOT fee)",
        "w2_meaning": "quote weight w_q",
        "identity": "y_s/y = (1-phi)^(1/w_q) at fee=0",
        "numeric_a13": {
            "y_s_over_y": op0["y"] / y0,
            "one_minus_phi_pow": (1.0 - phi) ** (1.0 / w_q),
        },
        "cite": "September 2026 independent derivation; fees ignored there",
    }

    dest = Path(__file__).resolve().parent / "no_arbitrage_derive.json"
    dest.write_text(json.dumps(out, indent=2) + "\n")
    print(json.dumps(out, indent=2))
    print(f"wrote {dest}")


if __name__ == "__main__":
    main()
