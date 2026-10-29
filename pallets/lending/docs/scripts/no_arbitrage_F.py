#!/usr/bin/env python3
"""Deprecated helper — see no_arbitrage_derive.py for the pallet-first model.

μ's September 2026 factor used φ as the *lift fraction* (fees ignored).
An earlier draft of no-arbitrage.tex mis-read φ as a swap fee; that is wrong.
This file retains μ's algebraic expression only as a numeric cross-check of
the identity y_s/y = (1-φ)^(1/w_q), not as our derivation.
"""

from __future__ import annotations

import json
from pathlib import Path


def mu_F_sept2026(f_h: float, phi_lift: float, w_q: float) -> float:
    """μ's fee-free factor (Sept 2026). phi_lift = lift fraction, w_q = quote weight."""
    if f_h <= 0:
        return 1.0
    one_m = (1.0 - phi_lift) ** (1.0 / w_q)
    base = one_m / (one_m + f_h - phi_lift)
    inner = 1.0 - (1.0 - one_m) / (f_h - phi_lift + 1.0)
    if inner <= 0:
        return float("nan")
    exp = (f_h + 1.0) / (
        (f_h + 1.0 + phi_lift) * (inner ** (w_q / (1.0 - w_q)))
    )
    return (f_h + 1.0) * (base**exp)


def main() -> None:
    # Cross-check identity only (a13 numbers)
    y, y_s, w_q, phi = 7449.956, 6758.050, 0.471657, None
    phi = 1.0 - (y_s / y) ** w_q
    out = {
        "warning": "Use no_arbitrage_derive.py for profitability. This is μ cross-check only.",
        "phi_lift": phi,
        "y_s_over_y": y_s / y,
        "one_minus_phi_pow": (1.0 - phi) ** (1.0 / w_q),
        "mu_F_samples": [
            {"f_h": f, "F": mu_F_sept2026(f, phi, w_q)}
            for f in (0.01, 0.05, 0.10, 0.165, 0.25)
        ],
    }
    dest = Path(__file__).resolve().parent / "no_arbitrage_F.json"
    dest.write_text(json.dumps(out, indent=2) + "\n")
    print(json.dumps(out, indent=2))
    print(f"wrote {dest}")


if __name__ == "__main__":
    main()
