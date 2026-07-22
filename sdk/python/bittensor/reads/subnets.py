"""Subnet reads: existence, cost, identity, mechanisms, and the raw metagraph."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Optional

from .._generated import constants
from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from .base import read, utf8_text


@dataclass
class SubnetInfo:
    netuid: int
    tempo: int
    burn: Balance
    neuron_count: int


@read(
    "subnet",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def subnet(view, netuid: int) -> SubnetInfo:
    """Tempo, burn, and neuron count for one subnet (the three reads run concurrently)."""
    view = await view.at()
    tempo, burn_rao, count = await asyncio.gather(
        view.query(st.SubtensorModule.Tempo, [netuid]),
        view.query(st.SubtensorModule.Burn, [netuid]),
        view.query(st.SubtensorModule.SubnetworkN, [netuid]),
    )
    return SubnetInfo(
        netuid=netuid,
        tempo=int(tempo),
        burn=Balance.from_rao(int(burn_rao)),
        neuron_count=int(count or 0),
    )


@read("subnets", {}, category="Subnets")
async def subnets(view) -> list[SubnetInfo]:
    """Info for every subnet, fetched in four batched map queries rather than
    one-query-per-subnet. This is what listing should use.
    """
    view = await view.at()
    added, tempos, burns, counts = await asyncio.gather(
        view.query_map(st.SubtensorModule.NetworksAdded),
        view.query_map(st.SubtensorModule.Tempo),
        view.query_map(st.SubtensorModule.Burn),
        view.query_map(st.SubtensorModule.SubnetworkN),
    )
    tempo_map = {int(k): int(v) for k, v in tempos}
    burn_map = {int(k): int(v) for k, v in burns}
    count_map = {int(k): int(v or 0) for k, v in counts}
    netuids = sorted(int(k) for k, is_added in added if is_added)
    return [
        SubnetInfo(
            netuid=netuid,
            tempo=tempo_map.get(netuid, 0),
            burn=Balance.from_rao(burn_map.get(netuid, 0)),
            neuron_count=count_map.get(netuid, 0),
        )
        for netuid in netuids
    ]


@read(
    "burn",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def burn(view, netuid: int) -> Balance:
    """Current burn (recycle) cost to register on a subnet."""
    value = await view.query(st.SubtensorModule.Burn, [netuid])
    return Balance.from_rao(int(value))


@read(
    "commit_reveal_enabled",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def commit_reveal_enabled(view, netuid: int) -> bool:
    """Whether commit-reveal weights are enabled on a subnet."""
    value = await view.query(st.SubtensorModule.CommitRevealWeightsEnabled, [netuid])
    return bool(value)


# I32F32 has 32 fractional bits. The hyperparams RPC returns
# ``alpha_sigmoid_steepness`` as I32F32(saturating_from_num(i16)), but storage
# and ``sudo_set_alpha_sigmoid_steepness`` take the plain i16 — so peel the
# scale at the read boundary so display / copy-paste match the setter.
_I32F32_ONE = 2**32


def _hyperparam_value(tagged: dict) -> object:
    """Flatten one v3 ``{type_tag: payload}`` value to its raw payload.

    U64F64 decodes as ``{'bits': raw}``; those bits are what ``sudo set``
    writes for ``burn_increase_mult``. I32F32 is only used for
    ``alpha_sigmoid_steepness``, whose setter takes the integer part of the
    fixed-point value (bits / 2^32), not the raw bits.
    """
    ((tag, payload),) = tagged.items()
    if isinstance(payload, dict) and set(payload) == {"bits"}:
        bits = payload["bits"]
        if tag == "I32F32":
            return int(bits) // _I32F32_ONE
        return bits
    return payload


@read(
    "subnet_hyperparameters",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def subnet_hyperparameters(view, netuid: int) -> Optional[dict]:
    """All hyperparameters for a subnet, as a flat name -> raw value mapping.

    The set of names is version-dependent; None if the subnet does not exist.
    Uses the forward-compatible ``get_subnet_hyperparams_v3`` runtime API, so
    newly added chain hyperparameters (e.g. ``burn_half_life``) show up
    without a client update.
    """
    entries = await view.runtime(api.SubnetInfoRuntimeApi.get_subnet_hyperparams_v3, [netuid])
    if entries is None:
        return None
    return {entry["name"]: _hyperparam_value(entry["value"]) for entry in entries}


@read(
    "metagraph",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet whose metagraph to fetch."},
)
async def metagraph(view, netuid: int) -> dict:
    """The full metagraph for a subnet in one call (stakes, ranks, emissions, axons, ...).

    `name` and `symbol` are decoded to text (the wire carries them as
    compact-u16 vectors of utf-8 bytes, unlike every other text field).
    On runtimes with miner collateral (spec >= 435), each neuron also carries
    `collateral_locked`, `collateral_min`, and `collateral_earned` (alpha
    balances; ``None`` on older nodes).
    """
    graph = await view.runtime(api.SubnetInfoRuntimeApi.get_metagraph, [netuid])
    if isinstance(graph, dict):
        for key in ("name", "symbol"):
            if key in graph:
                graph[key] = utf8_text(graph[key])
    return graph


@read(
    "subnet_identity",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet whose identity to read."},
)
async def subnet_identity(view, netuid: int) -> Optional[dict]:
    """The identity metadata of a subnet, or None."""
    value = await view.query(st.SubtensorModule.SubnetIdentitiesV3, [netuid])
    return {k: utf8_text(v) for k, v in value.items()} if value else None


@read("subnet_names", {}, category="Subnets")
async def subnet_names(view) -> dict[int, str]:
    """Registered name of every subnet that published an identity, keyed by netuid."""
    rows = await view.query_map(st.SubtensorModule.SubnetIdentitiesV3)
    out: dict[int, str] = {}
    for netuid, identity in rows:
        name = utf8_text((identity or {}).get("subnet_name"))
        if name:
            out[int(netuid)] = str(name)
    return out


@read("token_symbols", {}, category="Subnets")
async def token_symbols(view) -> dict[int, str]:
    """Chain-registered token symbol of every subnet, keyed by netuid.

    Connecting runs this automatically (through a disk cache) and
    installs the result as the connection's display symbols, so balances
    decoded by that client render with each subnet's real symbol.
    """
    rows = await view.query_map(st.SubtensorModule.TokenSymbol)
    return {int(netuid): str(utf8_text(raw)) for netuid, raw in rows}


@read(
    "subnet_start_schedule",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def subnet_start_schedule(view, netuid: int) -> dict:
    """When a registered subnet can call ``start_call``.

    All values are block numbers: the subnet can start once the current block
    reaches `earliest_start_block` (registration block plus the chain's delay).
    """
    view = await view.at()
    registered_at = await view.query(st.SubtensorModule.NetworkRegisteredAt, [netuid])
    delay = int(await view.constant(constants.SubtensorModule.InitialStartCallDelay))
    registered_block = int(registered_at or 0)
    return {
        "netuid": netuid,
        "registered_at": registered_block,
        "start_call_delay": delay,
        "earliest_start_block": registered_block + delay,
        "current_block": view.block,
    }


@read(
    "mechanism_count",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def mechanism_count(view, netuid: int) -> int:
    """Current mechanism count configured for a subnet."""
    value = await view.query(st.SubtensorModule.MechanismCountCurrent, [netuid])
    return int(value or 0)


@read(
    "mechanism_emission_split",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet to query."},
)
async def mechanism_emission_split(view, netuid: int) -> list[int]:
    """Emission split between mechanisms on a subnet.

    Returns the raw on-chain per-mechanism share values, in mechanism order; an
    empty list means no explicit split is set.
    """
    value = await view.query(st.SubtensorModule.MechanismEmissionSplit, [netuid])
    return [int(x) for x in (value or [])]


@read("subnet_registration_cost", {}, category="Subnets")
async def subnet_registration_cost(view) -> Balance:
    """Current cost to register a new subnet.

    Denominated in TAO. The cost is dynamic: it rises when subnets register
    and decays back down over time.
    """
    rao = await view.runtime(api.SubnetRegistrationRuntimeApi.get_network_registration_cost, [])
    return Balance.from_rao(int(rao))
