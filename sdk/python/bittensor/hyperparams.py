"""Subnet hyperparameter semantics: units, docs, and value conversion.

On chain, most hyperparameters are stored as raw fixed-point integers whose
meaning is invisible in the raw value: ``kappa 32767`` is really 0.5 of
``u16::MAX``, ``bonds_moving_avg 900000`` is 0.9 of 1,000,000, ``min_burn
700000000`` is τ0.7 in rao. This module is the single place that knows each
parameter's unit, so the CLI can render raw values with their normalized
meaning and accept either form when setting.

Conversion rules (see :func:`to_raw`): a plain integer is always the raw
on-chain value; a value with a decimal point is the human form — a 0..1
fraction for normalized parameters, a TAO amount for rao parameters.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional, Union

from .settings import BLOCKTIME, RAO_PER_TAO, TAO_SYMBOL, U16_MAX

U64_MAX = 2**64 - 1
PER_MILLION = 1_000_000

# Unit kinds:
#   u16          fraction stored as 0..65535 (65535 = 1.0)
#   u64          fraction stored as 0..u64::MAX (u64::MAX = 1.0)
#   per_million  fraction stored as 0..1_000_000 (1_000_000 = 1.0)
#   rao          TAO amount in rao (1 TAO = 1e9 rao)
#   blocks       block count (12s blocks; annotated with wall-clock time)
#   epochs       tempo count
#   difficulty   raw u64 PoW difficulty (u64::MAX = effectively disabled)
#   int          plain integer
#   bool         flag
KINDS = ("u16", "u64", "per_million", "rao", "blocks", "epochs", "difficulty", "int", "bool")

# Denominator for each fraction kind (raw / denominator = normalized 0..1).
_FRACTION_DENOMINATOR = {"u16": U16_MAX, "u64": U64_MAX, "per_million": PER_MILLION}


@dataclass(frozen=True)
class Hyperparam:
    kind: str
    doc: str


HYPERPARAMS: dict[str, Hyperparam] = {
    "rho": Hyperparam(
        "int",
        "Temperature of the sigmoid that maps a validator's consensus alignment to "
        "trust; higher values make the trust curve steeper.",
    ),
    "kappa": Hyperparam(
        "u16",
        "Majority-stake threshold of Yuma consensus: the stake fraction at which a "
        "weight counts as consensus-supported. Stored as u16 (65535 = 1.0), so "
        "32767 means 0.5 — a simple stake majority.",
    ),
    "immunity_period": Hyperparam(
        "blocks",
        "Blocks a newly registered neuron is immune from being pruned as the "
        "lowest-scoring UID when the subnet is full.",
    ),
    "min_allowed_weights": Hyperparam(
        "int",
        "Minimum number of distinct weights a validator must submit in one set_weights call.",
    ),
    "max_weights_limit": Hyperparam(
        "u16",
        "Cap on any single normalized weight a validator can assign to one miner. "
        "Stored as u16 (65535 = 1.0 — no cap).",
    ),
    "tempo": Hyperparam(
        "blocks",
        "Blocks per epoch: how often the subnet runs consensus and distributes emissions.",
    ),
    "min_difficulty": Hyperparam(
        "difficulty",
        "Lower bound for the PoW registration difficulty controller. u64::MAX "
        "pins difficulty at maximum, effectively disabling PoW registration.",
    ),
    "max_difficulty": Hyperparam(
        "difficulty",
        "Upper bound for the PoW registration difficulty controller. u64::MAX "
        "leaves the difficulty unbounded above.",
    ),
    "difficulty": Hyperparam(
        "difficulty",
        "Current PoW registration difficulty. u64::MAX means PoW registration is "
        "effectively disabled (burned registration only).",
    ),
    "weights_version": Hyperparam(
        "int",
        "Minimum version key validators must send with set_weights; raising it "
        "forces validators to upgrade before their weights are accepted.",
    ),
    "weights_rate_limit": Hyperparam(
        "blocks",
        "Minimum blocks a validator must wait between weight submissions.",
    ),
    "adjustment_interval": Hyperparam(
        "blocks",
        "Blocks between adjustments of the registration difficulty and burn cost.",
    ),
    "activity_cutoff": Hyperparam(
        "blocks",
        "Blocks without setting weights after which a validator is considered "
        "inactive and excluded from consensus.",
    ),
    "registration_allowed": Hyperparam(
        "bool",
        "Whether new neuron registrations are currently accepted on this subnet.",
    ),
    "network_pow_registration_allowed": Hyperparam(
        "bool",
        "Whether proof-of-work registration is allowed (as opposed to burned registration only).",
    ),
    "target_regs_per_interval": Hyperparam(
        "int",
        "Registrations per adjustment interval the difficulty/burn controller steers toward.",
    ),
    "min_burn": Hyperparam(
        "rao",
        "Floor for the burned-registration cost, in rao (1 TAO = 1e9 rao).",
    ),
    "max_burn": Hyperparam(
        "rao",
        "Ceiling for the burned-registration cost, in rao (1 TAO = 1e9 rao).",
    ),
    "bonds_moving_avg": Hyperparam(
        "per_million",
        "Bonds EMA smoothing factor, stored over 1,000,000 (900000 = 0.9): higher "
        "retains more of a validator's past bonds each epoch.",
    ),
    "max_regs_per_block": Hyperparam(
        "int",
        "Maximum registrations accepted in a single block.",
    ),
    "serving_rate_limit": Hyperparam(
        "blocks",
        "Minimum blocks between axon serve calls for one neuron.",
    ),
    "max_validators": Hyperparam(
        "int",
        "Maximum validator permits: only the top-stake neurons up to this count may validate.",
    ),
    "adjustment_alpha": Hyperparam(
        "u64",
        "Smoothing factor for the difficulty/burn adjustment, stored as u64 "
        "(u64::MAX = 1.0): higher values adjust more slowly.",
    ),
    "commit_reveal_period": Hyperparam(
        "epochs",
        "Epochs (tempos) between committing a weights hash and revealing the actual weights.",
    ),
    "commit_reveal_weights_enabled": Hyperparam(
        "bool",
        "Whether validators must use commit-reveal for weights, hiding them from "
        "copiers until the reveal.",
    ),
    "alpha_high": Hyperparam(
        "u16",
        "Upper bound of the liquid-alpha bonds smoothing range. Stored as u16 (65535 = 1.0).",
    ),
    "alpha_low": Hyperparam(
        "u16",
        "Lower bound of the liquid-alpha bonds smoothing range. Stored as u16 (65535 = 1.0).",
    ),
    "liquid_alpha_enabled": Hyperparam(
        "bool",
        "Whether liquid alpha is on: the bonds EMA factor then varies per-weight "
        "between alpha_low and alpha_high instead of using bonds_moving_avg.",
    ),
    # Owner-settable parameters that the hyperparameters read does not list.
    "max_allowed_uids": Hyperparam(
        "int",
        "Maximum neuron slots (UIDs) on the subnet; registrations beyond this "
        "prune the lowest-scoring neuron.",
    ),
    "burn_increase_mult": Hyperparam(
        "int",
        "Multiplier applied to the burn cost after each registration within an adjustment window.",
    ),
    "burn_half_life": Hyperparam(
        "blocks",
        "Blocks for the burn cost to decay halfway back toward min_burn.",
    ),
    "yuma3_enabled": Hyperparam(
        "bool",
        "Whether the Yuma3 consensus variant is enabled for this subnet.",
    ),
    "bonds_reset_enabled": Hyperparam(
        "bool",
        "Whether validator bonds are reset on certain subnet events.",
    ),
    "transfers_enabled": Hyperparam(
        "bool",
        "Whether stake transfers between coldkeys are enabled on this subnet.",
    ),
    "owner_cut_enabled": Hyperparam(
        "bool",
        "Whether the subnet owner takes their emission cut.",
    ),
    "owner_cut_auto_lock_enabled": Hyperparam(
        "bool",
        "Whether the owner's emission cut is automatically locked.",
    ),
}


def kind_of(name: str) -> str:
    """The unit kind for a hyperparameter ('int' when unknown)."""
    meta = HYPERPARAMS.get(name)
    return meta.kind if meta else "int"


def doc_of(name: str) -> Optional[str]:
    meta = HYPERPARAMS.get(name)
    return meta.doc if meta else None


def normalized(name: str, raw: int) -> Optional[float]:
    """The 0..1 fraction a raw fixed-point value encodes, or None for other kinds."""
    denominator = _FRACTION_DENOMINATOR.get(kind_of(name))
    if denominator is None:
        return None
    return raw / denominator


def _duration(blocks: int) -> Optional[str]:
    """Rough wall-clock reading of a block count ('1h 12m'), or None when tiny."""
    seconds = int(blocks * BLOCKTIME)
    if seconds < 60:
        return None
    units = (("d", 86400), ("h", 3600), ("m", 60))
    parts = []
    for suffix, size in units:
        if seconds >= size:
            parts.append(f"{seconds // size}{suffix}")
            seconds %= size
        elif parts:
            break
    return " ".join(parts[:2])


def annotate(name: str, raw) -> Optional[str]:
    """Human reading of a raw value ('≈ 0.5', '= τ0.7', '≈ 1h 12m'), or None."""
    if isinstance(raw, bool) or not isinstance(raw, int):
        return None
    kind = kind_of(name)
    fraction = normalized(name, raw)
    if fraction is not None:
        return f"≈ {fraction:.4g}"
    if kind == "rao":
        return f"= {TAO_SYMBOL}{raw / RAO_PER_TAO:g}"
    if kind == "blocks":
        duration = _duration(raw)
        return f"≈ {duration}" if duration else None
    if kind == "difficulty" and raw == U64_MAX:
        return "= u64::MAX"
    return None


# Input shape accepted by `to_raw` for each kind, phrased for --help/hints.
def value_forms(name: str) -> str:
    kind = kind_of(name)
    denominator = _FRACTION_DENOMINATOR.get(kind)
    if denominator is not None:
        return f"a 0..1 fraction (e.g. 0.5) or the raw integer (0..{denominator})"
    if kind == "rao":
        return "a TAO amount with a decimal point (e.g. 0.7) or the raw rao integer"
    if kind == "bool":
        return "true/false (or 1/0)"
    if kind == "blocks":
        return "a block count (12s blocks)"
    if kind == "epochs":
        return "an epoch (tempo) count"
    return "an integer"


_TRUE = {"true", "yes", "y", "on", "1"}
_FALSE = {"false", "no", "n", "off", "0"}


def to_raw(name: str, value: Union[int, float, str, bool]) -> int:
    """Convert a user-supplied value into the raw on-chain integer.

    Integers pass through as the raw value. Floats (and strings containing a
    decimal point) are the human form: a 0..1 fraction for normalized kinds, a
    TAO amount for rao kinds. Booleans and true/false strings work for flags.
    """
    kind = kind_of(name)
    if isinstance(value, bool):
        value = int(value)
    if isinstance(value, str):
        text = value.strip().lower().removeprefix("+")
        if kind == "bool" and text in _TRUE | _FALSE:
            return int(text in _TRUE)
        try:
            value = float(text) if ("." in text or "e" in text) else int(text)
        except ValueError:
            raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}")
    if isinstance(value, int):
        if value < 0:
            raise ValueError(f"{name} cannot be negative")
        return value
    # A float is the human form.
    denominator = _FRACTION_DENOMINATOR.get(kind)
    if denominator is not None:
        if not 0.0 <= value <= 1.0:
            raise ValueError(
                f"{name} is a normalized fraction: pass 0..1 (e.g. 0.5), or the "
                f"raw integer 0..{denominator} without a decimal point"
            )
        return round(value * denominator)
    if kind == "rao":
        return round(value * RAO_PER_TAO)
    if value.is_integer():
        return int(value)
    raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}")
