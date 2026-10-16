"""Subnet reads: existence, cost, identity, mechanisms, and the raw metagraph."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Optional

from .._generated import constants
from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..emission_weights import (
    DEFAULT_OWNER,
    SLOT_NAMES,
    Rules,
    min_validator_parts,
    normalize_weights,
    rate_limit_status,
    share_to_float,
)
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


def _i64f64(value: object) -> float:
    """Decode an ``I64F64`` fixed-point value (``{'bits': raw}`` or raw int)."""
    if isinstance(value, dict):
        value = value.get("bits") or 0
    if value is None:
        return 0.0
    if not isinstance(value, (int, float, str)):
        raise TypeError("I64F64 requires numeric bits")
    return int(value or 0) / 2**64


@read("subnet_tao_flows", {}, category="Subnets")
async def subnet_tao_flows(view) -> dict[int, float]:
    """Per-subnet EMA of net TAO flow, in rao per block.

    TAO flow is the chain's running measure of user stake activity on a
    subnet: every stake adds to the accumulator and every unstake subtracts,
    so a positive value means TAO is (on average) flowing into the subnet
    and a negative value means it is flowing out. The chain smooths the
    per-block accumulator (``SubnetTaoFlow``) into an exponential moving
    average (``SubnetEmaTaoFlow``); this read returns that EMA per netuid.
    Subnets with no recorded flow are omitted.
    """
    rows = await view.query_map(st.SubtensorModule.SubnetEmaTaoFlow)
    flows: dict[int, float] = {}
    for key, value in rows:
        netuid = int(key[0]) if isinstance(key, (tuple, list)) else int(key)
        # Value is `(last_update_block, I64F64 ema)`.
        if isinstance(value, (tuple, list)) and len(value) == 2:
            flows[netuid] = _i64f64(value[1])
    return flows


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


def emission_weights_rate_limit_key(netuid: int) -> dict:
    """``LastRateLimitedBlock`` key for the owner's emission-weights change window."""
    return {"OwnerHyperparamUpdate": [netuid, "EmissionWeights"]}


async def root_preservation_inputs(query, netuid: int) -> tuple[int, int, float]:
    """``(root_tao, alpha_issuance, global_tao_weight)``: the live values root's
    baseline share of a subnet's emission is computed from every block — root
    TAO and alpha issuance in rao, the global ``TaoWeight`` as a 0..1 float.
    ``query(item, keys)`` is an async storage read."""
    root_tao, alpha_in, alpha_out, reservoir, tao_weight = await asyncio.gather(
        query(st.SubtensorModule.SubnetTAO, [0]),
        query(st.SubtensorModule.SubnetAlphaIn, [netuid]),
        query(st.SubtensorModule.SubnetAlphaOut, [netuid]),
        query(st.Swap.BalancerAlphaReservoir, [netuid]),
        query(st.SubtensorModule.TaoWeight, []),
    )
    issuance = int(alpha_in or 0) + int(alpha_out or 0) + int(reservoir or 0)
    return int(root_tao or 0), issuance, share_to_float(int(tao_weight or 0))


@dataclass
class SubnetEmissionWeights:
    """A subnet's emission weight vector and the rule inputs that bound it.

    Shares are u64 parts of ``u64::MAX``; ``shares`` carries the same values as
    0..1 floats keyed by slot name. ``baseline_root_share`` is root's baseline
    share of this subnet's emission at ``block`` (what root earns with the
    default validator share and the global ``TaoWeight``, from ``root_tao``,
    ``alpha_issuance`` in rao and ``global_tao_weight``); the chain pays root
    that off the top of the validator slot every block, capped at the slot, so
    ``root_share = min(validator, baseline_root_share)`` and alpha validators
    get ``validator_share_after_root``. ``root_capped`` is True when the
    baseline has grown past the validator slot and validators get nothing.
    ``baseline_root_parts`` is the baseline in exact u64 parts (what the
    runtime API reports); ``min_validator`` is the lowest validator share
    (parts) the chain accepts at this block: one part above it.
    ``last_change_block`` is when
    the owner last changed the vector (0 = never); :meth:`rate_limit` turns it
    into when the owner may change it next.
    """

    netuid: int
    weights: list[int]
    shares: dict[str, float]
    burn: int
    recycle: int
    owner: int
    validator: int
    miner: int
    global_tao_weight: float
    created_via_lease: bool
    root_tao: int
    alpha_issuance: int
    root_share: float
    baseline_root_share: float
    baseline_root_parts: int
    validator_share_after_root: float
    root_capped: bool
    min_validator: int
    block: int
    last_change_block: int

    def rules(self) -> Rules:
        return Rules(
            min_validator=self.min_validator,
            min_owner=DEFAULT_OWNER,
            created_via_lease=self.created_via_lease,
        )

    def rate_limit(self) -> dict[str, object]:
        """When the subnet owner may next change the vector (root is exempt)."""
        return rate_limit_status(self.last_change_block, self.block)


@read(
    "subnet_emission_weights",
    {"netuid": "integer"},
    category="Subnets",
    param_docs={"netuid": "Subnet whose emission weight vector to read."},
)
async def subnet_emission_weights(view, netuid: int) -> Optional[SubnetEmissionWeights]:
    """The subnet's emission weight vector ``[burn, recycle, owner, validator,
    miner]`` with the bounds that apply to it, or None if the subnet does not
    exist (the root subnet has no vector).

    Pair with the ``set_subnet_emission_weights`` intent: ``min_validator``
    and ``created_via_lease`` are the per-subnet inputs the chain uses to
    accept or reject a change (``min_validator`` moves with root stake, alpha
    issuance and the global TAO weight, so it is a snapshot of this block).
    """
    view = await view.at()
    info = await view.runtime(api.SubnetInfoRuntimeApi.get_subnet_emission_weights, [netuid])
    if info is None:
        return None
    weights = normalize_weights(info["weights"])
    rate_limit_key = emission_weights_rate_limit_key(netuid)
    (root_tao, alpha_issuance, global_tao_weight), last_change = await asyncio.gather(
        root_preservation_inputs(view.query, netuid),
        view.query(st.SubtensorModule.LastRateLimitedBlock, [rate_limit_key]),
    )
    baseline_parts = int(info["root_baseline_share"])
    root_parts = min(int(info["root_share"]), weights[3])
    return SubnetEmissionWeights(
        netuid=int(info["netuid"]),
        weights=weights,
        shares={name: share_to_float(w) for name, w in zip(SLOT_NAMES, weights)},
        burn=int(info["burn"]),
        recycle=int(info["recycle"]),
        owner=int(info["owner"]),
        validator=int(info["validator"]),
        miner=int(info["miner"]),
        global_tao_weight=global_tao_weight,
        created_via_lease=bool(info["created_via_lease"]),
        root_tao=root_tao,
        alpha_issuance=alpha_issuance,
        root_share=share_to_float(root_parts),
        baseline_root_share=share_to_float(baseline_parts),
        baseline_root_parts=baseline_parts,
        validator_share_after_root=share_to_float(weights[3] - root_parts),
        root_capped=baseline_parts > weights[3],
        min_validator=min_validator_parts(baseline_parts),
        block=view.block,
        last_change_block=int(last_change or 0),
    )


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
