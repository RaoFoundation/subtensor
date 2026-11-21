#!/usr/bin/env python3
"""The shogting cycle: closed forms, fresh-fork base case, ROI plot.

Companion to no-arbitrage.tex. The *shogting cycle* is the single-wallet
TAO-pure sequence

    open short → stake/pump → close → unstake

with attacker budget V = h · y (h = budget / tao_in).

Fresh base case uses Finney mainnet SN29 at the lending-clone fork block
#9,378,125 where SwapBalancer quote weight is 0.5 (not the contaminated
post-a12/a13/a15 chain).

Readd modes:
  unfixed — update_weights_for_added_liquidity at the post-pump mark
  fixed   — keep-weights credit (shipped root fix)
"""

from __future__ import annotations

import json
import math
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

HERE = Path(__file__).resolve().parent
DOCS = HERE.parent

FEE_PROD = 33 / 65535
LTV = 0.25
PHI_MAX_SHORT_DEEP = 0.165  # Cap_short/y on deep floor-tier pools

# Finney SN29 at clone fork block 9_378_125 (archive-verified).
FORK = {
    "block": 9_378_125,
    "netuid": 29,
    "y": 7897.952225,
    "x": 3153658.330377,
    "w_q": 0.499999998580193,  # ≈ 1/2
}


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


def buy_exact(
    x: float, y: float, w_b: float, w_q: float, need: float, fee: float
) -> tuple[float, float, float]:
    """TAO cost to buy exactly `need` alpha (binary search)."""
    if need <= 0:
        return x, y, 0.0
    lo, hi = 0.0, y * 50 + 1.0
    best_x, best_y = x, y
    for _ in range(80):
        mid = 0.5 * (lo + hi)
        x1, y1, got = buy_alpha(x, y, w_b, w_q, mid, fee)
        if got < need:
            lo = mid
        else:
            hi = mid
            best_x, best_y = x1, y1
    return best_x, best_y, hi


def update_weights(
    x: float, y: float, w_b: float, w_q: float, d_alpha: float, d_tao: float
) -> tuple[float, float, float, float]:
    x1, y1 = x + d_alpha, y + d_tao
    q_b = w_b * y * x1
    q_q = w_q * x * y1
    w_q1 = q_q / (q_b + q_q)
    return 1.0 - w_q1, w_q1, x1, y1


def size_shogting(h: float, *, lam: float = LTV, phi_max: float = PHI_MAX_SHORT_DEEP) -> dict:
    """Budget V=h·y → (φ, collateral fraction, pump fraction).

    First-principles split: when unconstrained, set C = λ φ y and
    S = φ y, i.e. φ = h/(1+λ), so collateral and a same-size pump share
    the budget. Capacity then caps φ at φ_max and extra budget pumps.
    """
    if h <= 0:
        return {"phi": 0.0, "feasible": False}
    phi_unc = h / (1.0 + lam)
    phi = min(phi_max, phi_unc)
    # C/y = λ φ; S/y = h - λ φ
    c_over_y = lam * phi
    s_over_y = h - c_over_y
    feasible = s_over_y >= -1e-15 and phi > 0
    return {
        "phi": phi,
        "c_over_y": c_over_y,
        "s_over_y": max(0.0, s_over_y),
        "capacity_bound": phi_unc > phi_max + 1e-15,
        "feasible": feasible,
    }


def _pump_then_buy_cost(
    x_s: float,
    y_s: float,
    w_b: float,
    w_q: float,
    S: float,
    P: float,
    fee: float,
) -> tuple[float, float, float, float, float]:
    """After open state (x_s,y_s): pump S, then TAO cost to buy P. Returns
    (x_after_buy, y_after_buy, a_pump, B, a_buy)."""
    x, y, a_pump = buy_alpha(x_s, y_s, w_b, w_q, S, fee)
    x, y, B = buy_exact(x, y, w_b, w_q, P, fee)
    return x, y, a_pump, B, P


def shogting_cycle(
    *,
    x0: float,
    y0: float,
    w_q: float,
    h: float,
    fee: float,
    readd: str,  # "unfixed" | "fixed"
    lam: float = LTV,
    phi_max: float = PHI_MAX_SHORT_DEEP,
) -> dict:
    """Single-wallet shogting cycle (wallet-close). ROI = V'/V - 1.

    Budget V = h·y posts collateral C and splits the remainder between
    pump S and repay-buy B so that S + B = V - C (wallet-close solvency).
    Close burns P from wallet, refunds E+C, readds lift legs, unstakes
    residual alpha (pump inventory; repay buy is exact).
    """
    w_b = 1.0 - w_q
    sizing = size_shogting(h, lam=lam, phi_max=phi_max)
    if not sizing["feasible"]:
        return {"error": "infeasible", **sizing}
    phi = sizing["phi"]
    V = h * y0
    C = sizing["c_over_y"] * y0
    remainder = V - C
    if remainder <= 0:
        return {"error": "no_remainder", "phi": phi, "h": h}
    P = phi * x0
    lift_tao = phi * y0

    # Open: lift + force-sell
    x_s, y_s = x0 - P, y0 - lift_tao
    x_s, y_s, E = sell_alpha(x_s, y_s, w_b, w_q, P, fee)

    # Binary-search pump S ∈ [0, remainder] s.t. S + B(S) = remainder.
    lo, hi = 0.0, remainder
    S = 0.0
    x = y = a_pump = B = 0.0
    for _ in range(64):
        mid = 0.5 * (lo + hi)
        x, y, a_pump, B, _ = _pump_then_buy_cost(
            x_s, y_s, w_b, w_q, mid, P, fee
        )
        if mid + B > remainder:
            hi = mid  # pump too large → buy too expensive
        else:
            lo = mid
            S = mid
    x, y, a_pump, B, _ = _pump_then_buy_cost(
        x_s, y_s, w_b, w_q, S, P, fee
    )
    # Top up buy if rounding left dust budget
    dust = remainder - S - B
    if dust > 1e-9:
        # already bought exact P; dust stays as free TAO
        pass
    elif B + S > remainder + 1e-6:
        return {
            "error": "cannot_split_budget",
            "phi": phi,
            "h": h,
            "S": S,
            "B": B,
            "remainder": remainder,
        }

    free_dust = max(0.0, remainder - S - B)

    # Wallet repay: burn P, refund E+C; free gains E+C, already spent B
    # Readd
    w_b_r, w_q_r = w_b, w_q
    if readd == "unfixed":
        w_b_r, w_q_r, x, y = update_weights(x, y, w_b, w_q, P, lift_tao)
    elif readd == "fixed":
        x, y = x + P, y + lift_tao
    else:
        raise ValueError(readd)

    # Unstake pump inventory (repay buy was exact → no leftover alpha)
    x, y, tao_out = sell_alpha(x, y, w_b_r, w_q_r, a_pump, fee)

    # Accounting from initial V:
    # spent C+S+B, received E+C on repay, +tao_out unstake, +dust
    V_prime = V - C - S - B + E + C + tao_out + free_dust
    # = V - S - B + E + tao_out + free_dust
    roi = V_prime / V - 1.0 if V > 0 else float("nan")
    return {
        "h": h,
        "phi": phi,
        "V": V,
        "C": C,
        "S": S,
        "E": E,
        "B": B,
        "free_dust": free_dust,
        "a_pump": a_pump,
        "tao_out": tao_out,
        "V_prime": V_prime,
        "roi": roi,
        "roi_pct": 100.0 * roi,
        "w_q_after": w_q_r,
        "w_stays_half": abs(w_q_r - 0.5) < 1e-9,
        "y_end": y,
        "readd": readd,
        "fee": fee,
        "capacity_bound": sizing["capacity_bound"],
        "close": "wallet",
    }


def closed_form_fee0_equal_weight(h: float, y: float, x: float, **kw) -> dict:
    """Fee-free equal-weight closed form for the shogting cycle.

    With w_b = w_q = 1/2 and ε = 0, wallet close, V = h y, C = λ φ y,
    S+B = V-C (solvency split):

      y_s = y (1-φ)² ,   E = y φ (1-φ)
      after pump S: y_p = y_s+S ,  x_p = x √(y_s/y_p) ,  α_p = x-x_p
      B   = y_p ( 1/(1 - φ √(y_p/y_s))² - 1 )     [exact buy of P=φx]
      after buy: y_c = y_p+B ,  x_c = x_p √(y_p/y_c)

      fixed:   x_r = x_c+φx , y_r = y_c+φy , w = 1/2 unchanged
               Δy = y_r · α_p / (x_r + α_p)
      unfixed: (w_b',w_q') via update_weights_for_added_liquidity at
               (x_c,y_c); then Δy = y_r (1 - (x_r/(x_r+α_p))^{w_b'/w_q'})

      V' = V - S - B + E + Δy ,   ROI = V'/V - 1 .
    """
    out = {}
    for readd in ("unfixed", "fixed"):
        r = shogting_cycle(
            x0=x, y0=y, w_q=0.5, h=h, fee=0.0, readd=readd, **kw
        )
        out[readd] = r
    pieces = {
        "identity": "y_s/y=(1-φ)^2 at w_q=1/2, ε=0",
        "roi_identity": "ROI = (E - S - B + Δy_unstake)/V",
    }
    if "error" not in out.get("fixed", {}):
        r = out["fixed"]
        phi, S, B, E = r["phi"], r["S"], r["B"], r["E"]
        y_s = y * (1.0 - phi) ** 2
        y_p = y_s + S
        pieces.update(
            {
                "phi": phi,
                "y_s": y_s,
                "E": E,
                "S": S,
                "B": B,
                "y_p": y_p,
                "a_pump": r["a_pump"],
                "tao_out": r["tao_out"],
            }
        )
    out["pieces"] = pieces
    return out


def fresh_base_case() -> dict:
    """Shogting cycle on fork-state SN29 (w≈0.5), vs contaminated appendix."""
    y, x, w_q = FORK["y"], FORK["x"], FORK["w_q"]
    # Representative h at capacity-bound short with moderate pump
    # (matches a15-scale: ~8200 budget on ~7900 pool → h≈1.04)
    rows = {}
    for h in (0.05, 0.165, 0.5, 1.0, 2.0):
        rows[str(h)] = {}
        for fee in (0.0, FEE_PROD):
            for readd in ("unfixed", "fixed"):
                r = shogting_cycle(
                    x0=x, y0=y, w_q=w_q, h=h, fee=fee, readd=readd
                )
                rows[str(h)][f"fee={fee:.6f}/{readd}"] = {
                    k: r.get(k)
                    for k in (
                        "roi_pct",
                        "phi",
                        "w_q_after",
                        "w_stays_half",
                        "V",
                        "V_prime",
                        "error",
                    )
                    if k in r or True
                }
    # Weight permanence at a workable h
    demo_h = 0.5
    weight_demo = {
        readd: shogting_cycle(
            x0=x, y0=y, w_q=0.5, h=demo_h, fee=0.0, readd=readd
        )
        for readd in ("unfixed", "fixed")
    }
    return {
        "fork": FORK,
        "note": (
            "w_q ≈ 1/2 at fork. Fixed (keep-weights) leaves w_q = 1/2; "
            "unfixed rewrites w_q away from 1/2."
        ),
        "weight_demo_h": demo_h,
        "weight_demo": {
            k: {
                "w_q_after": v.get("w_q_after"),
                "w_stays_half": v.get("w_stays_half"),
                "roi_pct": v.get("roi_pct"),
            }
            for k, v in weight_demo.items()
        },
        "grid": rows,
        "closed_form_check_h0.5": closed_form_fee0_equal_weight(0.5, y, x),
    }


def contaminated_appendix() -> dict:
    """Sequential a12→a13→a15 contaminated books (labelled appendix only)."""
    return {
        "warning": (
            "These w_q values are NOT fresh mainnet. They are the result of "
            "sequential attack runs on one clone chain (a12 then a13 then a15)."
        ),
        "a13": {
            "y": 7449.956,
            "x": 3_153_861.0,
            "w_q": 0.471657,
            "note": "starts after a12 drained; printed report w1=0.4717",
        },
        "a15": {
            "y": 7152.475,
            "x": 3_153_861.0,
            "w_q": 0.450710,
            "note": "starts after a13; printed report w1=0.4507",
        },
    }


def roi_sweep(
    *,
    y: float,
    x: float,
    w_q: float,
    h_min: float = 0.01,
    h_max: float = 10.0,
    n: int = 80,
) -> dict:
    hs = np.geomspace(h_min, h_max, n)
    series = {
        "h": hs.tolist(),
        "unfixed_fee0": [],
        "fixed_fee0": [],
        "unfixed_fee_prod": [],
        "fixed_fee_prod": [],
    }
    for h in hs:
        for readd, fee, key in (
            ("unfixed", 0.0, "unfixed_fee0"),
            ("fixed", 0.0, "fixed_fee0"),
            ("unfixed", FEE_PROD, "unfixed_fee_prod"),
            ("fixed", FEE_PROD, "fixed_fee_prod"),
        ):
            r = shogting_cycle(
                x0=x, y0=y, w_q=w_q, h=float(h), fee=fee, readd=readd
            )
            series[key].append(
                r.get("roi_pct") if "error" not in r else float("nan")
            )
    return series


def plot_roi(series: dict, dest_png: Path, dest_pdf: Path) -> None:
    h = np.array(series["h"])
    fig, ax = plt.subplots(figsize=(7.4, 4.6))
    styles = [
        ("unfixed_fee0", "unfixed, ε=0", "#922b21", "-"),
        ("unfixed_fee_prod", "unfixed, ε=33/65535", "#c0392b", "--"),
        ("fixed_fee0", "fixed (keep-w), ε=0", "#1a5276", "-"),
        ("fixed_fee_prod", "fixed (keep-w), ε=33/65535", "#2980b9", "--"),
    ]
    for key, label, color, ls in styles:
        ax.plot(h, series[key], color=color, ls=ls, lw=1.8, label=label)
    ax.axhline(0.0, color="#333", lw=0.8)
    ax.set_xscale("log")
    ax.set_xlim(0.01, 10)
    ax.set_xlabel(r"$h = \mathrm{budget}/y$  (log; $y=\mathrm{SubnetTAO}$)")
    ax.set_ylabel(r"shogting-cycle ROI (%)  $= 100\,(V'/V - 1)$")
    ax.set_title("Shogting cycle on fresh-fork SN29 (w_q = 1/2): unfixed vs keep-weights")
    ax.grid(True, which="both", alpha=0.3)
    ax.legend(fontsize=8, loc="upper left")
    fig.tight_layout()
    fig.savefig(dest_png, dpi=160)
    fig.savefig(dest_pdf)
    plt.close(fig)


def main() -> None:
    base = fresh_base_case()
    appendix = contaminated_appendix()
    series = roi_sweep(y=FORK["y"], x=FORK["x"], w_q=0.5)

    png = DOCS / "no-arbitrage-shogting-roi.png"
    pdf = DOCS / "no-arbitrage-shogting-roi.pdf"
    plot_roi(series, png, pdf)

    # Compact closed-form summary for tex
    cf = closed_form_fee0_equal_weight(0.5, FORK["y"], FORK["x"])
    out = {
        "fresh_base_case": {
            "fork": base["fork"],
            "note": base["note"],
            "weight_demo": base["weight_demo"],
            "grid_roi_pct": {
                h: {
                    k: v.get("roi_pct")
                    for k, v in modes.items()
                }
                for h, modes in base["grid"].items()
            },
        },
        "contaminated_appendix": appendix,
        "closed_form_fee0_equal_weight_at_h0.5": {
            "pieces": cf["pieces"],
            "unfixed_roi_pct": cf["unfixed"].get("roi_pct"),
            "fixed_roi_pct": cf["fixed"].get("roi_pct"),
            "unfixed_w_q_after": cf["unfixed"].get("w_q_after"),
            "fixed_w_q_after": cf["fixed"].get("w_q_after"),
        },
        "roi_sweep_meta": {
            "h_range": [0.01, 10],
            "pool": {"y": FORK["y"], "x": FORK["x"], "w_q": 0.5},
            "png": str(png.name),
            "pdf": str(pdf.name),
        },
        "roi_sweep": series,
    }
    dest = HERE / "no_arbitrage_shogting.json"
    # Drop bulky full sweep arrays from the printed summary path — keep in file
    dest.write_text(json.dumps(out, indent=2) + "\n")
    summary = {
        "fork_w_q": FORK["w_q"],
        "weight_demo": base["weight_demo"],
        "cf_h0.5": out["closed_form_fee0_equal_weight_at_h0.5"],
        "png": str(png),
        "pdf": str(pdf),
    }
    print(json.dumps(summary, indent=2))
    print(f"wrote {dest}")
    print(f"wrote {png}")
    print(f"wrote {pdf}")


if __name__ == "__main__":
    main()
