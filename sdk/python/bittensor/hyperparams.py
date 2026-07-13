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

from ._generated import storage as st
from .settings import BLOCKTIME, RAO_PER_TAO, TAO_SYMBOL, U16_MAX

U64_MAX = 2**64 - 1
PER_MILLION = 1_000_000

# Denominator per sp_arithmetic ratio type identity (a raw value equal to the
# denominator encodes exactly 1.0). ``PerU16`` is what post-newtype runtimes
# use for take/consensus-style ratios; it encodes identically to a bare u16
# over 65535. This dict is the one place in the SDK that knows these
# denominators — pre-newtype call sites pass "PerU16" explicitly instead of
# dividing by 65535 themselves.
RATIO_TYPE_DENOMINATORS: dict[str, int] = {
    "PerU16": U16_MAX,
    "Percent": 100,
    "Permill": 1_000_000,
    "Perbill": 1_000_000_000,
}


def ratio_fraction(type_ident: Optional[str], raw: int) -> Optional[float]:
    """The 0..1 fraction a ratio-typed raw int encodes, or None for non-ratio idents.

    ``type_ident`` is a type identity from the metadata IR / generated storage
    descriptors: "PerU16" -> raw/65535, "Percent" -> raw/100, "Permill" ->
    raw/1e6, "Perbill" -> raw/1e9. Anything else (bare primitives, balances,
    structural names, None) returns None so callers can fall back to their
    pre-newtype convention.
    """
    denominator = RATIO_TYPE_DENOMINATORS.get(type_ident or "")
    if denominator is None:
        return None
    return raw / denominator


# Unit kinds:
#   u16          fraction stored as 0..65535 (65535 = 1.0)
#   u64          fraction stored as 0..u64::MAX (u64::MAX = 1.0)
#   per_million  fraction stored as 0..1_000_000 (1_000_000 = 1.0)
#   rao          TAO amount in rao (1 TAO = 1e9 rao)
#   blocks       block count (12s blocks; annotated with wall-clock time)
#   epochs       tempo count
#   difficulty   raw u64 PoW difficulty (u64::MAX = effectively disabled)
#   fixed128     U64F64 fixed-point multiplier (bits / 2^64 = the real value)
#   int          plain integer
#   bool         flag
KINDS = (
    "u16",
    "u64",
    "per_million",
    "rao",
    "blocks",
    "epochs",
    "difficulty",
    "fixed128",
    "int",
    "bool",
)

# Scale of the U64F64 fixed-point kind (bits value / 2^64 = the real number).
FIXED128_ONE = 2**64

# Denominator for each fraction kind (raw / denominator = normalized 0..1).
_FRACTION_DENOMINATOR = {"u16": U16_MAX, "u64": U64_MAX, "per_million": PER_MILLION}


# Ceiling for `Hyperparam.short`, so the listing's description column stays
# one glanceable line (enforced by `codegen.check --units`).
SHORT_MAX = 38


@dataclass(frozen=True)
class Hyperparam:
    kind: str
    doc: str
    short: str = ""


HYPERPARAMS: dict[str, Hyperparam] = {
    "rho": Hyperparam(
        "int",
        "Temperature of the sigmoid that maps a validator's consensus alignment to "
        "trust; higher values make the trust curve steeper.",
        short="trust curve steepness",
    ),
    "kappa": Hyperparam(
        "u16",
        "Majority-stake threshold of Yuma consensus: the stake fraction at which a "
        "weight counts as consensus-supported. Stored as u16 (65535 = 1.0), so "
        "32767 means 0.5 — a simple stake majority.",
        short="consensus majority-stake threshold",
    ),
    "immunity_period": Hyperparam(
        "blocks",
        "Blocks a newly registered neuron is immune from being pruned as the "
        "lowest-scoring UID when the subnet is full.",
        short="prune-immunity window for new neurons",
    ),
    "min_allowed_weights": Hyperparam(
        "int",
        "Minimum number of distinct weights a validator must submit in one set_weights call.",
        short="minimum weights per submission",
    ),
    "max_weights_limit": Hyperparam(
        "u16",
        "Cap on any single normalized weight a validator can assign to one miner. "
        "Stored as u16 (65535 = 1.0 — no cap).",
        short="cap on a single miner's weight",
    ),
    "tempo": Hyperparam(
        "blocks",
        "Blocks per epoch: how often the subnet runs consensus and distributes emissions.",
        short="blocks per consensus epoch",
    ),
    "min_difficulty": Hyperparam(
        "difficulty",
        "Lower bound for the PoW registration difficulty controller. u64::MAX "
        "pins difficulty at maximum, effectively disabling PoW registration.",
        short="PoW registration difficulty floor",
    ),
    "max_difficulty": Hyperparam(
        "difficulty",
        "Upper bound for the PoW registration difficulty controller. u64::MAX "
        "leaves the difficulty unbounded above.",
        short="PoW registration difficulty ceiling",
    ),
    "difficulty": Hyperparam(
        "difficulty",
        "Current PoW registration difficulty. u64::MAX means PoW registration is "
        "effectively disabled (burned registration only).",
        short="current PoW registration difficulty",
    ),
    "weights_version": Hyperparam(
        "int",
        "Minimum version key validators must send with set_weights; raising it "
        "forces validators to upgrade before their weights are accepted.",
        short="minimum version key for set_weights",
    ),
    "weights_rate_limit": Hyperparam(
        "blocks",
        "Minimum blocks a validator must wait between weight submissions.",
        short="wait between weight submissions",
    ),
    "adjustment_interval": Hyperparam(
        "blocks",
        "Blocks between adjustments of the registration difficulty and burn cost.",
        short="difficulty/burn adjustment cadence",
    ),
    "activity_cutoff": Hyperparam(
        "blocks",
        "Blocks without setting weights after which a validator is considered "
        "inactive and excluded from consensus.",
        short="no-weights window before inactive",
    ),
    "registration_allowed": Hyperparam(
        "bool",
        "Whether new neuron registrations are currently accepted on this subnet.",
        short="new neuron registrations allowed",
    ),
    "network_pow_registration_allowed": Hyperparam(
        "bool",
        "Whether proof-of-work registration is allowed (as opposed to burned registration only).",
        short="PoW registration toggle",
    ),
    "target_regs_per_interval": Hyperparam(
        "int",
        "Registrations per adjustment interval the difficulty/burn controller steers toward.",
        short="registration-rate controller target",
    ),
    "min_burn": Hyperparam(
        "rao",
        "Floor for the burned-registration cost, in rao (1 TAO = 1e9 rao).",
        short="burned-registration cost floor",
    ),
    "max_burn": Hyperparam(
        "rao",
        "Ceiling for the burned-registration cost, in rao (1 TAO = 1e9 rao).",
        short="burned-registration cost ceiling",
    ),
    "bonds_moving_avg": Hyperparam(
        "per_million",
        "Bonds EMA smoothing factor, stored over 1,000,000 (900000 = 0.9): higher "
        "retains more of a validator's past bonds each epoch.",
        short="bonds EMA smoothing factor",
    ),
    "max_regs_per_block": Hyperparam(
        "int",
        "Maximum registrations accepted in a single block.",
        short="per-block registration cap",
    ),
    "serving_rate_limit": Hyperparam(
        "blocks",
        "Minimum blocks between axon serve calls for one neuron.",
        short="cooldown between axon serve calls",
    ),
    "max_validators": Hyperparam(
        "int",
        "Maximum validator permits: only the top-stake neurons up to this count may validate.",
        short="top-stake validator permit cap",
    ),
    "adjustment_alpha": Hyperparam(
        "u64",
        "Smoothing factor for the difficulty/burn adjustment, stored as u64 "
        "(u64::MAX = 1.0): higher values adjust more slowly.",
        short="difficulty/burn adjust smoothing",
    ),
    "commit_reveal_period": Hyperparam(
        "epochs",
        "Epochs (tempos) between committing a weights hash and revealing the actual weights.",
        short="weight commit-to-reveal delay",
    ),
    "commit_reveal_weights_enabled": Hyperparam(
        "bool",
        "Whether validators must use commit-reveal for weights, hiding them from "
        "copiers until the reveal.",
        short="commit-reveal weights toggle",
    ),
    "alpha_high": Hyperparam(
        "u16",
        "Upper bound of the liquid-alpha bonds smoothing range. Stored as u16 (65535 = 1.0).",
        short="liquid-alpha smoothing upper bound",
    ),
    "alpha_low": Hyperparam(
        "u16",
        "Lower bound of the liquid-alpha bonds smoothing range. Stored as u16 (65535 = 1.0).",
        short="liquid-alpha smoothing lower bound",
    ),
    "liquid_alpha_enabled": Hyperparam(
        "bool",
        "Whether liquid alpha is on: the bonds EMA factor then varies per-weight "
        "between alpha_low and alpha_high instead of using bonds_moving_avg.",
        short="per-weight bonds EMA (liquid alpha)",
    ),
    "bonds_penalty": Hyperparam(
        "u16",
        "Penalty applied to bonds for out-of-consensus weights. Stored as u16 "
        "(65535 = 1.0 — full penalty).",
        short="penalty on out-of-consensus bonds",
    ),
    "alpha_sigmoid_steepness": Hyperparam(
        "int",
        "Steepness of the liquid-alpha sigmoid mapping consensus alignment to the "
        "bonds EMA factor; negative values (root only) invert the curve.",
        short="liquid-alpha sigmoid steepness",
    ),
    "min_childkey_take": Hyperparam(
        "u16",
        "Minimum childkey take allowed on this subnet. Stored as u16 (65535 = 1.0).",
        short="floor for childkey take",
    ),
    "owner_immune_neuron_limit": Hyperparam(
        "int",
        "Number of subnet-owner-designated neurons that are immune from pruning.",
        short="owner-designated prune-immune UIDs",
    ),
    # Owner-settable parameters that the hyperparameters read does not list.
    "max_allowed_uids": Hyperparam(
        "int",
        "Maximum neuron slots (UIDs) on the subnet; registrations beyond this "
        "prune the lowest-scoring neuron.",
        short="neuron slot capacity before pruning",
    ),
    "burn_increase_mult": Hyperparam(
        "fixed128",
        "Multiplier applied to the burn cost after each registration within an "
        "adjustment window. U64F64 fixed-point: the raw bits divided by 2^64 "
        "give the real multiplier.",
        short="burn cost bump per registration",
    ),
    "burn_half_life": Hyperparam(
        "blocks",
        "Blocks for the burn cost to decay halfway back toward min_burn.",
        short="burn cost decay half-life",
    ),
    "yuma3_enabled": Hyperparam(
        "bool",
        "Whether the Yuma3 consensus variant is enabled for this subnet.",
        short="yuma3 consensus variant toggle",
    ),
    "bonds_reset_enabled": Hyperparam(
        "bool",
        "Whether validator bonds are reset on certain subnet events.",
        short="bonds reset on metadata commit",
    ),
    "transfers_enabled": Hyperparam(
        "bool",
        "Whether stake transfers between coldkeys are enabled on this subnet.",
        short="stake transfers between coldkeys",
    ),
    "owner_cut_enabled": Hyperparam(
        "bool",
        "Whether the subnet owner takes their emission cut.",
        short="owner emission cut toggle",
    ),
    "owner_cut_auto_lock_enabled": Hyperparam(
        "bool",
        "Whether the owner's emission cut is automatically locked.",
        short="auto-lock the owner's emission cut",
    ),
}


# Hyperparameter name -> the storage value that holds it. The generated
# descriptors carry each value's type identity (value_type_ident), so a
# post-newtype runtime's metadata can dictate the unit kind directly.
# Parameters without one dedicated storage value (alpha_high/alpha_low share
# the AlphaValues tuple) stay hand-tabled only.
STORAGE_ITEMS: dict[str, st.Item] = {
    "rho": st.SubtensorModule.Rho,
    "kappa": st.SubtensorModule.Kappa,
    "immunity_period": st.SubtensorModule.ImmunityPeriod,
    "min_allowed_weights": st.SubtensorModule.MinAllowedWeights,
    "max_weights_limit": st.SubtensorModule.MaxWeightsLimit,
    "tempo": st.SubtensorModule.Tempo,
    "min_difficulty": st.SubtensorModule.MinDifficulty,
    "max_difficulty": st.SubtensorModule.MaxDifficulty,
    "difficulty": st.SubtensorModule.Difficulty,
    "weights_version": st.SubtensorModule.WeightsVersionKey,
    "weights_rate_limit": st.SubtensorModule.WeightsSetRateLimit,
    "adjustment_interval": st.SubtensorModule.AdjustmentInterval,
    "activity_cutoff": st.SubtensorModule.ActivityCutoff,
    "registration_allowed": st.SubtensorModule.NetworkRegistrationAllowed,
    "network_pow_registration_allowed": st.SubtensorModule.NetworkPowRegistrationAllowed,
    "target_regs_per_interval": st.SubtensorModule.TargetRegistrationsPerInterval,
    "min_burn": st.SubtensorModule.MinBurn,
    "max_burn": st.SubtensorModule.MaxBurn,
    "bonds_moving_avg": st.SubtensorModule.BondsMovingAverage,
    "max_regs_per_block": st.SubtensorModule.MaxRegistrationsPerBlock,
    "serving_rate_limit": st.SubtensorModule.ServingRateLimit,
    "max_validators": st.SubtensorModule.MaxAllowedValidators,
    "adjustment_alpha": st.SubtensorModule.AdjustmentAlpha,
    "commit_reveal_period": st.SubtensorModule.RevealPeriodEpochs,
    "commit_reveal_weights_enabled": st.SubtensorModule.CommitRevealWeightsEnabled,
    "liquid_alpha_enabled": st.SubtensorModule.LiquidAlphaOn,
    "bonds_penalty": st.SubtensorModule.BondsPenalty,
    "alpha_sigmoid_steepness": st.SubtensorModule.AlphaSigmoidSteepness,
    "min_childkey_take": st.SubtensorModule.MinChildkeyTakePerSubnet,
    "owner_immune_neuron_limit": st.SubtensorModule.ImmuneOwnerUidsLimit,
    "max_allowed_uids": st.SubtensorModule.MaxAllowedUids,
    "burn_increase_mult": st.SubtensorModule.BurnIncreaseMult,
    "burn_half_life": st.SubtensorModule.BurnHalfLife,
    "yuma3_enabled": st.SubtensorModule.Yuma3On,
    "bonds_reset_enabled": st.SubtensorModule.BondsResetOn,
    "transfers_enabled": st.SubtensorModule.TransferToggle,
    "owner_cut_enabled": st.SubtensorModule.OwnerCutEnabled,
    "owner_cut_auto_lock_enabled": st.SubtensorModule.OwnerCutAutoLockEnabled,
}

# Unit-bearing type identities -> the unit kind they dictate. Ratio types are
# fractions over their denominator; TaoBalance is a rao amount. Everything the
# metadata cannot express (blocks, epochs, per_million-by-convention, bool,
# int) stays with the hand table.
_IDENT_KINDS: dict[str, str] = {
    "PerU16": "u16",
    "Permill": "per_million",
    "TaoBalance": "rao",
}


def metadata_kind(name: str) -> Optional[str]:
    """The unit kind a hyperparameter's storage value type identity dictates.

    None when the parameter has no dedicated storage value, or when the
    (possibly pre-newtype) metadata carries no unit-bearing identity for it —
    the hand table then decides.
    """
    item = STORAGE_ITEMS.get(name)
    if item is None:
        return None
    ident = getattr(item, "value_type_ident", "")
    return _IDENT_KINDS.get(ident) if ident else None


def kind_of(name: str) -> str:
    """The unit kind for a hyperparameter ('int' when unknown).

    Metadata-primary: when the parameter's storage descriptor carries a
    unit-bearing type identity ("PerU16", "TaoBalance"), that identity
    decides; the hand table covers everything else and pre-newtype metadata.
    """
    kind = metadata_kind(name)
    if kind is not None:
        return kind
    meta = HYPERPARAMS.get(name)
    return meta.kind if meta else "int"


def doc_of(name: str) -> Optional[str]:
    meta = HYPERPARAMS.get(name)
    return meta.doc if meta else None


def short_of(name: str) -> Optional[str]:
    """The one-line blurb shown next to a value in the listing, or None."""
    meta = HYPERPARAMS.get(name)
    return meta.short or None if meta else None


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
    if kind == "fixed128":
        return f"≈ {raw / FIXED128_ONE:.4g}"
    return None


# Input shape accepted by `to_raw` for each kind, phrased for --help/hints.
def value_forms(name: str) -> str:
    kind = kind_of(name)
    denominator = _FRACTION_DENOMINATOR.get(kind)
    if denominator is not None:
        return f"a 0..1 fraction (e.g. 0.5) or the raw integer (0..{denominator})"
    if kind == "rao":
        return "a TAO amount with a decimal point (e.g. 0.7) or the raw rao integer"
    if kind == "fixed128":
        return "a multiplier with a decimal point (e.g. 1.5) or the raw U64F64 bits"
    if kind == "bool":
        return "true/false (or 1/0)"
    if kind == "blocks":
        return "a block count (12s blocks)"
    if kind == "epochs":
        return "an epoch (tempo) count"
    return "an integer"


def proportion_to_raw(
    value: Union[int, float, str],
    denominator: int,
    label: str = "proportion",
) -> int:
    """Convert a normalized-proportion input to its raw fixed-point integer.

    The same convention as :func:`to_raw`'s fraction kinds, reusable for values
    that aren't hyperparameters (takes, child proportions, emission splits):
    an integer is the raw on-chain value (bounded by ``denominator``); a float
    or a string with a decimal point is the human 0..1 fraction.
    """
    if isinstance(value, bool):
        raise ValueError(f"{label} takes a 0..1 fraction or a raw integer, not a boolean")
    if isinstance(value, str):
        text = value.strip().lower().removeprefix("+")
        try:
            value = float(text) if ("." in text or "e" in text) else int(text)
        except ValueError:
            raise ValueError(
                f"{label} takes a 0..1 fraction (e.g. 0.18) or the raw integer "
                f"(0..{denominator}); got {value!r}"
            ) from None
    if isinstance(value, int):
        if not 0 <= value <= denominator:
            raise ValueError(
                f"a raw {label} must be within 0..{denominator} (= 1.0); got {value}. "
                "Pass a value with a decimal point for the 0..1 human form."
            )
        return value
    if not 0.0 <= value <= 1.0:
        raise ValueError(
            f"a fractional {label} must be within 0..1 (e.g. 0.18); got {value}. "
            f"Pass a plain integer for the raw 0..{denominator} form."
        )
    return round(value * denominator)


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
    denominator = _FRACTION_DENOMINATOR.get(kind)
    if isinstance(value, int):
        if value < 0:
            raise ValueError(f"{name} cannot be negative")
        # A raw fixed-point fraction can never exceed its denominator (= 1.0);
        # a larger integer is almost certainly a unit mistake.
        if denominator is not None and value > denominator:
            raise ValueError(
                f"{name} takes {value_forms(name)}; {value} exceeds the raw maximum "
                f"{denominator} (= 1.0)"
            )
        return value
    # A float is the human form.
    if denominator is not None:
        if not 0.0 <= value <= 1.0:
            raise ValueError(
                f"{name} is a normalized fraction: pass 0..1 (e.g. 0.5), or the "
                f"raw integer 0..{denominator} without a decimal point"
            )
        return round(value * denominator)
    if kind == "rao":
        return round(value * RAO_PER_TAO)
    if kind == "fixed128":
        return round(value * FIXED128_ONE)
    if value.is_integer():
        return int(value)
    raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}")
