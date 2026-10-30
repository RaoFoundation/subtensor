#!/usr/bin/env python3
"""Slippage multiplier vs lift fraction φ at side-cap maxima.

While a short lift of fraction φ is outstanding, reserves are scaled to
(1-φ) of the un-lifted book (protocol custody holds the rest). For a small
trade the local depth therefore shrinks by (1-φ), so relative slippage is
about 1/(1-φ) times the un-lifted slippage.

Chart: M(φ)=1/(1-φ) for each live subnet up to its Cap_short φ_max, with
the cap marked. Empirical check: ratio of α-slippage on a fixed τ trade
at lifted depth vs unlifted.
"""

from __future__ import annotations

import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

from no_arbitrage_cycle_search import (
    Pool,
    apply_buys_split,
    load_reserves,
    side_caps,
)

HERE = Path(__file__).resolve().parent


def price_impact_buy(pool: Pool, trade_tao: float) -> float:
    """Fractional spot move from a TAO→alpha buy: (p1-p0)/p0."""
    p0 = pool.price()
    pool1, _got, _n = apply_buys_split(pool, trade_tao, 1, fee=0.0)
    p1 = pool1.price()
    return (p1 - p0) / p0 if p0 > 0 else float("nan")


def empirical_multiplier(pool0: Pool, phi: float, trade_tao: float) -> float:
    """Depth-only M: impact on post-lift book / impact on unlifted book.

    Lift alone scales reserves to (1-φ) at unchanged spot. A fixed-τ trade
    then has ≈1/(1-φ) the fractional price impact of the same trade on the
    unlifted book. (Force-sell is excluded here: it moves the mark; the
    shallowness claim is the lift itself.)
    """
    if phi <= 0 or phi >= 0.95:
        return float("nan")
    imp0 = price_impact_buy(pool0, trade_tao)
    lifted = Pool(
        y=(1.0 - phi) * pool0.y,
        x=(1.0 - phi) * pool0.x,
        w1=pool0.w1,
        w2=pool0.w2,
    )
    imp1 = price_impact_buy(lifted, trade_tao)
    if imp0 == 0 or not np.isfinite(imp0) or not np.isfinite(imp1):
        return float("nan")
    return imp1 / imp0


def main() -> None:
    data = load_reserves()
    params = data["params"]
    subnets = data["subnets"]

    fig, ax = plt.subplots(figsize=(7.2, 4.4))
    colors = ["#1b4f72", "#b9770e", "#196f3d", "#6c3483", "#922b21"]

    rows = []
    phi_line = np.linspace(0.0, 0.5, 400)
    ax.plot(
        phi_line,
        1.0 / (1.0 - phi_line),
        color="#333333",
        lw=1.2,
        ls="--",
        label=r"theory $1/(1-\varphi)$",
    )

    for i, snap in enumerate(subnets):
        y, x, w_q = snap["y"], snap["x"], snap["w_q"]
        w_b = 1.0 - w_q
        caps = side_caps(
            y,
            w_b,
            floor_bps=params["floor_bps"],
            thin_floor_bps=params["thin_floor_bps"],
            ceiling_bps=params["ceiling_bps"],
            split_short_bps=params["split_short_bps"],
            thin_tao=params["thin_tao"],
            ref_tao=params["ref_tao"],
            move_bps=params["move_bps"],
        )
        phi_max = caps["phi_max_short"]
        phis = np.linspace(0.0, phi_max, 60)
        theory = 1.0 / (1.0 - phis)
        color = colors[i % len(colors)]
        label = f"SN{snap['netuid']} {snap['name']} (cap {phi_max:.3f})"
        ax.plot(phis, theory, color=color, lw=2.0, label=label)
        # Cap marker
        m_cap = 1.0 / (1.0 - phi_max)
        ax.scatter([phi_max], [m_cap], color=color, s=36, zorder=5)

        # Empirical at mid and cap (small trade = 0.1% of y)
        pool0 = Pool(y=y, x=x, w1=w_b, w2=w_q)
        trade = 0.001 * y
        emp_mid = empirical_multiplier(pool0, 0.5 * phi_max, trade)
        emp_cap = empirical_multiplier(pool0, phi_max, trade)
        rows.append(
            {
                "netuid": snap["netuid"],
                "name": snap["name"],
                "y": y,
                "thin": caps["thin"],
                "phi_max_short": phi_max,
                "Cap_short": caps["Cap_short"],
                "M_theory_at_cap": m_cap,
                "M_emp_mid": emp_mid,
                "M_emp_cap": emp_cap,
                "trade_tao": trade,
            }
        )

    ax.set_xlabel(r"lift fraction $\varphi$")
    ax.set_ylabel(r"slippage multiplier $M(\varphi)$")
    ax.set_title("Lifted-pool slippage vs $\\varphi$ (side-cap marked)")
    ax.set_xlim(0, 0.52)
    ax.set_ylim(1.0, 2.2)
    ax.grid(True, alpha=0.3)
    ax.legend(fontsize=8, loc="upper left")
    fig.tight_layout()

    png = HERE.parent / "no-arbitrage-slippage.png"
    pdf = HERE.parent / "no-arbitrage-slippage.pdf"
    fig.savefig(png, dpi=160)
    fig.savefig(pdf)
    plt.close(fig)

    meta = {
        "note": (
            "M(φ)=1/(1-φ) from lift alone (reserves ×(1-φ), spot unchanged). "
            "Markers are Cap_short/y. Empirical M is fractional spot impact "
            "of a 0.1%·y fee-free buy on the lifted book over the same buy "
            "on the unlifted book."
        ),
        "subnets": rows,
    }
    (HERE / "no_arbitrage_slippage.json").write_text(
        json.dumps(meta, indent=2) + "\n"
    )
    print(json.dumps(meta, indent=2))
    print(f"wrote {png}", flush=True)
    print(f"wrote {pdf}", flush=True)


if __name__ == "__main__":
    main()
