"""Declarative typed reads — the read-side analogue of intents.

Each read is a small async function registered by a stable name, written
against a *view* (the client, or a block-pinned snapshot — see ``base``). It
is built on the generic accessors (``view.query`` / ``view.runtime``) over the
generated descriptors, and applies the one bit metadata can't express: units
and domain typing. Reads are dispatchable by name (``client.read``),
block-pinnable (``(await client.at(block)).read``), and catalog-able for
agents (``client.reads``), exactly like intents are for writes.

Importing this package registers every read. Modules mirror the intents
layout: one file per domain, each a few-line addition away from a new read
that inherits dispatch + catalog + the generated CLI ``query`` command.
"""

from . import (  # noqa: F401  (imported for registration side effects)
    accounts,
    chain,
    collateral,
    delegation,
    epochs,
    hyperparameters,
    identity,
    leasing,
    locks,
    neurons,
    prices,
    staking,
    subnets,
    weights,
)
from .accounts import balance, balances, existential_deposit
from .base import REGISTRY, Grouped, Matrix, ReadSpec, dispatch, list_reads, read
from .delegation import DelegatedStake, DelegateInfo
from .identity import Commitment
from .neurons import Neuron
from .prices import SwapQuote
from .staking import StakePosition, StakeValuation, stake, stake_for_coldkey
from .subnets import SubnetInfo, token_symbols

__all__ = [
    "REGISTRY",
    "Commitment",
    "DelegateInfo",
    "DelegatedStake",
    "Grouped",
    "Matrix",
    "Neuron",
    "ReadSpec",
    "StakePosition",
    "StakeValuation",
    "SubnetInfo",
    "SwapQuote",
    "balance",
    "balances",
    "dispatch",
    "existential_deposit",
    "list_reads",
    "read",
    "stake",
    "stake_for_coldkey",
    "token_symbols",
]
