"""Lock and conviction reads, including client-side lock projections."""

from __future__ import annotations

import asyncio
import math
from typing import Any, Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from .base import read

# Chain default for UnlockRate/MaturityRate (blocks); used when the storage
# read comes back empty instead of carrying the ValueQuery default.
_DEFAULT_LOCK_RATE = 934_866

# The pallet's ONE_YEAR: a subnet's ownership can only change after this age.
_ONE_YEAR_BLOCKS = 7200 * 365 + 1800


async def _lock_record(view, coldkey_ss58: str, netuid: int, hotkey: str) -> Optional[dict]:
    """One lock rolled forward to now: the runtime API supplies the decayed
    ``LockState`` (``locked_mass``/``conviction``); the hotkey comes from the
    ``Lock`` storage key and perpetual mode from ``DecayingLock`` (present and
    false means the lock does not decay)."""
    state, decaying = await asyncio.gather(
        view.runtime(api.StakeInfoRuntimeApi.get_coldkey_lock, [coldkey_ss58, netuid]),
        view.query(st.SubtensorModule.DecayingLock, [coldkey_ss58, netuid]),
    )
    if not state:
        return None
    return {
        "hotkey": hotkey,
        "netuid": netuid,
        "locked_alpha": view.balance(int(state.get("locked_mass") or 0), netuid),
        "is_perpetual": decaying is not None and not bool(decaying),
    }


@read(
    "coldkey_lock",
    {"coldkey_ss58": "string", "netuid": "integer"},
    category="Locks & conviction",
    param_docs={
        "coldkey_ss58": "Coldkey whose lock to read.",
        "netuid": "Subnet to query.",
    },
)
async def coldkey_lock(view, coldkey_ss58: str, netuid: int) -> Optional[dict]:
    """Lock state for a coldkey on a subnet, or None if no lock exists.

    `locked_alpha` is denominated in the subnet's alpha and rolled forward
    (decayed) to the current block; `is_perpetual` locks do not decay.
    """
    rows = await view.query_map(st.SubtensorModule.Lock, [coldkey_ss58])
    hotkey = next((str(hk) for (nid, hk), _ in rows if int(nid) == netuid), None)
    if hotkey is None:
        return None
    return await _lock_record(view, coldkey_ss58, netuid, hotkey)


@read(
    "locks_for_coldkey",
    {"coldkey_ss58": "string"},
    category="Locks & conviction",
    param_docs={"coldkey_ss58": "Coldkey whose locks to list."},
)
async def locks_for_coldkey(view, coldkey_ss58: str) -> list[dict]:
    """Every lock a coldkey holds (one per subnet), rolled forward to now."""
    rows = await view.query_map(st.SubtensorModule.Lock, [coldkey_ss58])
    pairs = sorted((int(nid), str(hk)) for (nid, hk), _ in rows)
    records = await asyncio.gather(
        *[_lock_record(view, coldkey_ss58, nid, hk) for nid, hk in pairs]
    )
    return [r for r in records if r]


@read(
    "hotkey_conviction",
    {"hotkey_ss58": "string", "netuid": "integer"},
    category="Locks & conviction",
    param_docs={
        "hotkey_ss58": "Hotkey whose conviction to read.",
        "netuid": "Subnet to query.",
    },
)
async def hotkey_conviction(view, hotkey_ss58: str, netuid: int) -> dict:
    """Conviction metrics for a hotkey on a subnet.

    Returns `conviction_alpha` denominated in the subnet's alpha, rolled
    forward to the current block.
    """
    value = await view.runtime(api.StakeInfoRuntimeApi.get_hotkey_conviction, [hotkey_ss58, netuid])
    if isinstance(value, dict) and "bits" in value:
        # U64F64 fixed point in alpha rao; the integer part is the rao amount.
        return {"conviction_alpha": view.balance(int(value["bits"]) >> 64, netuid)}
    return value if isinstance(value, dict) else {"value": value}


@read(
    "most_convicted_hotkey",
    {"netuid": "integer"},
    category="Locks & conviction",
    param_docs={"netuid": "Subnet to query."},
)
async def most_convicted_hotkey(view, netuid: int) -> Optional[str]:
    """Hotkey with the highest conviction on a subnet, if any."""
    value = await view.runtime(
        api.StakeInfoRuntimeApi.get_most_convicted_hotkey_on_subnet, [netuid]
    )
    return str(value) if value else None


def _lock_state_parts(value: Any) -> tuple[int, float, int]:
    """Decode a raw ``LockState``: (locked_mass rao, conviction rao, last_update).

    Conviction is a U64F64 fixed-point number of alpha rao (``{'bits': ...}``).
    """
    if not isinstance(value, dict):
        return 0, 0.0, 0
    conviction = value.get("conviction") or 0
    if isinstance(conviction, dict):
        conviction = int(conviction.get("bits") or 0) / 2**64
    return (
        int(value.get("locked_mass") or 0),
        float(conviction),
        int(value.get("last_update") or 0),
    )


def _exp_decay(dt: float, tau: float) -> float:
    if dt <= 0:
        return 1.0
    if tau <= 0:
        return 0.0
    return math.exp(-min(dt / tau, 40.0))


def _project_lock(
    mass: float,
    conviction: float,
    dt: float,
    unlock_rate: float,
    maturity_rate: float,
    *,
    perpetual: bool,
    owner: bool,
) -> tuple[float, float]:
    """The (locked_mass, conviction) of one lock bucket ``dt`` blocks ahead.

    Float mirror of the pallet's ``roll_forward_lock``: locked mass decays at
    the unlock timescale (perpetual locks keep it), conviction matures toward
    the locked mass at the maturity timescale, and owner locks always count
    conviction equal to their locked mass.
    """
    unlock_decay = _exp_decay(dt, unlock_rate)
    maturity_decay = _exp_decay(dt, maturity_rate)
    new_mass = mass if perpetual else mass * unlock_decay
    if perpetual:
        from_mass = mass * (1.0 - maturity_decay)
    elif unlock_rate == maturity_rate:
        from_mass = mass * (dt / maturity_rate) * maturity_decay if maturity_rate else 0.0
    elif unlock_rate <= 0 or maturity_rate <= 0:
        from_mass = 0.0
    else:
        gamma = unlock_rate * (unlock_decay - maturity_decay) / (unlock_rate - maturity_rate)
        from_mass = mass * gamma if gamma > 0 else 0.0
    new_conviction = new_mass if owner else conviction * maturity_decay + from_mass
    return new_mass, new_conviction


# One rolled lock bucket: (locked_mass rao, conviction rao, perpetual, owner).
_LockBucket = tuple[float, float, bool, bool]


def _conviction_at(
    buckets: list[_LockBucket], dt: float, unlock_rate: float, maturity_rate: float
) -> float:
    return sum(
        _project_lock(m, c, dt, unlock_rate, maturity_rate, perpetual=p, owner=o)[1]
        for m, c, p, o in buckets
    )


def _blocks_until_conviction(
    buckets: list[_LockBucket],
    threshold: float,
    unlock_rate: float,
    maturity_rate: float,
) -> Optional[int]:
    """Blocks until the buckets' combined conviction first reaches ``threshold``,
    0 when already there, or None when it never gets there at current locks.

    Conviction is not monotonic (decaying locks mature and then fade), so this
    scans forward over ~10 timescales and bisects the first crossing.
    """
    if threshold <= 0:
        return None
    if _conviction_at(buckets, 0, unlock_rate, maturity_rate) >= threshold:
        return 0
    horizon = int(10 * max(unlock_rate, maturity_rate, 1.0))
    step = max(1, horizon // 4000)
    previous = 0
    for dt in range(step, horizon + 1, step):
        if _conviction_at(buckets, dt, unlock_rate, maturity_rate) >= threshold:
            low, high = previous, dt
            while high - low > 1:
                mid = (low + high) // 2
                if _conviction_at(buckets, mid, unlock_rate, maturity_rate) >= threshold:
                    high = mid
                else:
                    low = mid
            return high
        previous = dt
    return None


@read(
    "subnet_convictions",
    {"netuid": "integer"},
    category="Locks & conviction",
    param_docs={"netuid": "Subnet to query."},
)
async def subnet_convictions(view, netuid: int) -> dict:
    """Every hotkey with locked stake on a subnet, rolled forward to now.

    Per hotkey: locked mass, conviction, and the estimated blocks until its
    conviction reaches 10% of the subnet's outstanding alpha. That per-hotkey
    figure is a projection heuristic, not a takeover trigger: the ownership
    takeover in ``change_subnet_owner_if_needed`` requires the subnet to be
    at least ~1 year old (2,629,800 blocks) and the total aggregate
    conviction across all lockers to reach 10% of ``SubnetAlphaOut``, at
    which point the highest-conviction hotkey's coldkey becomes the subnet
    owner. Projections assume the lock rates and alpha out stay constant.
    """
    view = await view.at()
    (
        hotkey_rows,
        decaying_rows,
        owner_lock,
        decaying_owner_lock,
        owner_hotkey,
        alpha_out,
        unlock_rate,
        maturity_rate,
        registered_at,
    ) = await asyncio.gather(
        view.query_map(st.SubtensorModule.HotkeyLock, [netuid]),
        view.query_map(st.SubtensorModule.DecayingHotkeyLock, [netuid]),
        view.query(st.SubtensorModule.OwnerLock, [netuid]),
        view.query(st.SubtensorModule.DecayingOwnerLock, [netuid]),
        view.query(st.SubtensorModule.SubnetOwnerHotkey, [netuid]),
        view.query(st.SubtensorModule.SubnetAlphaOut, [netuid]),
        view.query(st.SubtensorModule.UnlockRate),
        view.query(st.SubtensorModule.MaturityRate),
        view.query(st.SubtensorModule.NetworkRegisteredAt, [netuid]),
    )
    now = view.block
    unlock_rate = int(unlock_rate) if unlock_rate else _DEFAULT_LOCK_RATE
    maturity_rate = int(maturity_rate) if maturity_rate else _DEFAULT_LOCK_RATE
    owner_hotkey = str(owner_hotkey) if owner_hotkey else None

    def _rolled_bucket(value: Any, *, perpetual: bool, owner: bool) -> _LockBucket:
        mass, conviction, last_update = _lock_state_parts(value)
        mass, conviction = _project_lock(
            mass,
            conviction,
            max(0, now - last_update),
            unlock_rate,
            maturity_rate,
            perpetual=perpetual,
            owner=owner,
        )
        return mass, conviction, perpetual, owner

    def _hotkey_of(key: Any) -> str:
        return str(key[0]) if isinstance(key, (tuple, list)) else str(key)

    buckets: dict[str, list[_LockBucket]] = {}
    for key, value in hotkey_rows:
        buckets.setdefault(_hotkey_of(key), []).append(
            _rolled_bucket(value, perpetual=True, owner=False)
        )
    for key, value in decaying_rows:
        buckets.setdefault(_hotkey_of(key), []).append(
            _rolled_bucket(value, perpetual=False, owner=False)
        )
    if owner_hotkey and owner_lock:
        buckets.setdefault(owner_hotkey, []).append(
            _rolled_bucket(owner_lock, perpetual=True, owner=True)
        )
    if owner_hotkey and decaying_owner_lock:
        buckets.setdefault(owner_hotkey, []).append(
            _rolled_bucket(decaying_owner_lock, perpetual=False, owner=True)
        )

    threshold = int(alpha_out or 0) / 10
    entries = []
    for hotkey, hotkey_buckets in buckets.items():
        locked = sum(mass for mass, _, _, _ in hotkey_buckets)
        conviction = sum(conviction for _, conviction, _, _ in hotkey_buckets)
        entries.append(
            {
                "hotkey": hotkey,
                "is_owner": hotkey == owner_hotkey,
                "locked_alpha": view.balance(int(locked), netuid),
                "conviction_alpha": view.balance(int(conviction), netuid),
                "pct_of_threshold": conviction / threshold if threshold else None,
                "blocks_to_threshold": _blocks_until_conviction(
                    hotkey_buckets, threshold, unlock_rate, maturity_rate
                ),
            }
        )
    entries.sort(key=lambda entry: -entry["conviction_alpha"].rao)

    all_buckets = [bucket for group in buckets.values() for bucket in group]
    total_locked = sum(mass for mass, _, _, _ in all_buckets)
    total_conviction = sum(conviction for _, conviction, _, _ in all_buckets)
    return {
        "netuid": netuid,
        "block": now,
        "alpha_out": view.balance(int(alpha_out or 0), netuid),
        "threshold_alpha": view.balance(int(threshold), netuid),
        "total_locked_alpha": view.balance(int(total_locked), netuid),
        "total_conviction_alpha": view.balance(int(total_conviction), netuid),
        "total_blocks_to_threshold": _blocks_until_conviction(
            all_buckets, threshold, unlock_rate, maturity_rate
        ),
        "unlock_rate": unlock_rate,
        "maturity_rate": maturity_rate,
        "owner_hotkey": owner_hotkey,
        "registered_at": int(registered_at or 0),
        "ownership_changeable_at_block": int(registered_at or 0) + _ONE_YEAR_BLOCKS,
        "hotkeys": entries,
    }
