"""Launch parameters of SPEC-v1.md v2.0 (sections 2 and 7.2), as integers.

Shared values (APR, pace, min notional, deposit, withdraw buffer, pause band, charge
cadence) live only on SynthParams / LendingParams. LongParams keeps the long-only
fields. v1.9 behaviour is reachable through ModelSwitches for ablations.
"""
from dataclasses import dataclass, field, replace
import os as _os

# v201x: v2.0.1 + switches for the fixes proposed in IGOR-REVIEW-v201.md. Defaults == v2.0.1.
# Env override for batch runs: V201X_FIX="delev_funds=P,c_exhaust_default=1,cov_ref=spot_sma"
def _parse_fix(s):
    out = {}
    for kv in filter(None, (x.strip() for x in s.split(','))):
        k, v = kv.split('=')
        out[k] = v
    return out
_FIX = _parse_fix(_os.environ.get('V201X_FIX', ''))
def _fb(k, d):
    return bool(int(_FIX[k])) if k in _FIX else d
def _fs(k, d):
    return _FIX.get(k, d)

RAO = 10**9                 # 1 TAO / 1 alpha in rao
BPD = 7200                  # blocks per day (12 s)
BPY = BPD * 365             # blocks per year used for APR accrual
Q64 = 1 << 64               # price fixed point: rao TAO per rao alpha, U64F64 bits
FEE_NUM, FEE_DEN = 33, 65535  # swap fee on input (subtensor swap pallet)
BPS = 10_000
ALPHA_SUPPLY_CAP = 21_000_000 * RAO   # P23 (v1.7, not settable)
MAX_LONG_POSITIONS = 256              # DeregParams.max_long_positions_per_subnet launch
SMA_PAUSE_LAPSE_BLOCKS = 216_000      # 30 days; used only if ModelSwitches.sma_pause_lapse


@dataclass(frozen=True)
class SynthParams:          # section 2 LendingParams + ShortParams (shared fields live here)
    stress_bps: int = 30_000            # P2 Stress = 3.0
    stress_slope_bps: int = 20_000      # P2 slope 2
    apr_bps: int = 2_000                # P3 flat 20 % (shared)
    breaker_on_bps: int = 200           # P21 legacy (off by default)
    breaker_off_bps: int = 100          # P21 legacy
    zone_bps: int = 12_500              # P16 zone: SMA30 <= 1.25 x lowest non-immune SMA30
    pause_bps: int = 12_500             # P17 TWAP term, shared (pause_twap_bps)
    buy_ceiling_sma_bps: int = 12_500   # P22 SMA30 term, shared (pause_sma_bps)
    twap_samples: int = 72              # P6
    twap_interval_blocks: int = 300     # P6 hourly
    pace_bps_per_block: int = 5         # P7 0.05 % (shared)
    open_impact_bps: int = 100          # P8 net minted per block <= 1 % of SubnetAlphaIn
    min_notional: int = 10 * RAO        # P10 (shared): shorts q * TWAP_median, longs D
    min_pool_tao: int = 1_000 * RAO     # P18 (shared)
    deposit: int = RAO // 10            # P19 (shared)
    withdraw_buffer_days: int = 7       # P20 (shared)
    charge_interval: int = 360          # P15 (shared)
    health_interest_days: int = 1       # P5: C must cover 1 day of interest
    rank_days: int = 30                 # P13 SMA30 of daily means
    exit_hysteresis_bps: int = 500      # v2.0.1 P5 exit margin (shared)
    # v2.0: sma_pause_max_blocks removed (no lapse). Ablation uses ModelSwitches + constant.


@dataclass(frozen=True)
class LongParams:           # section 7.5 LongParams (long-only; shared fields are on SynthParams)
    stress_bps: int = 20_000            # L2 m = 2
    stress_floor_bps: int = 15_000      # L2 floor
    lift_cap_bps: int = 1_000           # L5 10 % of SubnetTAO (pre-open)
    open_limit_bps: int = 100           # L8 net lifted per block <= 1 % SubnetTAO
    # Legacy paced phase-A length, used only if ModelSwitches.phase_a (removed from spec as L13).
    unwind_blocks: int = 7_200


@dataclass(frozen=True)
class ModelSwitches:
    """Places where the spec is silent, or ablations against v1.9 / proposed fixes.
    Defaults are v2.0."""
    delev_exit_on_health: bool = True    # G1: does a deleveraging position return to Open once healthy?
    charge_on_touch: bool = True         # G2: charge accrued interest at close/increase/withdraw
    long_pace_net_of_repay: bool = True  # G3: long down-budget measured after the repay leg
    r_uses_min_snapshot: bool = True     # r = min(T_flag, T_ema, T_A)/all_alpha
    zone_excluded_from_r: bool = False   # proposed fix F-r
    long_ref_includes_sma: bool = False  # proposed fix F-long-ref
    long_withdraw_ref_sma: bool = False
    stress_x_ema: bool = False           # proposed fix: Stress_eff uses EMA of SubnetAlphaIn
    t_avg_for_r: bool = False
    r_after_long_repay: bool = False     # review #5 fixed-point r (superseded by one-shot sale)
    prune_sells_longs: bool = True       # v1.9.1 default: one-shot pro-rata sale inside prune
    phase_a: bool = False                # v1.9.1: paced phase A removed; True = legacy for finding3
    rank_partial: bool = False           # review #4 alt (forbidden by I25 unless set for finding4)
    rank_cold_start: bool = False        # review #4 ema fallback (forbidden by I25 unless finding4)
    health_book: bool = True             # v1.9 book_share health
    p21_breaker: bool = False            # v1.6 P21 (removed in v1.7)
    supply_cap: bool = True              # v1.7 P23
    twap_pause_lapse_blocks: int = 0     # proposed fix (review #8): 0 = never
    p21_surcharge: bool = False          # proposed fix (review #1)
    # v2.0 ablations (defaults off = v2.0):
    legacy_ema_price_ref: bool = False   # True: short ref max(EMA, spot, SMA30); notional on EMA
    sma_pause_lapse: bool = False        # True: restore 30-day SMA-pause lapse and counter (v1.9)
    # ---- v201x fix switches (IGOR-REVIEW-v201.md); defaults reproduce v2.0.1 exactly
    delev_funds: str = _fs('delev_funds', 'PC')            # 'PC' v2.0.1 | 'P' buy-back spends P only | 'none' no forced buy-back
    c_exhaust_default: bool = _fb('c_exhaust_default', False)  # N4: delev short with C == 0 and interest due defaults (P -> SubnetTAO)
    cov_ref: str = _fs('cov_ref', 'max3')                  # 'max3' v2.0.1 | 'spot_sma' (2.4: drop TWAP median from coverage ref)
    long_open_curve: bool = _fb('long_open_curve', False)  # 2.2 L-a: price the lift-buy on the unlifted pool
    l8_gross: bool = _fb('l8_gross', False)
    long_virtual: bool = _fb('long_virtual', False)        # 2.2 fix: open is an on-curve buy; lifted TAO is an unbacked credit in SubnetTAO retired at repay                # 2.2: L8 counts lifts gross (closes don't free room)
    open_marginal: bool = _fb('open_marginal', False)
    dereg_pay_proceeds: bool = _fb('dereg_pay_proceeds', False)  # 2.7 option: at dereg a short pays max(normal, own remaining proceeds)      # 8.6: opener pays marginal BuyCost(Q+q)-BuyCost(Q), floored at pro rata


DEFAULT_SYNTH = SynthParams()
DEFAULT_LONG = LongParams()
DEFAULT_SW = ModelSwitches()
