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
    """Unit kind plus the codec/semantic bounds used by :func:`to_raw`.

    ``signed`` / ``bits`` override inference from the parameter's storage type
    identity (or the kind default). ``minimum`` / ``maximum`` tighten the raw
    integer range further when the chain enforces a semantic window inside the
    codec type (tempo, activity cutoff factor, burn multiplier, …).
    """

    kind: str
    doc: str
    short: str = ""
    signed: Optional[bool] = None
    bits: Optional[int] = None
    minimum: Optional[int] = None
    maximum: Optional[int] = None


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
        "Blocks per epoch: how often the subnet runs consensus and distributes "
        "emissions. Owner changes are bounded to 360-50,400 blocks, rate-limited "
        "to one per 360 blocks, and reset the epoch cycle.",
        short="blocks per consensus epoch",
        minimum=360,
        maximum=50_400,
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
        "inactive and excluded from consensus. Read-only: the epoch derives it "
        "as activity_cutoff_factor x tempo / 1000.",
        short="no-weights window before inactive",
    ),
    "activity_cutoff_factor": Hyperparam(
        "int",
        "Activity cutoff as per-mille of tempo (1000 = one tempo, bounds "
        "1,000-50,000): the effective cutoff is factor x tempo / 1000 blocks. "
        "Supersedes the absolute-blocks activity_cutoff.",
        short="activity cutoff, per-mille of tempo",
        minimum=1_000,
        maximum=50_000,
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
        signed=True,
        bits=16,
        minimum=0,  # owner path; root sets negatives via the raw-call escape hatch
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
        minimum=FIXED128_ONE,
        maximum=3 * FIXED128_ONE,
    ),
    "burn_half_life": Hyperparam(
        "blocks",
        "Blocks for the burn cost to decay halfway back toward min_burn.",
        short="burn cost decay half-life",
    ),
    "collateral_lock_share": Hyperparam(
        "u16",
        "Share of the registration price locked as miner collateral instead of "
        "burned. Stored as u16 (65535 = 1.0) and capped at 62258 (95%) so the "
        "burned share stays positive; 0 disables collateral.",
        short="registration price share locked",
        maximum=62258,
    ),
    "collateral_drain_ratio": Hyperparam(
        "fixed128",
        "Alpha of locked miner collateral released per alpha of incentive "
        "earned. U64F64 fixed-point, snapshot per miner at registration; must "
        "be positive and at most 10.",
        short="collateral released per α earned",
        minimum=1,
        maximum=10 * FIXED128_ONE,
    ),
    "yuma3_enabled": Hyperparam(
        "bool",
        "Whether the Yuma3 consensus variant is enabled for this subnet.",
        short="yuma3 consensus variant toggle",
    ),
    "yuma_version": Hyperparam(
        "int",
        "Consensus variant the epoch runs: 2 for classic Yuma, 3 when "
        "yuma3_enabled is set. Derived from that flag, not stored on chain.",
        short="epoch consensus variant (2 or 3)",
    ),
    "subnet_is_active": Hyperparam(
        "bool",
        "Whether the owner's one-shot start_call has fired: staking, alpha "
        "trading, and emissions are live. False for a registered-but-unstarted "
        "subnet.",
        short="subnet started (staking + emissions)",
    ),
    "subnet_emission_enabled": Hyperparam(
        "bool",
        "Root-controlled pool-side TAO emission switch. False means the subnet "
        "earns no TAO emission share — the pool-side alpha_in/tao_in/excess_tao "
        "chain-buy paths are zeroed — even when subnet_is_active is true. Only "
        "root (the chain sudo key) can flip it, via the "
        "set_subnet_emission_enabled intent.",
        short="root switch for TAO emission share",
    ),
    "user_liquidity_enabled": Hyperparam(
        "bool",
        "Legacy swap-v3 flag for user-provided liquidity positions; always "
        "false since the balancer migration deprecated all user LP calls.",
        short="legacy user-LP flag (always false)",
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
    "activity_cutoff_factor": st.SubtensorModule.ActivityCutoffFactorMilli,
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
    # TODO(codegen): switch to generated descriptors once the storage registry
    # is regenerated against spec >= 435.
    "collateral_lock_share": st.Item("SubtensorModule", "CollateralLockShare", "u16"),
    "collateral_drain_ratio": st.Item("SubtensorModule", "CollateralDrainRatio", "U64F64"),
    "yuma3_enabled": st.SubtensorModule.Yuma3On,
    "subnet_emission_enabled": st.SubtensorModule.SubnetEmissionEnabled,
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
        return "a non-negative TAO amount with a decimal point (e.g. 0.7) or the raw rao integer"
    if kind == "fixed128":
        lo, hi = raw_bounds(name)
        if lo == FIXED128_ONE and hi == 3 * FIXED128_ONE:
            return "a multiplier with a decimal point in 1..3 (e.g. 1.5) or the raw U64F64 bits"
        return "a non-negative multiplier with a decimal point (e.g. 1.5) or the raw U64F64 bits"
    if kind == "bool":
        return "true/false (or 1/0)"
    if kind == "blocks":
        lo, hi = raw_bounds(name)
        return f"a block count in {lo}..{hi} (12s blocks)"
    if kind == "epochs":
        return "an epoch (tempo) count"
    lo, hi = raw_bounds(name)
    return f"an integer in {lo}..{hi}"


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
        except OverflowError:
            raise ValueError(f"{label} value is out of range") from None
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
    try:
        return round(value * denominator)
    except OverflowError:
        raise ValueError(f"{label} value is out of range") from None


# Codec width inferred from the storage value's type identity when the hand
# table does not set `signed`/`bits` explicitly.
_IDENT_CODEC: dict[str, tuple[bool, int]] = {
    "u8": (False, 8),
    "u16": (False, 16),
    "u32": (False, 32),
    "u64": (False, 64),
    "u128": (False, 128),
    "i8": (True, 8),
    "i16": (True, 16),
    "i32": (True, 32),
    "i64": (True, 64),
    "bool": (False, 1),
    "PerU16": (False, 16),
    "Percent": (False, 8),
    "Permill": (False, 32),
    "Perbill": (False, 32),
    "TaoBalance": (False, 64),
    "FixedU128": (False, 128),
}

# Fallback when neither the hand table nor storage metadata supplies a codec.
_KIND_CODEC: dict[str, tuple[bool, int]] = {
    "u16": (False, 16),
    "u64": (False, 64),
    "per_million": (False, 64),
    "rao": (False, 64),
    "blocks": (False, 64),
    "epochs": (False, 64),
    "difficulty": (False, 64),
    "fixed128": (False, 128),
    "int": (False, 64),
    "bool": (False, 1),
}

_TRUE = frozenset({"true", "1"})
_FALSE = frozenset({"false", "0"})


def codec_of(name: str) -> tuple[bool, int]:
    """``(signed, bits)`` for the raw on-chain integer of ``name``.

    Precedence: explicit ``Hyperparam.signed``/``bits``, then the storage value
    type identity, then the kind default.
    """
    meta = HYPERPARAMS.get(name)
    if meta is not None and meta.bits is not None:
        signed = bool(meta.signed) if meta.signed is not None else False
        return signed, meta.bits
    item = STORAGE_ITEMS.get(name)
    if item is not None:
        ident = getattr(item, "value_type_ident", "") or ""
        if ident in _IDENT_CODEC:
            return _IDENT_CODEC[ident]
    kind = kind_of(name)
    return _KIND_CODEC.get(kind, (False, 64))


def raw_bounds(name: str) -> tuple[int, int]:
    """Inclusive ``(minimum, maximum)`` raw integer accepted by :func:`to_raw`."""
    signed, bits = codec_of(name)
    if bits <= 1:
        lo, hi = 0, 1
    elif signed:
        lo, hi = -(1 << (bits - 1)), (1 << (bits - 1)) - 1
    else:
        lo, hi = 0, (1 << bits) - 1
    meta = HYPERPARAMS.get(name)
    if meta is not None:
        if meta.minimum is not None:
            lo = max(lo, meta.minimum)
        if meta.maximum is not None:
            hi = min(hi, meta.maximum)
    denominator = _FRACTION_DENOMINATOR.get(kind_of(name))
    if denominator is not None:
        lo = max(lo, 0)
        hi = min(hi, denominator)
    return lo, hi


def _check_raw(name: str, raw: int) -> int:
    lo, hi = raw_bounds(name)
    if not lo <= raw <= hi:
        raise ValueError(f"{name} must be within {lo}..{hi} (raw on-chain range); got {raw}")
    return raw


def to_raw(name: str, value: Union[int, float, str, bool]) -> int:
    """Convert a user-supplied value into the raw on-chain integer.

    Integers pass through as the raw value (checked against the parameter's
    codec and semantic bounds). Floats (and strings containing a decimal point
    or exponent) are the human form: a 0..1 fraction for normalized kinds, a
    non-negative TAO amount for rao kinds, a non-negative multiplier for
    fixed128. Booleans accept only true/false or 0/1.
    """
    kind = kind_of(name)
    try:
        return _to_raw_unchecked(name, kind, value)
    except OverflowError:
        raise ValueError(f"{name} value is out of range") from None


def _to_raw_unchecked(name: str, kind: str, value: Union[int, float, str, bool]) -> int:
    if isinstance(value, bool):
        if kind != "bool":
            raise ValueError(f"{name} takes {value_forms(name)}; got a boolean")
        return int(value)
    if isinstance(value, str):
        text = value.strip().lower().removeprefix("+")
        if kind == "bool":
            if text in _TRUE:
                return 1
            if text in _FALSE:
                return 0
            raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}")
        try:
            value = float(text) if ("." in text or "e" in text) else int(text)
        except ValueError:
            raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}") from None
    if kind == "bool":
        if isinstance(value, int) and value in (0, 1):
            return value
        raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}")
    denominator = _FRACTION_DENOMINATOR.get(kind)
    if isinstance(value, int):
        return _check_raw(name, value)
    # A float is the human form.
    if denominator is not None:
        if not 0.0 <= value <= 1.0:
            raise ValueError(
                f"{name} is a normalized fraction: pass 0..1 (e.g. 0.5), or the "
                f"raw integer 0..{denominator} without a decimal point"
            )
        return _check_raw(name, round(value * denominator))
    if kind == "rao":
        if value < 0:
            raise ValueError(f"{name} cannot be negative")
        return _check_raw(name, round(value * RAO_PER_TAO))
    if kind == "fixed128":
        if value < 0:
            raise ValueError(f"{name} cannot be negative")
        return _check_raw(name, round(value * FIXED128_ONE))
    if value.is_integer():
        return _check_raw(name, int(value))
    raise ValueError(f"{name} takes {value_forms(name)}; got {value!r}")
