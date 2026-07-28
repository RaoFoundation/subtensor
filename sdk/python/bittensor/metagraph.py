"""Typed metagraph: the whole state of a subnet as one object.

``client.subnets.metagraph(netuid)`` returns a :class:`Metagraph` — every
neuron with its stake, rank/trust/consensus, axon endpoint and identity, plus
the subnet-level pool and epoch fields, in one runtime-API call. Unlike the
``bittensor`` SDK's Metagraph this also carries each hotkey's **commitment**
(the Commitments pallet), including timelocked ones: their plaintext value once
visible, the block they were published at, and whether the sealed payload has
been decrypted on chain yet.

The raw runtime record stays available as ``metagraph.raw`` for any field not
lifted into a typed attribute. For per-uid weights/bonds matrices use
``client.neurons.all(netuid, lite=False)`` — they are omitted here because
they dominate the payload size and almost no caller wants them.
"""

from __future__ import annotations

import asyncio
import ipaddress
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Any, Iterator, Optional

from . import timelock
from ._generated.runtime_apis import SubnetInfoRuntimeApi
from ._generated.storage import Commitments as st_commitments
from ._generated.storage import SubtensorModule as st_subtensor
from .balance import Balance
from .hyperparams import ratio_fraction
from .settings import BLOCKTIME, GLOBAL_MAX_SUBNET_COUNT

# I96F32 fixed point: 32 fractional bits (moving_price encoding).
_I96F32_ONE = 2**32


class _CommitmentMap(dict):
    """A dict whose ``[]`` yields None for absent keys — most uids/hotkeys have
    no commitment, and "no commitment" is an answer, not an error."""

    def __missing__(self, key):
        return None


@dataclass
class NeuronCommitment:
    """A hotkey's on-chain commitment on one subnet, timelock-aware.

    ``data`` is what is readable *right now* from the commitment itself: the
    plaintext ``Raw`` fields, plus hash variants rendered as ``sha256:0x…``
    (empty when the whole payload is sealed). ``revealed`` is the
    chain-decrypted history from the Commitments pallet: ``(reveal_block,
    plaintext)`` pairs, oldest first. Use :attr:`value` for "the current
    visible content" and :attr:`decrypted` / :attr:`status` for where a sealed
    payload is in its lifecycle.

    The chain drops a commitment's storage entry once its sealed payload is
    fully revealed; such commitments still appear here (status ``revealed``)
    with ``block`` set to the reveal block, since the commit block is gone.
    """

    hotkey: str
    netuid: int
    uid: Optional[int]  # the hotkey's uid, None when it is no longer registered
    block: int  # block the commitment was published at
    queried_block: int  # block the read was made against (age is measured to here)
    deposit: Balance
    data: str  # plaintext (Raw) part: utf-8 if possible, else 0x-hex, "" if none
    encrypted: bool  # carries a TimelockEncrypted payload
    reveal_round: Optional[int]  # drand round the sealed payload opens at
    revealed: list[tuple[int, str]]  # (reveal_block, plaintext), oldest first
    fields: list = field(repr=False)  # raw decoded field variants

    @property
    def decrypted(self) -> bool:
        """True once the chain has decrypted a payload revealed at/after this commitment."""
        return any(reveal_block >= self.block for reveal_block, _ in self.revealed)

    @property
    def status(self) -> str:
        """``"plain"`` (never sealed), ``"sealed"`` (waiting on drand), or ``"revealed"``."""
        if self.decrypted:
            return "revealed"
        return "sealed" if self.encrypted else "plain"

    @property
    def is_revealed(self) -> bool:
        """True when the content is readable now — committed in the clear or
        already chain-decrypted. False while a sealed payload waits on drand
        (``data`` may still carry a plaintext part committed alongside it)."""
        return self.status != "sealed"

    @property
    def age_blocks(self) -> int:
        """Blocks the commitment has been on chain, as of ``queried_block``."""
        return max(0, self.queried_block - self.block)

    @property
    def duration(self) -> timedelta:
        """Wall-clock time the commitment has been on chain (``age_blocks`` × block time)."""
        return timedelta(seconds=self.age_blocks * BLOCKTIME)

    @property
    def value(self) -> Optional[str]:
        """The commitment's currently visible content, or None while sealed.

        The latest chain-decrypted plaintext when one exists for this
        commitment, else the plaintext part committed in the clear.
        """
        for reveal_block, text in reversed(self.revealed):
            if reveal_block >= self.block:
                return text
        return self.data or None

    @property
    def reveals_at(self) -> Optional[datetime]:
        """UTC moment a sealed payload becomes decryptable (None when not sealed)."""
        if self.reveal_round is None or self.decrypted:
            return None
        return timelock.reveal_time(self.reveal_round)


@dataclass
class MetagraphNeuron:
    """One neuron's full row of the metagraph.

    Scores (``rank`` / ``trust`` / ``consensus`` / ``incentive`` / ``dividends``
    / ``pruning_score``) are normalized to 0..1. ``emission`` and stakes are
    :class:`Balance` values in the subnet's own units (``tao_stake`` is root
    TAO). ``axon`` is the served ``ip:port`` or None when nothing is served.
    """

    uid: int
    hotkey: str
    coldkey: str
    active: bool
    validator_permit: bool
    last_update: int
    block_at_registration: int
    rank: float
    trust: float
    consensus: float
    incentive: float
    dividends: float
    pruning_score: float
    emission: Balance
    alpha_stake: Balance
    tao_stake: Balance
    total_stake: Balance
    axon: Optional[str]
    identity: Optional[dict]
    commitment: Optional[NeuronCommitment]
    # Miner collateral (zero for hotkeys without a collateral entry; None on
    # runtimes older than spec 435, which do not report collateral).
    collateral_locked: Optional[Balance] = None
    collateral_min: Optional[Balance] = None
    collateral_earned: Optional[Balance] = None


@dataclass
class Metagraph:
    """A subnet's neurons, pool state, and commitments at one block.

    ``neurons`` is ordered by uid. ``commitments`` maps uid -> commitment for
    the registered neurons (each neuron also carries its own as
    ``neuron.commitment``); commitments left behind by hotkeys that have since
    deregistered are in ``unregistered_commitments``, keyed by hotkey. Both
    maps yield None (rather than raising) for a key with no commitment.
    ``raw`` is the untouched runtime record for everything not lifted
    (hyperparameters, dividend breakdowns, ...).
    """

    netuid: int
    mechid: int
    name: str
    symbol: str
    block: int
    tempo: int
    last_step: int
    blocks_since_last_step: int
    owner_hotkey: str
    owner_coldkey: str
    network_registered_at: int
    num_uids: int
    max_uids: int
    price: Optional[float]  # τ per alpha, spot (tao_in / alpha_in)
    moving_price: Optional[float]
    identity: Optional[dict]
    neurons: list[MetagraphNeuron]
    commitments: dict[int, NeuronCommitment]
    unregistered_commitments: dict[str, NeuronCommitment]
    raw: dict = field(repr=False)

    @property
    def hotkeys(self) -> list[str]:
        return [n.hotkey for n in self.neurons]

    @property
    def coldkeys(self) -> list[str]:
        return [n.coldkey for n in self.neurons]

    @property
    def validators(self) -> list[MetagraphNeuron]:
        """Neurons holding a validator permit."""
        return [n for n in self.neurons if n.validator_permit]

    def neuron(self, uid: int) -> MetagraphNeuron:
        """The neuron at a uid (raises ``KeyError`` for an unknown uid)."""
        if 0 <= uid < len(self.neurons):
            return self.neurons[uid]
        raise KeyError(f"no uid {uid} on netuid {self.netuid} ({len(self.neurons)} uids)")

    def by_hotkey(self, hotkey: str) -> Optional[MetagraphNeuron]:
        """The neuron registered under a hotkey, or None."""
        for n in self.neurons:
            if n.hotkey == hotkey:
                return n
        return None

    def __len__(self) -> int:
        return len(self.neurons)

    def __iter__(self) -> Iterator[MetagraphNeuron]:
        return iter(self.neurons)

    def __repr__(self) -> str:
        return (
            f"Metagraph(netuid={self.netuid}, name={self.name!r}, block={self.block}, "
            f"neurons={len(self.neurons)}, commitments={len(self.commitments)}"
            f"+{len(self.unregistered_commitments)} unregistered)"
        )


async def fetch(view, netuid: int, *, commitments: bool = True) -> Optional[Metagraph]:
    """Build a :class:`Metagraph`; None when the subnet does not exist.

    One runtime-API call for the graph plus (when ``commitments``) two storage
    map reads for the Commitments pallet, all pinned to the same block.
    ``view`` is a client or snapshot (see ``reads.base``).
    """
    view = await view.at()
    graph_task = view.runtime(SubnetInfoRuntimeApi.get_metagraph, [netuid])
    if commitments:
        graph, commitment_map = await asyncio.gather(
            graph_task, _fetch_commitment_map(view, netuid)
        )
    else:
        graph, commitment_map = await graph_task, {}
    if not isinstance(graph, dict):
        return None
    return _build(netuid, graph, commitment_map)


async def fetch_commitments(view, netuid: int) -> dict[str, NeuronCommitment]:
    """Every commitment on a subnet, keyed by hotkey, newest first.

    Batched storage map reads pinned to one block — no metagraph runtime call —
    so it stays cheap on large subnets. Each entry carries the hotkey's uid
    (None once deregistered). Includes commitments from hotkeys that have since
    deregistered, and fully-revealed commitments whose live storage entry the
    chain has already dropped.
    """
    view = await view.at()
    commitment_map = await _fetch_commitment_map(view, netuid, with_uids=True)
    ordered = sorted(commitment_map.values(), key=lambda c: c.block, reverse=True)
    return _CommitmentMap((c.hotkey, c) for c in ordered)


async def _fetch_commitment_map(
    view, netuid: int, *, with_uids: bool = False
) -> dict[str, NeuronCommitment]:
    """``view`` must already be block-pinned (its ``block`` dates the entries)."""
    committed, revealed, uid_rows = await asyncio.gather(
        view.query_map(st_commitments.CommitmentOf, [netuid]),
        view.query_map(st_commitments.RevealedCommitments, [netuid]),
        view.query_map(st_subtensor.Uids, [netuid]) if with_uids else _none(),
    )
    out = _commitments(netuid, committed, revealed, view.block)
    if uid_rows:
        uids = {str(hotkey): int(uid) for hotkey, uid in uid_rows}
        for hotkey, commitment in out.items():
            commitment.uid = uids.get(hotkey)
    return out


async def _none() -> None:
    return None


# --- Decoding ----------------------------------------------------------------


def _build(netuid: int, graph: dict, commitment_map: dict[str, NeuronCommitment]) -> Metagraph:
    hotkeys = [str(hk) for hk in graph.get("hotkeys") or []]
    # The metagraph response carries the subnet's own token symbol; tag the
    # alpha-denominated balances with it so they render correctly on their own.
    symbol = _text(graph.get("symbol")) or None

    # Split commitments into registered (keyed by uid) and left-behind ones.
    by_uid: dict[int, NeuronCommitment] = _CommitmentMap()
    for uid, hotkey in enumerate(hotkeys):
        commitment = commitment_map.pop(hotkey, None)
        if commitment is not None:
            commitment.uid = uid
            by_uid[uid] = commitment

    def column(key: str) -> list:
        values = list(graph.get(key) or [])
        return values + [None] * (len(hotkeys) - len(values))

    coldkeys = column("coldkeys")
    identities = column("identities")
    axons = column("axons")
    active = column("active")
    permits = column("validator_permit")
    last_update = column("last_update")
    registered = column("block_at_registration")
    ranks = column("rank")
    trusts = column("trust")
    consensus = column("consensus")
    incentives = column("incentives")
    dividends = column("dividends")
    pruning = column("pruning_score")
    emission = column("emission")
    alpha_stake = column("alpha_stake")
    tao_stake = column("tao_stake")
    total_stake = column("total_stake")
    # Present from spec 435; None-columns on older runtimes.
    has_collateral = graph.get("collateral_locked") is not None
    collateral_locked = column("collateral_locked")
    collateral_min = column("collateral_min")
    collateral_earned = column("collateral_earned")

    def collateral(values: list, uid: int) -> Optional[Balance]:
        if not has_collateral:
            return None
        return Balance.from_rao(int(values[uid] or 0), netuid, symbol)

    def score(values: list, uid: int) -> float:
        # Runtime-API values, so no storage descriptor carries their identity;
        # the runtime declares these vectors PerU16 (a u16 fraction over 65535).
        return ratio_fraction("PerU16", int(values[uid] or 0)) or 0.0

    neurons = [
        MetagraphNeuron(
            uid=uid,
            hotkey=hotkey,
            coldkey=str(coldkeys[uid]),
            active=bool(active[uid]),
            validator_permit=bool(permits[uid]),
            last_update=int(last_update[uid] or 0),
            block_at_registration=int(registered[uid] or 0),
            rank=score(ranks, uid),
            trust=score(trusts, uid),
            consensus=score(consensus, uid),
            incentive=score(incentives, uid),
            dividends=score(dividends, uid),
            pruning_score=score(pruning, uid),
            emission=Balance.from_rao(int(emission[uid] or 0), netuid, symbol),
            alpha_stake=Balance.from_rao(int(alpha_stake[uid] or 0), netuid, symbol),
            tao_stake=Balance.from_rao(int(tao_stake[uid] or 0)),
            total_stake=Balance.from_rao(int(total_stake[uid] or 0), netuid, symbol),
            axon=_axon_endpoint(axons[uid]),
            identity=identities[uid] if isinstance(identities[uid], dict) else None,
            commitment=by_uid.get(uid),
            collateral_locked=collateral(collateral_locked, uid),
            collateral_min=collateral(collateral_min, uid),
            collateral_earned=collateral(collateral_earned, uid),
        )
        for uid, hotkey in enumerate(hotkeys)
    ]

    tao_in = int(graph.get("tao_in") or 0)
    alpha_in = int(graph.get("alpha_in") or 0)
    moving_bits = (graph.get("moving_price") or {}).get("bits")
    identity = graph.get("identity")

    return Metagraph(
        netuid=netuid,
        mechid=int(graph.get("netuid") or netuid) // GLOBAL_MAX_SUBNET_COUNT,
        name=_text(graph.get("name")),
        symbol=_text(graph.get("symbol")),
        block=int(graph["block"]),
        tempo=int(graph["tempo"]),
        last_step=int(graph["last_step"]),
        blocks_since_last_step=int(graph["blocks_since_last_step"]),
        owner_hotkey=str(graph["owner_hotkey"]),
        owner_coldkey=str(graph["owner_coldkey"]),
        network_registered_at=int(graph["network_registered_at"]),
        num_uids=int(graph.get("num_uids") or len(neurons)),
        max_uids=int(graph.get("max_uids") or 0),
        price=(tao_in / alpha_in) if alpha_in else None,
        moving_price=(int(moving_bits) / _I96F32_ONE) if moving_bits is not None else None,
        identity=identity if isinstance(identity, dict) else None,
        neurons=neurons,
        commitments=by_uid,
        unregistered_commitments=_CommitmentMap(commitment_map),
        raw=graph,
    )


def _commitments(
    netuid: int,
    committed: list[tuple[Any, Any]],
    revealed: list[tuple[Any, Any]],
    queried_block: int,
) -> dict[str, NeuronCommitment]:
    revealed_map: dict[str, list[tuple[int, str]]] = {}
    for hotkey, entries in revealed or []:
        revealed_map[str(hotkey)] = [_revealed_entry(e) for e in entries or []]

    out: dict[str, NeuronCommitment] = {}
    for hotkey, record in committed or []:
        hotkey = str(hotkey)
        if not isinstance(record, dict):
            continue
        fields = list(((record.get("info") or {}).get("fields")) or [])
        reveal_round = _timelock_round(fields)
        out[hotkey] = NeuronCommitment(
            hotkey=hotkey,
            netuid=netuid,
            uid=None,
            block=int(record.get("block") or 0),
            queried_block=queried_block,
            deposit=Balance.from_rao(int(record.get("deposit") or 0)),
            data=_decode_fields(fields),
            encrypted=reveal_round is not None,
            reveal_round=reveal_round,
            revealed=sorted(revealed_map.get(hotkey, [])),
            fields=fields,
        )

    # Fully-revealed commitments lose their storage entry on reveal; keep them
    # visible through their revealed history (block = the reveal block).
    for hotkey, entries in revealed_map.items():
        if hotkey in out or not entries:
            continue
        latest = max(reveal_block for reveal_block, _ in entries)
        out[hotkey] = NeuronCommitment(
            hotkey=hotkey,
            netuid=netuid,
            uid=None,
            block=latest,
            queried_block=queried_block,
            deposit=Balance.from_rao(0),
            data="",
            encrypted=True,
            reveal_round=None,
            revealed=sorted(entries),
            fields=[],
        )
    return out


def _text(value: Any) -> str:
    """Decode a chain byte-string (list of ints, hex, bytes, or str) to text."""
    if value is None:
        return ""
    if isinstance(value, str):
        if value.startswith("0x"):
            value = bytes.fromhex(value[2:])
        else:
            return value
    if isinstance(value, (list, tuple)):
        value = bytes(int(b) for b in value)
    if isinstance(value, (bytes, bytearray)):
        return bytes(value).decode("utf-8", errors="replace")
    return str(value)


def _decode_fields(fields: list) -> str:
    """The readable content of commitment fields.

    ``Raw*`` bytes concatenate to utf-8 (else 0x-hex); hash variants render as
    ``sha256:0x…``; ``TimelockEncrypted`` payloads are sealed and contribute
    nothing here.
    """
    raw = b""
    hashes = []
    for entry in fields:
        for variant, value in (entry or {}).items():
            if variant.startswith("Raw") and isinstance(value, str):
                raw += bytes.fromhex(value.removeprefix("0x"))
            elif variant != "TimelockEncrypted" and isinstance(value, str):
                hashes.append(f"{variant.lower()}:{value}")
    parts = []
    if raw:
        try:
            parts.append(raw.decode("utf-8"))
        except UnicodeDecodeError:
            parts.append("0x" + raw.hex())
    return "\n".join(parts + hashes)


def _timelock_round(fields: list) -> Optional[int]:
    """The latest drand reveal round among TimelockEncrypted fields, or None."""
    rounds = [
        int(value["reveal_round"])
        for entry in fields
        for variant, value in (entry or {}).items()
        if variant == "TimelockEncrypted" and isinstance(value, dict)
    ]
    return max(rounds) if rounds else None


def _revealed_entry(entry: Any) -> tuple[int, str]:
    """Decode one RevealedCommitments (data, reveal_block) pair.

    The revealed data is the SCALE bytes of the decrypted payload: a compact
    length prefix followed by the plaintext. The transport hands it over as
    text when the bytes happen to be valid utf-8, else as 0x-hex.
    """
    data, reveal_block = entry
    if isinstance(data, (bytes, bytearray)):
        raw = bytes(data)
    elif isinstance(data, str) and data.startswith("0x"):
        raw = bytes.fromhex(data[2:])
    else:
        raw = str(data).encode("utf-8")
    if raw:
        mode = raw[0] & 0b11
        # Big-int compact (mode 0b11): the low byte says how many length bytes follow.
        offset = (
            1 + (raw[0] >> 2) + 4 if mode == 0b11 else 1 if mode == 0 else 2 if mode == 1 else 4
        )
        raw = raw[offset:]
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        text = "0x" + raw.hex()
    return int(reveal_block), text


def _axon_endpoint(axon: Any) -> Optional[str]:
    """``ip:port`` for a served axon, or None when nothing is served."""
    if not isinstance(axon, dict):
        return None
    ip = int(axon.get("ip") or 0)
    if not ip:
        return None
    port = int(axon.get("port") or 0)
    if int(axon.get("ip_type") or 4) == 6:
        return f"[{ipaddress.IPv6Address(ip)}]:{port}"
    return f"{ipaddress.IPv4Address(ip)}:{port}"
