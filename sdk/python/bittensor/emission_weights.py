"""Per-subnet emission weight vector: slot names, rules, parsing and renormalisation.

A subnet's alpha emission is divided by a five-slot vector
``[burn, recycle, owner, validator, miner]`` whose entries are ``u64`` parts of
``u64::MAX`` (so the slots sum to exactly one). The owner or root sets it with
``AdminUtils.sudo_set_subnet_emission_weights(netuid, uids, values)`` — a
*sparse* update: every ``uids[i]`` names a slot and ``values[i]`` its new share.
Omitted owner/validator/miner slots keep their current value; omitted burn and
recycle are treated as zero. The chain then checks the v1 rules:

* burn (0) and recycle (1) must be zero — reserved for a later runtime;
* ``0 <= owner <= 0.33``; lease-created subnets may not drop below the launch
  default (18% on mainnet);
* ``validator <= 0.50``, and it must stay *above* root's baseline share:
  every block the chain pays root ``min(V, r)`` of the subnet's emission off
  the top of the validator slot, where ``r`` is what root would earn there
  with the default validator share (``V0``, 41% on mainnet) and the global
  ``TaoWeight`` — the *baseline*, recomputed from live chain state (root
  TAO, the subnet's alpha issuance, the global ``TaoWeight``) on every block.
  Alpha validators get the rest, ``V - min(V, r)``. A set with ``V <= r`` is
  rejected because validators would get nothing; if the baseline later grows
  past ``V``, root's cut is capped at the whole slot until the owner raises
  ``V`` — see :func:`baseline_root_share` and :func:`validator_floor`;
* ``miner >= 0.33``;
* the five slots sum to exactly one.

Everything in this module is pure (no chain access) so the CLI can preview and
explain a change before submitting it; :func:`validate` mirrors the on-chain
checks and names the same error codes.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from fractions import Fraction
from typing import Iterable, Mapping, Optional, Union

U64_MAX = 2**64 - 1

#: Denominator of every emission weight (``u64::MAX``).
ONE = U64_MAX

#: Slot order on chain; the index of a name is its ``uid``.
SLOT_NAMES: tuple[str, ...] = ("burn", "recycle", "owner", "validator", "miner")
BURN, RECYCLE, OWNER, VALIDATOR, MINER = range(len(SLOT_NAMES))

#: Slots the v1 rules let an owner move (burn/recycle are pinned to zero).
ADJUSTABLE_SLOTS: tuple[int, ...] = (OWNER, VALIDATOR, MINER)


def fraction(numerator: int, denominator: int) -> int:
    """``numerator / denominator`` in parts of :data:`ONE` (floor, as on chain)."""
    return (ONE * numerator) // denominator


MAX_OWNER = fraction(33, 100)
MAX_VALIDATOR = fraction(50, 100)
MIN_MINER = fraction(33, 100)

#: Mainnet launch owner cut ``11796 / 65535`` scaled to u64 parts; also the
#: floor for lease-created subnets and the base from which the default vector
#: ``[0, 0, 0.18, 0.41, 0.41]`` is derived.
DEFAULT_OWNER = (11796 * ONE) // 65535

#: Validator share of the default vector (``V0``, ~41% on mainnet): half of what
#: is left after the default owner cut. Root's baseline share is anchored to
#: it: whatever a subnet sets, root is paid what it would earn at ``V0``.
DEFAULT_VALIDATOR = (ONE - DEFAULT_OWNER) // 2

#: How often a subnet owner may change the vector (blocks; ~7 days at 12 s).
#: Root is exempt and a subnet's first change is always allowed. Mirrors the
#: chain's ``SubnetEmissionWeightsRateLimit``.
OWNER_RATE_LIMIT_BLOCKS = 50_400
BLOCK_SECONDS = 12

ShareLike = Union[int, float, str]


def slot_index(slot: Union[int, str]) -> int:
    """Resolve a slot given by name (``"owner"``) or uid (``2``)."""
    if isinstance(slot, str):
        key = slot.strip().lower()
        if key.isdigit():
            return slot_index(int(key))
        if key not in SLOT_NAMES:
            raise ValueError(
                f"unknown emission slot {slot!r}; slots are "
                + ", ".join(f"{i} {name}" for i, name in enumerate(SLOT_NAMES))
            )
        return SLOT_NAMES.index(key)
    if not 0 <= slot < len(SLOT_NAMES):
        raise ValueError(
            f"emission slot {slot} is out of range; slots are "
            + ", ".join(f"{i} {name}" for i, name in enumerate(SLOT_NAMES))
        )
    return int(slot)


def parse_share(value: ShareLike) -> int:
    """Turn a human share into u64 parts.

    Accepts a 0..1 fraction (``0.41`` / ``"0.41"``), a percentage (``"41%"``),
    or a raw integer part count (``7560000000000000000``). Integers are taken
    as raw parts; anything with a decimal point or percent sign is a fraction.
    """
    if isinstance(value, bool):
        raise ValueError("emission share must be a number, not a boolean")
    if isinstance(value, int):
        parts = value
    elif isinstance(value, float):
        parts = _fraction_to_parts(_exact(repr(value), value), value)
    else:
        text = value.strip().replace("_", "")
        if not text:
            raise ValueError("emission share is empty")
        if text.endswith("%"):
            parts = _fraction_to_parts(_exact(text[:-1], value) / 100, value)
        elif "." in text or "e" in text.lower():
            parts = _fraction_to_parts(_exact(text, value), value)
        else:
            try:
                parts = int(text)
            except ValueError as error:
                raise ValueError(
                    f"cannot read emission share {value!r}; give a 0..1 fraction "
                    "(0.41), a percentage (41%), or raw u64 parts"
                ) from error
    if not 0 <= parts <= ONE:
        raise ValueError(f"emission share {value!r} is outside 0..1 (0..{ONE} parts)")
    return parts


def _exact(text: str, shown: object) -> Fraction:
    """Decimal text -> exact rational, so ``"50%"`` lands on ``fraction(50, 100)``."""
    try:
        return Fraction(Decimal(text))
    except (InvalidOperation, ValueError) as error:
        raise ValueError(
            f"cannot read emission share {shown!r}; give a 0..1 fraction "
            "(0.41), a percentage (41%), or raw u64 parts"
        ) from error


def _fraction_to_parts(fraction_value: Fraction, shown: object) -> int:
    """Exact rational -> u64 parts with the chain's floor convention."""
    if not 0 <= fraction_value <= 1:
        raise ValueError(f"emission share {shown!r} must be between 0 and 1 (0% and 100%)")
    return min(ONE, fraction(fraction_value.numerator, fraction_value.denominator))


def share_to_float(parts: int) -> float:
    """u64 parts -> 0..1 float."""
    return parts / ONE


def format_share(parts: int, places: int = 2) -> str:
    """u64 parts -> ``"41.00%"``."""
    return f"{share_to_float(parts) * 100:.{places}f}%"


@dataclass(frozen=True)
class Rules:
    """The per-subnet inputs the chain rules depend on.

    ``min_validator`` is the lowest validator share (parts) the chain accepts
    at this block: one part above root's baseline share, since the chain
    rejects ``V <= r`` (see :func:`validator_floor`); ``None`` when it is
    unknown (no chain state at hand). ``min_owner`` is the lease floor, zero
    for subnets not created through a lease.
    """

    min_validator: Optional[int] = None
    min_owner: int = 0
    created_via_lease: bool = False

    def limits(self) -> dict[str, dict[str, object]]:
        """Allowed range per slot, by slot name, as the machine-readable shape.

        Each entry carries ``min`` / ``max`` as 0..1 floats and ``min_parts`` /
        ``max_parts`` as exact u64 parts, plus ``why``. ``validator.min`` is the
        live floor (just above root's baseline share), ``None`` when unknown.
        """
        owner_min = self.min_owner if self.created_via_lease else 0
        return {
            "burn": _limit(0, 0, "reserved for a future runtime; must stay 0"),
            "recycle": _limit(0, 0, "reserved for a future runtime; must stay 0"),
            "owner": _limit(
                owner_min,
                MAX_OWNER,
                "lease-created subnet: contributors are paid from the owner cut"
                if self.created_via_lease
                else "0 turns the owner cut off",
            ),
            "validator": _limit(
                self.min_validator,
                MAX_VALIDATOR,
                "must exceed root's baseline share, which is paid off the top of this slot"
                if self.min_validator is not None
                else "must exceed root's baseline share (unknown without chain state)",
            ),
            "miner": _limit(MIN_MINER, ONE, "miner incentive floor"),
        }

    def table(self) -> list[tuple[str, str, str]]:
        """``(slot, allowed range, why)`` rows for human output, from :meth:`limits`."""
        rows: list[tuple[str, str, str]] = []
        for name, limit in self.limits().items():
            low, high = limit["min_parts"], limit["max_parts"]
            places = 4 if name == "validator" else 2
            if low == high:
                rule = f"= {format_share(int(high), 0)}"
            elif low is None:
                rule = f"<= {format_share(int(high), places)}"
            elif high == ONE:
                rule = f">= {format_share(int(low), places)}"
            else:
                rule = f"{format_share(int(low), places)} to {format_share(int(high), places)}"
            rows.append((name, rule, str(limit["why"])))
        rows.append(("sum", "= 100%", f"all five slots add up to exactly {ONE} parts"))
        return rows


def _limit(low: Optional[int], high: int, why: str) -> dict[str, object]:
    return {
        "min": None if low is None else share_to_float(low),
        "max": share_to_float(high),
        "min_parts": low,
        "max_parts": high,
        "why": why,
    }


@dataclass(frozen=True)
class Violation:
    """One broken rule: the chain error name and an explanation."""

    error: str
    message: str

    def __str__(self) -> str:
        return f"{self.error}: {self.message}"


def default_weights(owner: int = DEFAULT_OWNER) -> list[int]:
    """``[0, 0, owner, rest/2, rest - rest/2]`` — the chain's seed vector."""
    owner = min(owner, ONE)
    validator = (ONE - owner) // 2
    return [0, 0, owner, validator, ONE - owner - validator]


def normalize_weights(weights: Iterable[int]) -> list[int]:
    """Coerce a chain-shaped vector (any length <= 5, ints or Compact dicts) to five ints."""
    out = [int(w) for w in weights]
    if len(out) > len(SLOT_NAMES):
        raise ValueError(f"emission weight vector has {len(out)} slots; at most 5 are known")
    out.extend([0] * (len(SLOT_NAMES) - len(out)))
    return out


def merge(current: Iterable[int], updates: Mapping[Union[int, str], int]) -> list[int]:
    """Apply a sparse update exactly as the chain does: named slots are
    replaced, omitted owner/validator/miner keep their value, omitted
    burn/recycle become zero."""
    merged = normalize_weights(current)
    merged[BURN] = 0
    merged[RECYCLE] = 0
    for slot, value in updates.items():
        merged[slot_index(slot)] = int(value)
    return merged


def renormalize(
    current: Iterable[int],
    fixed: Mapping[Union[int, str], int],
) -> list[int]:
    """Pin the ``fixed`` slots and rescale the other adjustable slots so the
    vector sums to one again.

    The untouched owner/validator/miner slots keep their relative proportions
    (split evenly when they are all zero); the last of them in slot order —
    normally the miner — absorbs the rounding remainder. Raises if the pinned
    shares alone exceed one, or if every adjustable slot is pinned but the
    total is not one.
    """
    pinned = {slot_index(slot): int(value) for slot, value in fixed.items()}
    for slot in (BURN, RECYCLE):
        if pinned.get(slot, 0) != 0:
            raise ValueError(f"{SLOT_NAMES[slot]} (slot {slot}) must stay 0 in this runtime")
        pinned.setdefault(slot, 0)
    base = normalize_weights(current)
    free = [slot for slot in ADJUSTABLE_SLOTS if slot not in pinned]
    remaining = ONE - sum(pinned.values())
    if remaining < 0:
        raise ValueError(
            "the pinned shares alone exceed 100%: "
            + ", ".join(f"{SLOT_NAMES[s]}={format_share(v)}" for s, v in pinned.items() if v)
        )
    out = base[:]
    for slot, value in pinned.items():
        out[slot] = value
    if not free:
        if remaining != 0:
            raise ValueError(
                f"all adjustable slots are pinned but they sum to {format_share(ONE - remaining)}, "
                "not 100%"
            )
        return out
    free_total = sum(base[slot] for slot in free)
    if free_total == 0:
        shares = [remaining // len(free)] * len(free)
    else:
        shares = [(remaining * base[slot]) // free_total for slot in free]
    shares[-1] += remaining - sum(shares)
    for slot, value in zip(free, shares):
        out[slot] = value
    return out


def validate(weights: Iterable[int], rules: Optional[Rules] = None) -> list[Violation]:
    """Every rule the chain would reject, in the order it checks them (empty
    when the vector is accepted)."""
    rules = rules or Rules()
    w = normalize_weights(weights)
    found: list[Violation] = []
    if w[BURN] != 0:
        found.append(
            Violation(
                "BurnEmissionMustBeZero",
                f"burn (slot 0) is {format_share(w[BURN])}; burning emission is reserved for a "
                "future runtime, so it must be 0 (or omitted)",
            )
        )
    if w[RECYCLE] != 0:
        found.append(
            Violation(
                "RecycleEmissionMustBeZero",
                f"recycle (slot 1) is {format_share(w[RECYCLE])}; recycling emission is reserved "
                "for a future runtime, so it must be 0 (or omitted)",
            )
        )
    if w[OWNER] > MAX_OWNER:
        found.append(
            Violation(
                "OwnerEmissionAboveMaximum",
                f"owner (slot 2) is {format_share(w[OWNER])}; the owner cut may be at most "
                f"{format_share(MAX_OWNER)}",
            )
        )
    if rules.created_via_lease and w[OWNER] < rules.min_owner:
        found.append(
            Violation(
                "LeasedSubnetOwnerCutBelowMinimum",
                f"owner (slot 2) is {format_share(w[OWNER])}; this subnet was created through a "
                f"lease, so its owner cut may not drop below {format_share(rules.min_owner)} "
                "(lease contributors are paid from it)",
            )
        )
    if w[VALIDATOR] > MAX_VALIDATOR:
        found.append(
            Violation(
                "ValidatorEmissionAboveMaximum",
                f"validator (slot 3) is {format_share(w[VALIDATOR])}; validator dividends may "
                f"draw at most {format_share(MAX_VALIDATOR)}",
            )
        )
    if rules.min_validator is not None and w[VALIDATOR] < rules.min_validator:
        found.append(
            Violation(
                "ValidatorEmissionTooLowToPreserveRoot",
                f"validator (slot 3) is {format_share(w[VALIDATOR], 4)}; root is paid its "
                "baseline share of emission off the top of this slot every block, so right "
                f"now the validator share must be above {format_share(rules.min_validator - 1, 4)} "
                "or validators would get nothing",
            )
        )
    if w[MINER] < MIN_MINER:
        found.append(
            Violation(
                "MinerEmissionBelowMinimum",
                f"miner (slot 4) is {format_share(w[MINER])}; miner incentive must receive at "
                f"least {format_share(MIN_MINER)}",
            )
        )
    total = sum(w)
    if total != ONE:
        found.append(
            Violation(
                "EmissionWeightsDoNotSumToOne",
                f"the five slots sum to {format_share(total, 6)} ({total} parts), not exactly "
                f"100% ({ONE} parts); renormalise another slot in the same call "
                "(`btcli subnets emission set-value` does this for you)",
            )
        )
    return found


def root_emission_share(
    validator: float, tao_weight: float, root_tao: float, alpha_issuance: float
) -> float:
    """Root's share of the subnet's emission, ``V * x / (x + A)`` with
    ``x = root_tao * tao_weight``. With the default validator share and the
    global ``TaoWeight`` this is root's baseline, the shape the chain pays
    every block. Inputs are 0..1 floats for ``validator`` and ``tao_weight``
    and raw balances (any common unit) for ``root_tao`` and ``alpha_issuance``."""
    weighted = root_tao * tao_weight
    if weighted + alpha_issuance <= 0:
        return 0.0
    return validator * weighted / (weighted + alpha_issuance)


def baseline_root_share(global_tao_weight: float, root_tao: float, alpha_issuance: float) -> float:
    """Root's *baseline* share of a subnet's emission: what root earns there
    with the default validator share ``V0`` and the global ``TaoWeight``,
    ``V0 * x_g / (x_g + A)`` with ``x_g = root_tao * global_tao_weight``.
    The chain pays root exactly this (capped at the validator slot) off the
    top of the slot every block, recomputed from live state, so nothing the
    subnet sets can move it."""
    return root_emission_share(
        share_to_float(DEFAULT_VALIDATOR), global_tao_weight, root_tao, alpha_issuance
    )


def validator_floor(global_tao_weight: float, root_tao: float, alpha_issuance: float) -> float:
    """The validator share (0..1) a set must *exceed* right now: root's
    baseline share. The chain rejects ``V <= r`` because alpha validators
    would be left with nothing. Zero when root holds no TAO."""
    return baseline_root_share(global_tao_weight, root_tao, alpha_issuance)


def min_validator_parts(baseline_parts: int) -> int:
    """Lowest validator share in parts the chain accepts given root's baseline
    share in parts (as the runtime API reports it): one part above it."""
    return min(ONE, int(baseline_parts) + 1)


def root_share_paid(validator_parts: int, baseline_parts: int) -> float:
    """What root is paid of the subnet's emission (0..1) with the validator
    slot at ``validator_parts`` and root's baseline at ``baseline_parts``
    (both u64 parts, as the runtime API reports them): the baseline, capped
    at the slot."""
    return share_to_float(min(int(validator_parts), int(baseline_parts)))


def describe(weights: Iterable[int]) -> dict[str, str]:
    """``{slot name: "41.00%"}`` for display."""
    return {
        name: format_share(value) for name, value in zip(SLOT_NAMES, normalize_weights(weights))
    }


def shares(weights: Iterable[int]) -> dict[str, float]:
    """``{slot name: 0.41}`` — the vector as 0..1 floats keyed by slot name."""
    return {
        name: share_to_float(value) for name, value in zip(SLOT_NAMES, normalize_weights(weights))
    }


def preview(
    before: Iterable[int],
    after: Iterable[int],
    *,
    baseline_parts: int,
    global_tao_weight: float,
    root_tao: int,
    alpha_issuance: int,
) -> dict[str, object]:
    """The machine-readable preview of a vector change.

    ``before`` / ``after`` carry the full vectors in parts and as shares and
    ``changed`` lists the slots that move. ``root`` says what root TAO stakers
    earn on this subnet before and after (``min(V, r)`` with ``r`` the live
    baseline, given in u64 parts as the runtime API reports it), what alpha
    validators are left with (``validators_before`` / ``validators_after``),
    and whether root's cut is capped at the whole validator slot.
    ``share_after`` is ``None`` when ``V_new <= r``, which the chain rejects
    with ``ValidatorEmissionTooLowToPreserveRoot``. Comparisons are made in
    parts so the preview agrees with :func:`validate` to the last part.
    """
    old = normalize_weights(before)
    new = normalize_weights(after)
    baseline_parts = int(baseline_parts)
    baseline_share = share_to_float(baseline_parts)
    share_before = root_share_paid(old[VALIDATOR], baseline_parts)
    validators_before = share_to_float(old[VALIDATOR] - min(old[VALIDATOR], baseline_parts))
    rejected = new[VALIDATOR] <= baseline_parts
    share_after = None if rejected else root_share_paid(new[VALIDATOR], baseline_parts)
    validators_after = None if rejected else share_to_float(new[VALIDATOR] - baseline_parts)
    return {
        "before": {"weights": old, "shares": shares(old)},
        "after": {"weights": new, "shares": shares(new)},
        "changed": [SLOT_NAMES[slot] for slot in range(len(SLOT_NAMES)) if old[slot] != new[slot]],
        "root": {
            "share_before": share_before,
            "share_after": share_after,
            "baseline_share": baseline_share,
            "validators_before": validators_before,
            "validators_after": validators_after,
            "capped_before": baseline_parts > old[VALIDATOR],
            "capped_after": False if rejected else baseline_parts > new[VALIDATOR],
            "validator_floor": baseline_share,
            "global_tao_weight": global_tao_weight,
            "root_tao": root_tao,
            "alpha_issuance": alpha_issuance,
        },
    }


def rate_limit_status(
    last_change_block: int, current_block: int, limit: int = OWNER_RATE_LIMIT_BLOCKS
) -> dict[str, object]:
    """When the subnet owner may next change the vector.

    ``last_change_block`` is the chain's ``LastRateLimitedBlock`` entry for the
    subnet (0 when it has never been changed, which the chain treats as "first
    change is free"). Root is never limited.
    """
    next_block = current_block if last_change_block <= 0 else last_change_block + limit
    blocks_left = max(0, next_block - current_block)
    return {
        "limit_blocks": limit,
        "last_change_block": last_change_block,
        "next_allowed_block": next_block,
        "blocks_until_allowed": blocks_left,
        "owner_can_change_now": blocks_left == 0,
    }
