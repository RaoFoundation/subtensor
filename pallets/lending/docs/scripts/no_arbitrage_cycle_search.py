#!/usr/bin/env python3
"""Bounded N-action no-arbitrage search under keep-weights readd.

Pass/fail (Jacob; companion to no-arbitrage.tex §criterion):

  PASS iff there is no self-contained sequence of at most N atomic actions
  — including multi-coldkey and trade-split variants — that yields attacker
  net TAO Δ > 0 for any lift fraction φ ∈ (0, φ_max], where φ_max is the
  side-cap maximum Cap_short / y (short) or Cap_long / y (long), on the
  real reserve snapshots in no_arbitrage_reserves.json.

Atomic actions: open_short, open_long, buy (stake), sell (unstake),
close_wallet, close_buyback. A k-way trade split of a buy/sell counts as
k actions. Multi-coldkey = separate pumper coldkey; Δ sums all attacker-
controlled free balances. Self-contained = start and end with no open loan
and no residual attacker alpha inventory (fully unwound).

Designs:
  buggy — readd via update_weights_for_added_liquidity (pre-fix)
  a     — keep-weights credit (shipped)
"""

from __future__ import annotations

import json
import math
import sys
from dataclasses import dataclass, replace
from pathlib import Path

PHI_FEE = 33 / 65535
EPS_PROFIT = 1e-6  # τ; pass/fail float tolerance
N_MAX = 8
K_SPLIT_MAX = 3

HERE = Path(__file__).resolve().parent


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


def buy_alpha(pool: Pool, tao_in: float, fee: float = PHI_FEE) -> tuple[Pool, float]:
    if tao_in <= 0:
        return pool, 0.0
    dy = tao_in * (1.0 - fee)
    ratio = pool.y / (pool.y + dy)
    dx = pool.x * (1.0 - ratio ** (pool.w2 / pool.w1))
    return replace(pool, y=pool.y + dy, x=pool.x - dx), dx


def sell_alpha(pool: Pool, alpha_in: float, fee: float = PHI_FEE) -> tuple[Pool, float]:
    if alpha_in <= 0:
        return pool, 0.0
    dx = alpha_in * (1.0 - fee)
    ratio = pool.x / (pool.x + dx)
    dy = pool.y * (1.0 - ratio ** (pool.w1 / pool.w2))
    return replace(pool, y=pool.y - dy, x=pool.x + dx), dy


def buy_exact_alpha(
    pool: Pool, need: float, fee: float = PHI_FEE
) -> tuple[Pool, float]:
    """Binary-search TAO cost to buy at least `need` alpha."""
    if need <= 0:
        return pool, 0.0
    lo, hi = 0.0, pool.y * 20 + 1.0
    best = pool
    for _ in range(64):
        mid = 0.5 * (lo + hi)
        p_try, got = buy_alpha(pool, mid, fee)
        if got < need:
            lo = mid
        else:
            hi = mid
            best = p_try
    return best, hi


def update_weights_for_add(pool: Pool, dy: float, dx: float) -> Pool:
    y1, x1 = pool.y + dy, pool.x + dx
    q1 = pool.w1 * pool.y * x1
    q2 = pool.w2 * pool.x * y1
    s = q1 + q2
    if s <= 0:
        return replace(pool, y=y1, x=x1)
    w2 = q2 / s
    w2 = min(max(w2, 1e-6), 1.0 - 1e-6)
    return Pool(y=y1, x=x1, w1=1.0 - w2, w2=w2)


def credit_keep_w(pool: Pool, dy: float, dx: float) -> Pool:
    return replace(pool, y=pool.y + dy, x=pool.x + dx)


def y_star(w_b: float, ref_tao: float = 1000.0, move_bps: int = 1000) -> float:
    return ref_tao / ((1.0 + move_bps / 10_000.0) ** w_b - 1.0)


def side_caps(
    y: float,
    w_b: float,
    *,
    floor_bps: int = 3300,
    thin_floor_bps: int = 1500,
    ceiling_bps: int = 5000,
    split_short_bps: int = 5000,
    thin_tao: float = 3000.0,
    ref_tao: float = 1000.0,
    move_bps: int = 1000,
) -> dict:
    """Mirror LiftParams::side_caps_at at full ramp, G_ref = y (no open book)."""
    g = y
    f_bps = thin_floor_bps if g < thin_tao else floor_bps
    floor_cap = g * f_bps / 10_000.0
    ceiling_cap = g * ceiling_bps / 10_000.0
    impact_cap = max(0.0, g - y_star(w_b, ref_tao, move_bps))
    raw_cap = max(floor_cap, min(impact_cap, ceiling_cap))
    cap_short = raw_cap * split_short_bps / 10_000.0
    cap_long = raw_cap - cap_short
    return {
        "G": g,
        "thin": g < thin_tao,
        "Cap": raw_cap,
        "Cap_short": cap_short,
        "Cap_long": cap_long,
        "phi_max_short": cap_short / y if y > 0 else 0.0,
        "phi_max_long": cap_long / y if y > 0 else 0.0,
        "y_star": y_star(w_b, ref_tao, move_bps),
    }


def apply_buys_split(
    pool: Pool, tao_total: float, n_splits: int, fee: float
) -> tuple[Pool, float, int]:
    """Split a buy into n equal slices. Returns (pool, alpha_got, n_actions)."""
    n = max(1, n_splits)
    slice_tao = tao_total / n
    got = 0.0
    for _ in range(n):
        pool, dx = buy_alpha(pool, slice_tao, fee)
        got += dx
    return pool, got, n


def apply_sells_split(
    pool: Pool, alpha_total: float, n_splits: int, fee: float
) -> tuple[Pool, float, int]:
    n = max(1, n_splits)
    slice_a = alpha_total / n
    got = 0.0
    for _ in range(n):
        pool, dy = sell_alpha(pool, slice_a, fee)
        got += dy
    return pool, got, n


@dataclass
class Trial:
    design: str
    netuid: int
    name: str
    side: str
    close: str
    phi: float
    pump_frac: float
    n_pump_splits: int
    n_repay_splits: int
    n_unwind_splits: int
    multi_ck: bool
    n_actions: int
    profit: float
    w2_after: float
    L_ratio: float


def run_short_sequence(
    pool0: Pool,
    *,
    phi: float,
    pump_frac: float,
    close: str,
    design: str,
    n_pump_splits: int = 1,
    n_repay_splits: int = 1,
    n_unwind_splits: int = 1,
    multi_ck: bool = True,
    fee: float = PHI_FEE,
) -> Trial | None:
    """Self-contained short cycle; returns None if infeasible / over N budget later."""
    if phi <= 0 or phi >= 0.49:
        return None
    y0, x0 = pool0.y, pool0.x
    P = phi * x0
    lift_tao = phi * y0
    L0 = pool0.L()

    # Open short (1 action)
    n_actions = 1
    pool = replace(pool0, y=y0 - lift_tao, x=x0 - P)
    pool, proceeds = sell_alpha(pool, P, fee)
    # Force-sell is part of open extrinsic → not a separate action.
    coll = 0.25 * (pool0.price() * P)

    free_attacker = -coll
    free_pumper = 0.0
    alpha_pump = 0.0
    alpha_repay_extra = 0.0

    # Pump (k actions)
    pump_tao = pump_frac * y0
    if pump_tao > 0:
        pool, alpha_pump, n_p = apply_buys_split(pool, pump_tao, n_pump_splits, fee)
        n_actions += n_p
        if multi_ck:
            free_pumper -= pump_tao
        else:
            free_attacker -= pump_tao

    if close == "buyback":
        # Escrow spends proceeds (+ top-up) to buy P; count repay buys as actions.
        need = P
        # Approximate: binary search total cost, then apply as n_repay_splits slices.
        cost_pool, cost = buy_exact_alpha(pool, need, fee)
        # Re-apply as splits from original post-pump pool for path fidelity.
        if n_repay_splits <= 1:
            pool = cost_pool
            n_actions += 1
            spent = cost
        else:
            # Equal TAO slices totaling `cost`
            pool, got, n_r = apply_buys_split(pool, cost, n_repay_splits, fee)
            n_actions += n_r
            spent = cost
            if got < need * 0.999:
                # top-up remainder as one more action
                pool, extra = buy_exact_alpha(pool, need - got, fee)
                spent += extra
                n_actions += 1
        if spent > proceeds:
            free_attacker -= spent - proceeds
        else:
            free_attacker += proceeds - spent
        free_attacker += coll
        dy_c, dx_c = lift_tao, P
        # buyback burns P from escrow path; no wallet alpha to unwind from repay
    elif close == "wallet":
        # Buy P alpha (possibly split), repay, refund proceeds+coll
        if n_repay_splits <= 1:
            pool, cost = buy_exact_alpha(pool, P, fee)
            n_actions += 1
            free_attacker -= cost
        else:
            # Estimate cost then split
            _, cost_est = buy_exact_alpha(pool, P, fee)
            pool, got, n_r = apply_buys_split(pool, cost_est, n_repay_splits, fee)
            n_actions += n_r
            free_attacker -= cost_est
            if got < P * 0.999:
                pool, extra = buy_exact_alpha(pool, P - got, fee)
                free_attacker -= extra
                n_actions += 1
            alpha_repay_extra = max(0.0, got - P)
        free_attacker += proceeds + coll
        dy_c, dx_c = lift_tao, P
        n_actions += 1  # close_wallet
    else:
        return None

    if close == "buyback":
        n_actions += 1  # close_buyback extrinsic

    # Readd (protocol; not an attacker action)
    if design == "buggy":
        pool = update_weights_for_add(pool, dy_c, dx_c)
    elif design == "a":
        pool = credit_keep_w(pool, dy_c, dx_c)
    else:
        return None

    # Unwind residual alpha (pump inventory + any overbuy)
    residual = alpha_pump + alpha_repay_extra
    if residual > 0:
        if multi_ck and alpha_pump > 0:
            # Pumper unwinds their alpha; attacker unwinds overbuy if any
            pool, dy_p, n_u = apply_sells_split(
                pool, alpha_pump, n_unwind_splits, fee
            )
            n_actions += n_u
            free_pumper += dy_p
            if alpha_repay_extra > 0:
                pool, dy_a, n_u2 = apply_sells_split(
                    pool, alpha_repay_extra, 1, fee
                )
                n_actions += n_u2
                free_attacker += dy_a
        else:
            pool, dy, n_u = apply_sells_split(
                pool, residual, n_unwind_splits, fee
            )
            n_actions += n_u
            free_attacker += dy

    profit = free_attacker + free_pumper
    return Trial(
        design=design,
        netuid=0,
        name="",
        side="short",
        close=close,
        phi=phi,
        pump_frac=pump_frac,
        n_pump_splits=n_pump_splits,
        n_repay_splits=n_repay_splits,
        n_unwind_splits=n_unwind_splits,
        multi_ck=multi_ck,
        n_actions=n_actions,
        profit=profit,
        w2_after=pool.w2,
        L_ratio=pool.L() / L0 if L0 else float("nan"),
    )


def run_long_sequence(
    pool0: Pool,
    *,
    phi: float,
    dump_frac: float,
    close: str,
    design: str,
    n_dump_splits: int = 1,
    n_unwind_splits: int = 1,
    multi_ck: bool = False,
    fee: float = PHI_FEE,
) -> Trial | None:
    if phi <= 0 or phi >= 0.49:
        return None
    pool = pool0
    L0 = pool0.L()
    free = 0.0
    n_actions = 0

    dump_alpha = dump_frac * pool0.x
    if dump_alpha > 0:
        # Cost inventory at start spot (multi_ck: separate dumper funded at spot)
        cost0 = pool0.price() * dump_alpha
        free -= cost0
        pool, dy, n_d = apply_sells_split(pool, dump_alpha, n_dump_splits, fee)
        free += dy
        n_actions += n_d

    P = phi * pool.y
    if P <= 0 or P >= pool.y * 0.49:
        return None
    lift_tao = P
    lift_alpha = phi * pool.x
    n_actions += 1  # open_long

    pool = replace(pool, y=pool.y - lift_tao, x=pool.x - lift_alpha)
    pool, A = buy_alpha(pool, P, fee)  # force-buy inside open

    if close == "wallet":
        free -= P
        free_alpha = A
        n_actions += 1
        dy_c, dx_c = lift_tao, lift_alpha
    elif close == "buyback":
        pool, recovered = sell_alpha(pool, A, fee)
        free += recovered - P
        free_alpha = 0.0
        n_actions += 1
        dy_c, dx_c = lift_tao, lift_alpha
    else:
        return None

    if design == "buggy":
        pool = update_weights_for_add(pool, dy_c, dx_c)
    elif design == "a":
        pool = credit_keep_w(pool, dy_c, dx_c)
    else:
        return None

    if free_alpha > 0:
        pool, dy, n_u = apply_sells_split(pool, free_alpha, n_unwind_splits, fee)
        free += dy
        n_actions += n_u

    return Trial(
        design=design,
        netuid=0,
        name="",
        side="long",
        close=close,
        phi=phi,
        pump_frac=dump_frac,
        n_pump_splits=n_dump_splits,
        n_repay_splits=1,
        n_unwind_splits=n_unwind_splits,
        multi_ck=multi_ck,
        n_actions=n_actions,
        profit=free,
        w2_after=pool.w2,
        L_ratio=pool.L() / L0 if L0 else float("nan"),
    )


def load_reserves() -> dict:
    return json.loads((HERE / "no_arbitrage_reserves.json").read_text())


def phi_grid(phi_max: float) -> list[float]:
    """φ samples in (0, φ_max], denser near the cap."""
    if phi_max <= 0:
        return []
    fracs = [0.05, 0.1, 0.2, 0.35, 0.5, 0.65, 0.8, 0.9, 0.95, 1.0]
    out = sorted({round(phi_max * f, 8) for f in fracs if phi_max * f > 1e-6})
    return out


def search(n_max: int = N_MAX) -> dict:
    data = load_reserves()
    params = data["params"]
    snapshots = list(data["subnets"])
    # Include clone skew as an extra fixture (does not affect live pass/fail name)
    snapshots.append(data["clone_sn29_skew"])

    pump_fracs = [0.0, 0.25, 0.5, 0.75, 1.0, 1.25]
    dump_fracs = [0.0, 0.1, 0.25, 0.5]
    designs = ["buggy", "a"]
    closes = ["buyback", "wallet"]

    rows: list[Trial] = []
    subnet_meta: list[dict] = []

    for snap in snapshots:
        y, x, w_q = snap["y"], snap["x"], snap["w_q"]
        w_b = 1.0 - w_q
        pool0 = Pool(y=y, x=x, w1=w_b, w2=w_q)
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
        is_clone = "clone" in snap.get("name", "") or "clone" in str(
            snap.get("note", "")
        )
        subnet_meta.append(
            {
                "netuid": snap["netuid"],
                "name": snap["name"],
                "y": y,
                "x": x,
                "w_q": w_q,
                "clone_fixture": is_clone,
                **caps,
            }
        )

        for design in designs:
            for phi in phi_grid(caps["phi_max_short"]):
                for pump in pump_fracs:
                    for n_ps in range(1, K_SPLIT_MAX + 1):
                        for n_rs in range(1, K_SPLIT_MAX + 1):
                            for n_us in range(1, K_SPLIT_MAX + 1):
                                for multi in (True, False):
                                    for close in closes:
                                        # Skip redundant split combos when no pump
                                        if pump == 0.0 and (
                                            n_ps > 1 or n_us > 1
                                        ):
                                            continue
                                        r = run_short_sequence(
                                            pool0,
                                            phi=phi,
                                            pump_frac=pump,
                                            close=close,
                                            design=design,
                                            n_pump_splits=n_ps,
                                            n_repay_splits=n_rs,
                                            n_unwind_splits=n_us,
                                            multi_ck=multi,
                                        )
                                        if r is None or r.n_actions > n_max:
                                            continue
                                        if not math.isfinite(r.profit):
                                            continue
                                        r.netuid = snap["netuid"]
                                        r.name = snap["name"]
                                        rows.append(r)

            for phi in phi_grid(caps["phi_max_long"]):
                for dump in dump_fracs:
                    for n_ds in range(1, K_SPLIT_MAX + 1):
                        for n_us in range(1, K_SPLIT_MAX + 1):
                            for close in closes:
                                if dump == 0.0 and n_ds > 1:
                                    continue
                                r = run_long_sequence(
                                    pool0,
                                    phi=phi,
                                    dump_frac=dump,
                                    close=close,
                                    design=design,
                                    n_dump_splits=n_ds,
                                    n_unwind_splits=n_us,
                                )
                                if r is None or r.n_actions > n_max:
                                    continue
                                if not math.isfinite(r.profit):
                                    continue
                                r.netuid = snap["netuid"]
                                r.name = snap["name"]
                                rows.append(r)

    def summarize(design: str, live_only: bool) -> dict:
        live_names = {s["name"] for s in data["subnets"]}
        subset = [
            r
            for r in rows
            if r.design == design
            and (r.name in live_names if live_only else True)
        ]
        if not subset:
            return {"n": 0}
        top = max(subset, key=lambda r: r.profit)
        profitable = [r for r in subset if r.profit > EPS_PROFIT]
        return {
            "n": len(subset),
            "max_profit": top.profit,
            "n_profitable": len(profitable),
            "pass": len(profitable) == 0,
            "at": {
                "netuid": top.netuid,
                "name": top.name,
                "side": top.side,
                "close": top.close,
                "phi": top.phi,
                "pump_or_dump": top.pump_frac,
                "n_pump_splits": top.n_pump_splits,
                "n_repay_splits": top.n_repay_splits,
                "n_unwind_splits": top.n_unwind_splits,
                "multi_ck": top.multi_ck,
                "n_actions": top.n_actions,
                "w2_after": top.w2_after,
                "L_ratio": top.L_ratio,
            },
            "profitable_examples": [
                {
                    "netuid": r.netuid,
                    "name": r.name,
                    "side": r.side,
                    "close": r.close,
                    "phi": r.phi,
                    "pump_or_dump": r.pump_frac,
                    "n_actions": r.n_actions,
                    "multi_ck": r.multi_ck,
                    "splits": [r.n_pump_splits, r.n_repay_splits, r.n_unwind_splits],
                    "profit": r.profit,
                    "w2_after": r.w2_after,
                }
                for r in sorted(profitable, key=lambda r: -r.profit)[:10]
            ],
        }

    live_a = summarize("a", live_only=True)
    live_buggy = summarize("buggy", live_only=True)
    all_a = summarize("a", live_only=False)

    out = {
        "criterion": {
            "N": n_max,
            "K_split_max": K_SPLIT_MAX,
            "eps_profit_tao": EPS_PROFIT,
            "fee": PHI_FEE,
            "statement": (
                "PASS iff no self-contained ≤N-action sequence (multi-coldkey "
                "and trade-split variants included) yields attacker net TAO "
                f"> {EPS_PROFIT} for any φ up to side-cap φ_max on live snapshots."
            ),
            "design_under_test": "a (keep-weights)",
        },
        "subnets": subnet_meta,
        "by_design_live": {"a": live_a, "buggy": live_buggy},
        "by_design_including_clone_fixture": {"a": all_a},
        "verdict": {
            "keep_weights_pass": bool(live_a.get("pass")),
            "buggy_finds_profit": live_buggy.get("n_profitable", 0) > 0,
            "max_profit_keep_weights": live_a.get("max_profit"),
            "max_profit_buggy": live_buggy.get("max_profit"),
            "n_cells_keep_weights": live_a.get("n"),
            "n_cells_buggy": live_buggy.get("n"),
        },
    }
    return out


def main() -> None:
    n_max = N_MAX
    if len(sys.argv) > 1:
        n_max = int(sys.argv[1])
    out = search(n_max=n_max)
    dest = HERE / "no_arbitrage_cycle_search.json"
    dest.write_text(json.dumps(out, indent=2) + "\n")
    print(json.dumps(out["verdict"], indent=2))
    print(f"wrote {dest}", flush=True)
    print("\n=== subnet φ_max ===")
    for s in out["subnets"]:
        if s.get("clone_fixture"):
            continue
        print(
            f"  SN{s['netuid']} {s['name']}: y={s['y']:.1f} thin={s['thin']} "
            f"φ_max_short={s['phi_max_short']:.4f} Cap_short={s['Cap_short']:.1f}"
        )
    print("\n=== verdict ===")
    v = out["verdict"]
    print(
        f"  keep-weights PASS={v['keep_weights_pass']} "
        f"maxΔ={v['max_profit_keep_weights']:.6f} "
        f"cells={v['n_cells_keep_weights']}"
    )
    print(
        f"  buggy profitable cells={out['by_design_live']['buggy'].get('n_profitable')} "
        f"maxΔ={v['max_profit_buggy']:.6f}"
    )
    if not v["keep_weights_pass"]:
        print("FAIL: keep-weights found profitable sequence", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
