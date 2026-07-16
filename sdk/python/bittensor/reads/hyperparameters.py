"""Per-subnet hyperparameter scalar reads (one storage value + cast)."""

from __future__ import annotations

from .._generated import storage as st
from .base import scalar_read


def _hyperparam(name: str, item, doc: str) -> None:
    scalar_read(name, item, per_netuid=True, doc=doc, category="Hyperparameters")


_hyperparam(
    "weights_rate_limit",
    st.SubtensorModule.WeightsSetRateLimit,
    "Blocks a hotkey must wait between weight sets on a subnet.",
)
_hyperparam(
    "difficulty",
    st.SubtensorModule.Difficulty,
    "Current PoW registration difficulty for a subnet.",
)
_hyperparam(
    "min_allowed_weights",
    st.SubtensorModule.MinAllowedWeights,
    "Minimum number of weights a validator must set on a subnet. A pure "
    "self-weight (a single entry pointing at the caller's own uid) bypasses "
    "the minimum, and the subnet owner bypasses validator-permit rules.",
)
_hyperparam(
    "max_weight_limit",
    st.SubtensorModule.MaxWeightsLimit,
    "Normalized-fraction cap on any single weight: after the chain "
    "normalizes the submitted vector, no weight may exceed "
    "max_weight_limit/65535 of the total. Not a raw u16 ceiling per weight.",
)
_hyperparam(
    "immunity_period",
    st.SubtensorModule.ImmunityPeriod,
    "Blocks a newly registered neuron is immune from deregistration.",
)
_hyperparam(
    "reveal_period",
    st.SubtensorModule.RevealPeriodEpochs,
    "Commit-reveal reveal window, in epochs, for a subnet.",
)
_hyperparam(
    "subnet_emission_enabled",
    st.SubtensorModule.SubnetEmissionEnabled,
    "Root-controlled pool-side TAO emission flag for a subnet (1 = enabled). "
    "When 0, the subnet earns no TAO emission share even while "
    "subnet_is_active is true; only root can flip it (see the "
    "set_subnet_emission_enabled intent).",
)
