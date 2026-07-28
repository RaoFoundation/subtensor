"""Typed read namespaces: ``client.subnets`` / ``staking`` / ``balances`` / ...

One namespace per read category, each a projection over the read registry
(``bittensor.reads``) with two layers:

- **Curated methods** (hand-written below): the ergonomic surface — renamed
  (``subnets.all()``), aggregated, or more richly typed (``subnets.metagraph()``
  returns :class:`Metagraph`) than the raw registry read.
- **Every registered read**, under its registry name, resolved dynamically by
  :class:`_ReadNamespace.__getattr__` — so ``client.subnets.subnet_registration_cost()``
  works the moment the read is registered, with no per-read wrapper to write.
  Curated methods shadow same-named reads.

Autocomplete and type checking come from ``namespaces.pyi``, generated from
this module plus the registry (``python -m codegen.emit_namespaces``; CI gate:
``python -m codegen.check --namespaces``).

Every method takes an optional ``block=`` that pins the read via
``view.at(block)``; a namespace built over a snapshot is already pinned, so
``block`` can be omitted there.
"""

from __future__ import annotations

import inspect
from typing import ClassVar, Optional

from . import metagraph as metagraph_module
from .balance import Balance
from .metagraph import Metagraph, NeuronCommitment
from .reads import accounts as _accounts
from .reads import neurons as _neurons
from .reads import staking as _staking
from .reads import subnets as _subnets
from .reads.base import REGISTRY, ReadSpec
from .reads.neurons import Neuron
from .reads.staking import StakePosition
from .reads.subnets import SubnetInfo


async def _scoped(view, block: Optional[int]):
    """The view a ``block=`` keyword selects: the ambient one when None."""
    return view if block is None else await view.at(block)


def _accepts_pin(spec: ReadSpec) -> bool:
    """Whether the dynamic wrapper may add its own ``block=`` pin — not when the
    read already takes a parameter of that name (e.g. ``block_info``)."""
    return "block" not in inspect.signature(spec.fetch).parameters


class _ReadNamespace:
    """Dynamic projection of one read category over a view.

    Attribute lookup falls back to the read registry: any read in this
    namespace's category is callable under its registry name, with a
    keyword-only ``block=`` pinning the read via ``view.at(block)``.
    Hand-written methods on subclasses shadow same-named reads. The generated
    ``namespaces.pyi`` declares both surfaces, so the stub — not this
    ``__getattr__`` — is what type checkers see.
    """

    _category: ClassVar[str]

    def __init__(self, view):
        self._view = view

    @classmethod
    def _read_names(cls) -> set[str]:
        return {name for name, spec in REGISTRY.items() if spec.category == cls._category}

    def __getattr__(self, name: str):
        spec = REGISTRY.get(name)
        if spec is None or spec.category != self._category:
            raise AttributeError(
                f"{type(self).__name__} has no method or read {name!r}; "
                f"registry reads here: {', '.join(sorted(self._read_names()))}"
            )
        view = self._view
        if _accepts_pin(spec):

            async def method(*args, block: Optional[int] = None, **kwargs):
                return await spec.fetch(await _scoped(view, block), *args, **kwargs)

        else:

            async def method(*args, **kwargs):
                return await spec.fetch(view, *args, **kwargs)

        method.__name__ = spec.name
        method.__qualname__ = f"{type(self).__name__}.{spec.name}"
        method.__doc__ = spec.doc
        return method

    def __dir__(self):
        return sorted(set(super().__dir__()) | self._read_names())


class Balances(_ReadNamespace):
    """Free-TAO balances, plus every "Accounts & keys" read (multisig, proxies, ...)."""

    _category = "Accounts & keys"

    async def get(self, address: str, block: Optional[int] = None) -> Balance:
        """Free TAO balance of a coldkey address."""
        return await _accounts.balance(await _scoped(self._view, block), address)

    async def get_many(
        self, addresses: list[str], block: Optional[int] = None
    ) -> dict[str, Balance]:
        """Free TAO balance for several coldkey addresses in one batched request."""
        return await _accounts.balances(await _scoped(self._view, block), addresses)

    async def existential_deposit(self) -> Balance:
        """Minimum balance an account must keep to stay alive."""
        return await _accounts.existential_deposit(self._view)


class Staking(_ReadNamespace):
    """Stake amounts and positions, plus every "Staking" read."""

    _category = "Staking"

    async def get(
        self,
        coldkey_ss58: str,
        hotkey_ss58: str,
        netuid: int,
        block: Optional[int] = None,
    ) -> Balance:
        """Alpha staked by a coldkey to a hotkey on a subnet (TAO when netuid is 0)."""
        return await _staking.stake(
            await _scoped(self._view, block), coldkey_ss58, hotkey_ss58, netuid
        )

    async def positions(
        self, coldkey_ss58: str, block: Optional[int] = None
    ) -> list[StakePosition]:
        """Every stake position held by a coldkey, across all hotkeys and subnets."""
        return await _staking.stake_for_coldkey(await _scoped(self._view, block), coldkey_ss58)


class Subnets(_ReadNamespace):
    """Aggregating/decoding subnet reads, plus every "Subnets" read.

    For a single raw storage value (e.g. does netuid N exist), the generic
    ``client.query(storage.SubtensorModule.NetworksAdded, [netuid])`` accessor
    also works.
    """

    _category = "Subnets"

    async def burn(self, netuid: int, block: Optional[int] = None) -> Balance:
        """Current burn (recycle) cost to register on a subnet."""
        return await _subnets.burn(await _scoped(self._view, block), netuid)

    async def commit_reveal_enabled(self, netuid: int, block: Optional[int] = None) -> bool:
        """Whether commit-reveal weights are enabled on a subnet."""
        return await _subnets.commit_reveal_enabled(await _scoped(self._view, block), netuid)

    async def metagraph(
        self, netuid: int, block: Optional[int] = None, *, commitments: bool = True
    ) -> Optional[Metagraph]:
        """The typed metagraph for a subnet, or None when it does not exist.

        Every neuron with stakes, scores, axon endpoint, identity, and (unless
        ``commitments=False``) its on-chain commitment with timelock/decryption
        status. See :class:`Metagraph`. (This shadows the registry's raw
        ``metagraph`` read, which stays reachable via ``client.read("metagraph")``.)
        """
        return await metagraph_module.fetch(
            await _scoped(self._view, block), netuid, commitments=commitments
        )

    async def commitments(
        self, netuid: int, block: Optional[int] = None
    ) -> dict[str, NeuronCommitment]:
        """Every commitment on a subnet, keyed by hotkey, newest first.

        The bulk view a subnet owner polls: each entry carries the hotkey, its
        uid (None once deregistered), the visible content (``.value``), the
        commit block, how long it has been on chain (``.duration`` /
        ``.age_blocks``), and its reveal state (``.is_revealed`` / ``.status``
        / ``.reveals_at``), plus the chain-decrypted history (``.revealed``).
        Much cheaper than ``metagraph()`` when commitments are all you need.
        """
        return await metagraph_module.fetch_commitments(await _scoped(self._view, block), netuid)

    async def info(self, netuid: int, block: Optional[int] = None) -> SubnetInfo:
        """Tempo, burn, and neuron count for one subnet (the three reads run concurrently)."""
        return await _subnets.subnet(await _scoped(self._view, block), netuid)

    async def all(self, block: Optional[int] = None) -> list[SubnetInfo]:
        """Info for every subnet, fetched in four batched map queries rather than
        one-query-per-subnet. This is what listing should use.
        """
        return await _subnets.subnets(await _scoped(self._view, block))


class Neurons(_ReadNamespace):
    """Neuron listings and registration lookups, plus every "Neurons & registration" read."""

    _category = "Neurons & registration"

    async def all(
        self, netuid: int, block: Optional[int] = None, *, lite: bool = True
    ) -> list[Neuron]:
        """Every neuron on a subnet in ONE runtime-API call (the metagraph fast path).

        ``lite=True`` (default) omits per-neuron weights/bonds, which is far
        smaller and what almost every caller wants.
        """
        return await _neurons.neurons(await _scoped(self._view, block), netuid, lite=lite)


class Chain(_ReadNamespace):
    """Chain-level reads: time, block metadata, global limits."""

    _category = "Chain"


class Delegation(_ReadNamespace):
    """Delegate info, delegated stake, and child/parent hotkey relations."""

    _category = "Delegation"


class Epochs(_ReadNamespace):
    """Epoch timing: blocks since/until steps, epoch status."""

    _category = "Epochs & timing"


class Hyperparameters(_ReadNamespace):
    """Per-subnet hyperparameter scalars (difficulty, immunity period, ...)."""

    _category = "Hyperparameters"


class Identity(_ReadNamespace):
    """On-chain identities and commitments."""

    _category = "Identity & commitments"


class Leasing(_ReadNamespace):
    """Leases and crowdloans."""

    _category = "Leases & crowdloans"


class Collateral(_ReadNamespace):
    """Miner registration collateral."""

    _category = "Mining & collateral"


class Locks(_ReadNamespace):
    """Stake locks and conviction."""

    _category = "Locks & conviction"


class Prices(_ReadNamespace):
    """Alpha prices and stake/unstake swap quotes."""

    _category = "Prices & swaps"


class Weights(_ReadNamespace):
    """Weights, bonds, and timelocked weight commits."""

    _category = "Weights & bonds"


# Namespace attribute name on Client / Snapshot -> namespace class. The sync
# facade and the namespaces.pyi emitter iterate this; client.py and snapshot.py
# assign each attribute explicitly so type checkers see them.
NAMESPACES: dict[str, type[_ReadNamespace]] = {
    "balances": Balances,
    "chain": Chain,
    "collateral": Collateral,
    "delegation": Delegation,
    "epochs": Epochs,
    "hyperparameters": Hyperparameters,
    "identity": Identity,
    "leasing": Leasing,
    "locks": Locks,
    "neurons": Neurons,
    "prices": Prices,
    "staking": Staking,
    "subnets": Subnets,
    "weights": Weights,
}
