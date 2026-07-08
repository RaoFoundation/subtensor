"""Neuron and registration reads."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from .base import read


@dataclass
class Neuron:
    """One registered neuron, as reported by the chain's runtime API.

    ``raw`` carries the full decoded record (axon info, rank/trust/consensus,
    weights when ``lite=False``, ...) for anything not lifted into a field.
    """

    uid: int
    hotkey: str
    coldkey: str
    active: bool
    validator_permit: bool
    last_update: int
    total_stake: Balance
    raw: dict = field(repr=False)


@read(
    "neurons",
    {"netuid": "integer", "lite": "boolean"},
    category="Neurons & registration",
    param_docs={
        "netuid": "Subnet whose neurons to list.",
        "lite": "Omit per-neuron weights/bonds (much smaller; what most callers want).",
    },
)
async def neurons(view, netuid: int, lite: bool = True) -> list[Neuron]:
    """Every neuron on a subnet in ONE runtime-API call (the metagraph fast path).

    `lite=true` (the default) omits per-neuron weights/bonds, which is far
    smaller and what almost every caller wants.
    """
    method = (
        api.NeuronInfoRuntimeApi.get_neurons_lite if lite else api.NeuronInfoRuntimeApi.get_neurons
    )
    records = await view.runtime(method, [netuid])
    out = []
    for record in records or []:
        stake_rao = sum(int(amount) for _, amount in record.get("stake") or [])
        out.append(
            Neuron(
                uid=int(record["uid"]),
                hotkey=str(record["hotkey"]),
                coldkey=str(record["coldkey"]),
                active=bool(record["active"]),
                validator_permit=bool(record["validator_permit"]),
                last_update=int(record["last_update"]),
                total_stake=view.balance(stake_rao, netuid),
                raw=record,
            )
        )
    return sorted(out, key=lambda n: n.uid)


@read(
    "hotkey_owner",
    {"hotkey_ss58": "string"},
    category="Neurons & registration",
    param_docs={"hotkey_ss58": "Hotkey whose owning coldkey to look up."},
)
async def hotkey_owner(view, hotkey_ss58: str) -> Optional[str]:
    """The coldkey that owns a hotkey, or None if unowned."""
    value = await view.query(st.SubtensorModule.Owner, [hotkey_ss58])
    return str(value) if value else None


@read(
    "owned_hotkeys",
    {"coldkey_ss58": "string"},
    category="Neurons & registration",
    param_docs={"coldkey_ss58": "Coldkey whose hotkeys to list."},
)
async def owned_hotkeys(view, coldkey_ss58: str) -> list[str]:
    """Every hotkey owned by a coldkey (the reverse of `hotkey_owner`)."""
    value = await view.query(st.SubtensorModule.OwnedHotkeys, [coldkey_ss58])
    return [str(hotkey) for hotkey in value or []]


@read(
    "netuids_for_hotkey",
    {"hotkey_ss58": "string"},
    category="Neurons & registration",
    param_docs={"hotkey_ss58": "Hotkey whose subnet registrations to list."},
)
async def netuids_for_hotkey(view, hotkey_ss58: str) -> list[int]:
    """Every subnet a hotkey is registered on, as a sorted list of netuids."""
    rows = await view.query_map(st.SubtensorModule.IsNetworkMember, [hotkey_ss58])
    return sorted(int(netuid) for netuid, is_member in rows if is_member)


@read(
    "uid",
    {"hotkey_ss58": "string", "netuid": "integer"},
    category="Neurons & registration",
    param_docs={
        "hotkey_ss58": "Hotkey whose UID to look up.",
        "netuid": "Subnet to query.",
    },
)
async def uid(view, hotkey_ss58: str, netuid: int) -> Optional[int]:
    """UID of a hotkey on a subnet, or None if not registered there."""
    value = await view.query(st.SubtensorModule.Uids, [netuid, hotkey_ss58])
    return None if value is None else int(value)


@read(
    "associated_evm_key",
    {"netuid": "integer", "uid": "integer"},
    category="Neurons & registration",
    param_docs={
        "netuid": "Subnet the neuron is registered on.",
        "uid": "UID of the neuron on that subnet.",
    },
)
async def associated_evm_key(view, netuid: int, uid: int) -> Optional[dict]:
    """The EVM address associated with a neuron (by netuid + uid) and the block it was set."""
    value = await view.query(st.SubtensorModule.AssociatedEvmAddress, [netuid, uid])
    if not value:
        return None
    evm_address, block = value
    return {"evm_address": str(evm_address), "block": int(block)}
