"""v2.1 params. Shared rate curve: rate_bps = min(cap_bps, apr_bps * u^k) with u = debt / holdings.
Env override for sweeps: V21_RATE="k=4,cap=200000,apr=2000,floor=0" (cap and apr in bps per year)."""
from dataclasses import dataclass
import os as _os
from v201x.params import (RAO, BPD, BPY, Q64, FEE_NUM, FEE_DEN, BPS, ALPHA_SUPPLY_CAP,  # noqa: F401
                          MAX_LONG_POSITIONS, SMA_PAUSE_LAPSE_BLOCKS, LongParams)
from v201x.params import SynthParams as _SP, ModelSwitches as _MS

_R = dict(kv.split('=') for kv in filter(None, (x.strip() for x in _os.environ.get('V21_RATE', '').split(','))))


@dataclass(frozen=True)
class SynthParams(_SP):
    apr_bps: int = int(_R.get('apr', 2_000))          # P3 shared base APR (rate at u = 1)
    rate_k: int = int(_R.get('k', 4))                 # P5 curvature exponent k (1..8)
    rate_cap_bps: int = int(_R.get('cap', 200_000))   # P5 cap on the yearly rate (bps/yr)


@dataclass(frozen=True)
class ModelSwitches(_MS):
    # v2.1 pins Igor bundle C: no forced buy-back, C-exhaust default, cov_ref max(spot, SMA30), virtual long, gross L8
    delev_funds: str = 'none'
    c_exhaust_default: bool = True
    cov_ref: str = 'spot_sma'
    long_virtual: bool = True
    l8_gross: bool = True
    rate_floor: bool = bool(int(_R.get('floor', 0)))  # variant: rate = APR * max(1, u^k) (healthy pay flat APR)
    u_price_slow: bool = bool(int(_R.get('uprice', 0)))  # variant: u uses min(TWAP, SMA30) (shorts) / max(TWAP, SMA30) (longs)
    partial_close_derisk: bool = True                 # partial close allowed if coverage holds OR u does not rise


DEFAULT_SYNTH = SynthParams()
DEFAULT_LONG = LongParams()
DEFAULT_SW = ModelSwitches()
