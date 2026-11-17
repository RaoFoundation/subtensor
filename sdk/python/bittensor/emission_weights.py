"""Per-subnet emission weight vector: slot names, rules, parsing and renormalisation.

A subnet's alpha emission is divided by a five-slot vector
``[burn, recycle, owner, validator, miner]`` whose entries are ``u64`` parts of
``u64::MAX`` (so they sum to exactly one with the subnet's gamma shares). The
owner or root sets it with
``AdminUtils.sudo_set_subnet_emission_weights_v2(netuid, split)``, which
replaces the whole split, the five slots and the gamma shares (see
:func:`validate_gamma`). The chain checks:

* ``burn + recycle <= 0.33`` (combined cap);
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
* the five slots and the subnet's gamma shares sum to exactly one.

Burn destroys alpha but still counts toward issuance; recycle removes alpha
from ``SubnetAlphaOut`` and ``TotalAlphaIssuance``.

Everything in this module is pure (no chain access) so the CLI can preview and
explain a change before submitting it; :func:`validate` mirrors the on-chain
checks and names the same error codes.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Iterable, Mapping, Optional, Union

from ._policy import evaluate_policy

_POLICY_CONSTANTS = evaluate_policy("financial.constants", {})

U64_MAX = 2**64 - 1

#: Denominator of every emission weight (``u64::MAX``).
ONE = int(_POLICY_CONSTANTS["ONE"])

#: Slot order on chain; the index of a name is its ``uid``.
SLOT_NAMES: tuple[str, ...] = tuple(_POLICY_CONSTANTS["SLOT_NAMES"])
BURN, RECYCLE, OWNER, VALIDATOR, MINER = range(len(SLOT_NAMES))

#: Slots an owner may change (all five; burn+recycle share a combined cap).
ADJUSTABLE_SLOTS: tuple[int, ...] = (BURN, RECYCLE, OWNER, VALIDATOR, MINER)


def fraction(numerator: int, denominator: int) -> int:
    """``numerator / denominator`` in parts of :data:`ONE` (floor, as on chain)."""
    return int(
        evaluate_policy(
            "financial.emission.fraction", {"numerator": numerator, "denominator": denominator}
        )
    )


MAX_OWNER = int(_POLICY_CONSTANTS["MAX_OWNER"])
MAX_BURN_RECYCLE = int(_POLICY_CONSTANTS["MAX_BURN_RECYCLE"])
MAX_VALIDATOR = int(_POLICY_CONSTANTS["MAX_VALIDATOR"])
MIN_MINER = int(_POLICY_CONSTANTS["MIN_MINER"])
MAX_GAMMA = int(_POLICY_CONSTANTS["MAX_GAMMA"])
MAX_GAMMA_SHARES = int(_POLICY_CONSTANTS["MAX_GAMMA_SHARES"])

#: Most parts a split may miss one by and still count as rounding: shares
#: given as percentages or fractions floor to whole parts, so ones that add
#: up to 100% can fall a few parts short. See :func:`absorb_rounding`.
MAX_ROUNDING_PARTS = int(_POLICY_CONSTANTS["MAX_ROUNDING_PARTS"])

#: Mainnet launch owner cut ``11796 / 65535`` scaled to u64 parts; also the
#: floor for lease-created subnets and the base from which the default vector
#: ``[0, 0, 0.18, 0.41, 0.41]`` is derived.
DEFAULT_OWNER = int(_POLICY_CONSTANTS["DEFAULT_OWNER"])

#: Validator share of the default vector (``V0``, ~41% on mainnet): half of what
#: is left after the default owner cut. Root's baseline share is anchored to
#: it: whatever a subnet sets, root is paid what it would earn at ``V0``.
DEFAULT_VALIDATOR = int(_POLICY_CONSTANTS["DEFAULT_VALIDATOR"])

#: How often a subnet owner may change the vector (blocks; ~7 days at 12 s).
#: Root is exempt and a subnet's first change is always allowed. Mirrors the
#: chain's ``SubnetEmissionWeightsRateLimit``.
OWNER_RATE_LIMIT_BLOCKS = int(_POLICY_CONSTANTS["OWNER_RATE_LIMIT_BLOCKS"])
BLOCK_SECONDS = 12

ShareLike = Union[int, float, str]

#: Slot shares keyed by uid (``2``) or by name (``"owner"``).
SlotShares = Union[Mapping[int, int], Mapping[str, int]]


def slot_index(slot: Union[int, str]) -> int:
    """Resolve a slot given by name (``"owner"``) or uid (``2``)."""
    return evaluate_policy("financial.emission.slot", {"slot": slot})


def parse_share(value: ShareLike) -> int:
    """Turn a human share into u64 parts.

    Accepts a 0..1 fraction (``0.41`` / ``"0.41"``), a percentage (``"41%"``),
    or a raw integer part count (``7560000000000000000``). Integers are taken
    as raw parts; anything with a decimal point or percent sign is a fraction.
    """
    try:
        return int(
            evaluate_policy(
                "financial.emission.parse_share",
                {"value": value, "fraction_number": isinstance(value, float)},
            )
        )
    except (TypeError, ValueError) as error:
        raise ValueError(f"cannot read emission share {value!r}; {error}") from error


def share_to_float(parts: int) -> float:
    """u64 parts -> 0..1 float."""
    return evaluate_policy("financial.emission.float", {"parts": parts})


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
    for subnets not created through a lease. ``gamma_share`` is the sum of
    the subnet's gamma shares; the five slots sum to one less it.
    ``gamma_targets`` maps each subnet root whitelisted for gamma emission to
    whether gamma is enabled there, ``None`` when unknown; ``leased`` is whether the
    subnet is leased now.
    """

    min_validator: Optional[int] = None
    min_owner: int = 0
    created_via_lease: bool = False
    gamma_share: int = 0
    gamma_targets: Optional[Mapping[int, bool]] = None
    leased: bool = False

    def limits(self) -> dict[str, dict[str, object]]:
        """Allowed range per slot and per gamma share, by name, as the machine-readable shape.

        Each entry carries ``min`` / ``max`` as 0..1 floats and ``min_parts`` /
        ``max_parts`` as exact u64 parts, plus ``why``. ``validator.min`` is the
        live floor (just above root's baseline share), ``None`` when unknown.
        """
        limits = evaluate_policy("financial.emission.limits", {"rules": asdict(self)})
        owner_min = int(limits["owner"]["min_parts"])
        burn_recycle_why = (
            "burn + recycle together may be at most 33%; burn destroys alpha "
            "(still counts toward issuance), recycle removes it from issuance"
        )
        return {
            "burn": _limit(0, MAX_BURN_RECYCLE, burn_recycle_why),
            "recycle": _limit(0, MAX_BURN_RECYCLE, burn_recycle_why),
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
            "gamma": _limit(
                0,
                MAX_GAMMA,
                f"per gamma share; at most {MAX_GAMMA_SHARES} shares, each to another whitelisted "
                "subnet with gamma enabled; none on a leased subnet",
            ),
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
        rows.append(
            (
                "burn+recycle",
                f"<= {format_share(MAX_BURN_RECYCLE)}",
                "combined cap on slots 0 and 1",
            )
        )
        rows.append(
            (
                "sum",
                "= 100%",
                f"all five slots{and_gamma(self.gamma_share)} add up to exactly {ONE} parts",
            )
        )
        return rows


def and_gamma(gamma_share: int) -> str:
    """`` and the gamma shares (10.00%)`` when the subnet has any, for messages on the sum."""
    return f" and the gamma shares ({format_share(gamma_share)})" if gamma_share else ""


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
    return [int(value) for value in evaluate_policy("financial.emission.default", {"owner": owner})]


def normalize_weights(weights: Iterable[int]) -> list[int]:
    """Coerce a chain-shaped vector (any length <= 5, ints or Compact dicts) to five ints."""
    return [
        int(value)
        for value in evaluate_policy("financial.emission.normalize", {"weights": list(weights)})
    ]


def merge(current: Iterable[int], updates: SlotShares) -> list[int]:
    """Apply a sparse update exactly as the chain does: named slots are
    replaced; omitted slots keep their current value."""
    return [
        int(value)
        for value in evaluate_policy(
            "financial.emission.merge", {"weights": list(current), "updates": updates}
        )
    ]


def absorb_rounding(weights: Iterable[int], gamma_share: int = 0) -> list[int]:
    """The vector with its rounding absorbed: when the five slots and
    ``gamma_share`` miss one by at most :data:`MAX_ROUNDING_PARTS`, the miner
    slot makes up the difference. Any other vector comes back unchanged."""
    return [
        int(value)
        for value in evaluate_policy(
            "financial.emission.round", {"weights": list(weights), "gamma": gamma_share}
        )
    ]


def merge_gamma(current: Mapping[int, int], updates: Mapping[int, int]) -> dict[int, int]:
    """The gamma shares by target netuid after ``updates``: named targets are
    replaced, a share of 0 removes one, and omitted targets keep their share."""
    return {
        int(key): int(value)
        for key, value in evaluate_policy(
            "financial.emission.merge_gamma", {"current": current, "updates": updates}
        ).items()
    }


def renormalize(
    current: Iterable[int],
    fixed: SlotShares,
    gamma_share: int = 0,
) -> list[int]:
    """Pin the ``fixed`` slots and rescale the other adjustable slots so the
    vector and ``gamma_share``, the subnet's gamma shares, sum to one again.

    The untouched slots keep their relative proportions (split evenly when
    they are all zero); the last of them in slot order — normally the miner —
    absorbs the rounding remainder. Raises if the pinned shares and the gamma
    shares exceed one, or if every adjustable slot is pinned but the total is
    not one beyond the rounding the miner slot absorbs (:func:`absorb_rounding`).
    """
    return [
        int(value)
        for value in evaluate_policy(
            "financial.emission.renormalize",
            {"weights": list(current), "fixed": fixed, "gamma": gamma_share},
        )
    ]


def validate(weights: Iterable[int], rules: Optional[Rules] = None) -> list[Violation]:
    """Every rule the chain would reject, in the order it checks them (empty
    when the vector is accepted)."""
    rules = rules or Rules()
    w = normalize_weights(weights)
    found: list[Violation] = []
    errors = {
        row["error"]
        for row in evaluate_policy(
            "financial.emission.validate", {"weights": w, "rules": asdict(rules)}
        )
    }
    if "BurnRecycleEmissionAboveMaximum" in errors:
        found.append(
            Violation(
                "BurnRecycleEmissionAboveMaximum",
                f"burn (slot 0) is {format_share(w[BURN])} and recycle (slot 1) is "
                f"{format_share(w[RECYCLE])} ({format_share(w[BURN] + w[RECYCLE])} combined); "
                f"burn + recycle may be at most {format_share(MAX_BURN_RECYCLE)}",
            )
        )
    if "OwnerEmissionAboveMaximum" in errors:
        found.append(
            Violation(
                "OwnerEmissionAboveMaximum",
                f"owner (slot 2) is {format_share(w[OWNER])}; the owner cut may be at most "
                f"{format_share(MAX_OWNER)}",
            )
        )
    if "LeasedSubnetOwnerCutBelowMinimum" in errors:
        found.append(
            Violation(
                "LeasedSubnetOwnerCutBelowMinimum",
                f"owner (slot 2) is {format_share(w[OWNER])}; this subnet was created through a "
                f"lease, so its owner cut may not drop below {format_share(rules.min_owner)} "
                "(lease contributors are paid from it)",
            )
        )
    if "ValidatorEmissionAboveMaximum" in errors:
        found.append(
            Violation(
                "ValidatorEmissionAboveMaximum",
                f"validator (slot 3) is {format_share(w[VALIDATOR])}; validator dividends may "
                f"draw at most {format_share(MAX_VALIDATOR)}",
            )
        )
    if "ValidatorEmissionTooLowToPreserveRoot" in errors:
        found.append(
            Violation(
                "ValidatorEmissionTooLowToPreserveRoot",
                f"validator (slot 3) is {format_share(w[VALIDATOR], 4)}; root is paid its "
                "baseline share of emission off the top of this slot every block, so right "
                f"now the validator share must be above {format_share(rules.min_validator - 1, 4)} "
                "or validators would get nothing",
            )
        )
    if "MinerEmissionBelowMinimum" in errors:
        found.append(
            Violation(
                "MinerEmissionBelowMinimum",
                f"miner (slot 4) is {format_share(w[MINER])}; miner incentive must receive at "
                f"least {format_share(MIN_MINER)}",
            )
        )
    total = sum(w) + rules.gamma_share
    if "EmissionWeightsDoNotSumToOne" in errors:
        found.append(
            Violation(
                "EmissionWeightsDoNotSumToOne",
                f"the five slots{and_gamma(rules.gamma_share)} sum to {format_share(total, 6)} "
                f"({total} parts), not exactly 100% ({ONE} parts); renormalize another slot in "
                "the same call (`btcli subnets emission set-value` does this for you)",
            )
        )
    return found


def validate_gamma(
    netuid: int, shares: Mapping[int, int], rules: Optional[Rules] = None
) -> list[Violation]:
    """Every rule the chain would reject in ``shares``, the gamma shares of subnet
    ``netuid`` by target netuid (empty when they are accepted). The whitelist and
    gamma checks need ``rules.gamma_targets`` and are skipped without it."""
    rules = rules or Rules()
    found: list[Violation] = []
    errors = {
        (row["error"], row.get("target"))
        for row in evaluate_policy(
            "financial.emission.validate_gamma",
            {"netuid": netuid, "shares": shares, "rules": asdict(rules)},
        )
    }
    for target, share in sorted(shares.items()):
        if ("InvalidValue", target) in errors:
            found.append(
                Violation(
                    "InvalidValue",
                    f"gamma:{target} names this subnet; a gamma share must name another subnet",
                )
            )
        elif ("GammaEmissionTargetNotWhitelisted", target) in errors:
            found.append(
                Violation(
                    "GammaEmissionTargetNotWhitelisted",
                    f"subnet {target} is not whitelisted for gamma emission, so a split cannot "
                    f"name it; set gamma:{target} to 0 (`--gamma {target}=0` on `btcli subnets "
                    "emission set`) or ask root to whitelist it",
                )
            )
        elif ("GammaDisabled", target) in errors:
            found.append(
                Violation(
                    "GammaDisabled",
                    f"subnet {target} has gamma disabled, so a split cannot name it; set "
                    f"gamma:{target} to 0 (`--gamma {target}=0` on `btcli subnets emission set`) "
                    "or wait for its owner to enable it",
                )
            )
        if ("GammaEmissionAboveMaximum", target) in errors:
            found.append(
                Violation(
                    "GammaEmissionAboveMaximum",
                    f"gamma:{target} is {format_share(share)}; each gamma share may be at most "
                    f"{format_share(MAX_GAMMA)}",
                )
            )
    if ("TooManyGammaShares", None) in errors:
        found.append(
            Violation(
                "TooManyGammaShares",
                f"{len(shares)} gamma shares; a subnet may have at most {MAX_GAMMA_SHARES}",
            )
        )
    if ("GammaEmissionNotSettableOnLeasedSubnet", None) in errors:
        found.append(
            Violation(
                "GammaEmissionNotSettableOnLeasedSubnet",
                "this subnet is leased, so it cannot have gamma shares until the lease ends",
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
    return evaluate_policy(
        "financial.emission.root_share",
        {
            "validator": validator,
            "tao_weight": tao_weight,
            "root_tao": root_tao,
            "alpha_issuance": alpha_issuance,
        },
    )


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
    return int(evaluate_policy("financial.emission.min_validator", {"baseline": baseline_parts}))


def root_share_paid(validator_parts: int, baseline_parts: int) -> float:
    """What root is paid of the subnet's emission (0..1) with the validator
    slot at ``validator_parts`` and root's baseline at ``baseline_parts``
    (both u64 parts, as the runtime API reports them): the baseline, capped
    at the slot."""
    return evaluate_policy(
        "financial.emission.root_paid", {"validator": validator_parts, "baseline": baseline_parts}
    )


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
    result = evaluate_policy(
        "financial.emission.preview",
        {
            "before": list(before),
            "after": list(after),
            "baseline_parts": baseline_parts,
            "global_tao_weight": global_tao_weight,
            "root_tao": root_tao,
            "alpha_issuance": alpha_issuance,
        },
    )
    for side in ("before", "after"):
        result[side]["weights"] = [int(value) for value in result[side]["weights"]]
    for field in ("root_tao", "alpha_issuance"):
        result["root"][field] = int(result["root"][field])
    return result


def rate_limit_status(
    last_change_block: int, current_block: int, limit: int = OWNER_RATE_LIMIT_BLOCKS
) -> dict[str, object]:
    """When the subnet owner may next change the vector.

    ``last_change_block`` is the chain's ``LastRateLimitedBlock`` entry for the
    subnet (0 when it has never been changed, which the chain treats as "first
    change is free"). Root is never limited.
    """
    return evaluate_policy(
        "financial.emission.rate_limit",
        {"last": last_change_block, "current": current_block, "limit": limit},
    )
