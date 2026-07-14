"""Subnet-owner hyperparameters: the ``sudo set`` a subnet owner needs.

The AdminUtils pallet's ``sudo_set_*`` calls are mostly root-only, but a subset is
settable by the subnet owner for their own subnet. This exposes exactly that
owner-settable subset through one ``SetHyperparameter`` intent, keyed by a stable
name (the same names btcli uses). ``alpha_low`` and ``alpha_high`` share the
two-value ``sudo_set_alpha_values`` call: setting one reads the current pair from
chain and keeps the other side. Root-only params, the enum-valued
``recycle_or_burn``, and ``sn_owner_hotkey`` are left to the raw-call escape hatch.

``activity_cutoff`` is read-only here: the epoch derives the effective cutoff
from ``activity_cutoff_factor`` (per-mille of tempo), so the legacy
absolute-blocks setter is deliberately not exposed.

Read current values back with the ``subnet_hyperparameters`` read.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .._generated import calls
from .._generated.storage import SubtensorModule as st
from ..hyperparams import kind_of, to_raw
from .base import Intent
from .registry import register

# name -> (AdminUtils setter, value is boolean). Every setter takes (netuid, value),
# except the alpha pair below, which shares the two-value sudo_set_alpha_values.
OWNER_HYPERPARAMETERS: dict[str, tuple[str, bool]] = {
    "tempo": ("sudo_set_tempo", False),
    "immunity_period": ("sudo_set_immunity_period", False),
    "min_allowed_weights": ("sudo_set_min_allowed_weights", False),
    "weights_version": ("sudo_set_weights_version_key", False),
    "activity_cutoff_factor": ("sudo_set_activity_cutoff_factor", False),
    "min_burn": ("sudo_set_min_burn", False),
    "max_burn": ("sudo_set_max_burn", False),
    "bonds_moving_avg": ("sudo_set_bonds_moving_average", False),
    "bonds_penalty": ("sudo_set_bonds_penalty", False),
    "serving_rate_limit": ("sudo_set_serving_rate_limit", False),
    "commit_reveal_period": ("sudo_set_commit_reveal_weights_interval", False),
    "max_allowed_uids": ("sudo_set_max_allowed_uids", False),
    "burn_increase_mult": ("sudo_set_burn_increase_mult", False),
    "burn_half_life": ("sudo_set_burn_half_life", False),
    "adjustment_alpha": ("sudo_set_adjustment_alpha", False),
    "rho": ("sudo_set_rho", False),
    "max_difficulty": ("sudo_set_max_difficulty", False),
    "alpha_sigmoid_steepness": ("sudo_set_alpha_sigmoid_steepness", False),
    "min_childkey_take": ("sudo_set_min_childkey_take_per_subnet", False),
    "owner_immune_neuron_limit": ("sudo_set_owner_immune_neuron_limit", False),
    "alpha_low": ("sudo_set_alpha_values", False),
    "alpha_high": ("sudo_set_alpha_values", False),
    "commit_reveal_weights_enabled": ("sudo_set_commit_reveal_weights_enabled", True),
    "liquid_alpha_enabled": ("sudo_set_liquid_alpha_enabled", True),
    "network_pow_registration_allowed": ("sudo_set_network_pow_registration_allowed", True),
    "yuma3_enabled": ("sudo_set_yuma3_enabled", True),
    "bonds_reset_enabled": ("sudo_set_bonds_reset_enabled", True),
    "transfers_enabled": ("sudo_set_toggle_transfer", True),
    "owner_cut_enabled": ("sudo_set_owner_cut_enabled", True),
    "owner_cut_auto_lock_enabled": ("sudo_set_owner_cut_auto_lock_enabled", True),
}

# Names sharing the two-value alpha call: position of each in (alpha_low, alpha_high).
_ALPHA_PAIR: dict[str, int] = {"alpha_low": 0, "alpha_high": 1}

HYPERPARAMETER_NAME_HELP = (
    "Hyperparameter to set. One of: " + ", ".join(sorted(OWNER_HYPERPARAMETERS)) + "."
)

HYPERPARAMETER_VALUE_HELP = (
    "New value. Give the raw on-chain integer (within the parameter's codec bounds), "
    "or the human form as a float or a string with a decimal point (a 0..1 fraction for "
    "normalized parameters, a non-negative TAO amount for rao parameters). Boolean "
    "parameters take true/false or 0/1 only."
)


@register
@dataclass
class SetHyperparameter(Intent):
    """Set an owner-settable subnet hyperparameter (btcli ``sudo set``).

    Dispatches the matching AdminUtils ``sudo_set_*`` call for the named
    parameter (``alpha_low``/``alpha_high`` read the current pair from chain and
    set both via ``sudo_set_alpha_values``). The signer must be the subnet's
    owner coldkey; root-only parameters are not available here and must go
    through the raw-call escape hatch. Changes take effect on chain immediately
    and shape subnet economics and consensus (registration costs, weight
    rules, immunity, transfers), so
    verify the raw value before sending — ``value`` accepts either the raw
    on-chain integer or a human form that is converted for you. Some
    parameters are rate-limited by the chain, so a quick follow-up change can
    fail. Read current values back with the ``subnet_hyperparameters`` read.
    """

    op = "set_hyperparameter"
    signer = "coldkey"
    origin = "subnet_owner"
    wraps = tuple(
        dict.fromkeys(("AdminUtils", method) for method, _ in OWNER_HYPERPARAMETERS.values())
    )

    netuid: int = field(metadata={"help": "Subnet to configure; the signer must be its owner."})
    name: str = field(metadata={"help": HYPERPARAMETER_NAME_HELP})
    value: int | float | str = field(metadata={"help": HYPERPARAMETER_VALUE_HELP})

    def __post_init__(self):
        if self.name not in OWNER_HYPERPARAMETERS:
            raise ValueError(
                f"unknown or owner-unsettable hyperparameter {self.name!r}; "
                f"settable: {sorted(OWNER_HYPERPARAMETERS)}"
            )
        self.value = to_raw(self.name, self.value)

    async def build(self, substrate, wallet: Any):
        method, is_bool = OWNER_HYPERPARAMETERS[self.name]
        if self.name in _ALPHA_PAIR:
            # The chain sets alpha_low/alpha_high together; keep the other side.
            pair = list(await substrate.query(*st.AlphaValues, [self.netuid]))
            pair[_ALPHA_PAIR[self.name]] = int(self.value)
            call = calls.AdminUtils.sudo_set_alpha_values(self.netuid, *pair)
            return await substrate.compose(call)
        value: Any = bool(self.value) if is_bool else int(self.value)
        if kind_of(self.name) == "fixed128":
            # Fixed-point newtypes (U64F64) encode as a one-field struct.
            value = {"bits": value}
        return await substrate.compose(getattr(calls.AdminUtils, method)(self.netuid, value))

    def summary(self) -> str:
        return f"set hyperparameter {self.name}={self.value} on netuid {self.netuid}"
