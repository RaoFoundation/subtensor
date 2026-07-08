"""Typed read namespaces: ``client.balances`` / ``staking`` / ``subnets`` / ``neurons``.

Each namespace is a thin projection over the read registry (``bittensor.reads``)
— the fetch functions are the single implementation; these classes only give
them a discoverable, typed, dot-completed surface. Every method takes an
optional ``block=`` that pins the read via ``view.at(block)``; a namespace
built over a snapshot is already pinned, so ``block`` can be omitted there.
"""

from __future__ import annotations

from typing import Optional

from . import metagraph as metagraph_module
from .balance import Balance
from .metagraph import Metagraph, NeuronCommitment
from .reads import accounts as _accounts
from .reads import neurons as _neurons
from .reads import staking as _staking
from .reads import subnets as _subnets
from .reads.neurons import Neuron
from .reads.staking import StakePosition
from .reads.subnets import SubnetInfo


async def _scoped(view, block: Optional[int]):
    """The view a ``block=`` keyword selects: the ambient one when None."""
    return view if block is None else await view.at(block)


class Balances:
    def __init__(self, view):
        self._view = view

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


class Staking:
    def __init__(self, view):
        self._view = view

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


class Subnets:
    """Aggregating/decoding subnet reads. For a single raw value (e.g. does netuid
    N exist), use the generic ``client.query(storage.SubtensorModule.NetworksAdded,
    [netuid])`` accessor instead of a bespoke method.
    """

    def __init__(self, view):
        self._view = view

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
        status. See :class:`Metagraph`.
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


class Neurons:
    def __init__(self, view):
        self._view = view

    async def all(
        self, netuid: int, block: Optional[int] = None, *, lite: bool = True
    ) -> list[Neuron]:
        """Every neuron on a subnet in ONE runtime-API call (the metagraph fast path).

        ``lite=True`` (default) omits per-neuron weights/bonds, which is far
        smaller and what almost every caller wants.
        """
        return await _neurons.neurons(await _scoped(self._view, block), netuid, lite=lite)
