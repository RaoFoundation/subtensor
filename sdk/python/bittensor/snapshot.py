"""Block-pinned, read-only view of the chain: ``client.at(block)``.

A :class:`Snapshot` exposes the client's whole *read* surface — the generic
accessors (``query`` / ``query_map`` / ``query_batch`` / ``runtime`` /
``constant``), the read registry (``read`` / ``reads``), and the typed
namespaces (``subnets`` / ``staking`` / ``balances`` / ... — one per read
category) — with every call resolving against the same block. That gives two things:
consistency (no torn reads across blocks) and speed (the block hash resolves
once and is served from the transport's cache afterwards, instead of
re-resolving the chain head on every call).

Snapshots carry no write surface — a snapshot is a view, not a signer.
"""

from __future__ import annotations

from datetime import datetime
from typing import Any, Optional

from . import namespaces
from .balance import Balance
from .reads import base as read_registry


class Snapshot:
    """The client's read surface, pinned to one block."""

    def __init__(self, client: Any, block: int):
        self._client = client
        self.block = block
        # One namespace per read category (keep in sync with Client and
        # namespaces.NAMESPACES; explicit so type checkers see each attribute).
        self.balances = namespaces.Balances(self)
        self.chain = namespaces.Chain(self)
        self.collateral = namespaces.Collateral(self)
        self.delegation = namespaces.Delegation(self)
        self.epochs = namespaces.Epochs(self)
        self.hyperparameters = namespaces.Hyperparameters(self)
        self.identity = namespaces.Identity(self)
        self.leasing = namespaces.Leasing(self)
        self.locks = namespaces.Locks(self)
        self.neurons = namespaces.Neurons(self)
        self.prices = namespaces.Prices(self)
        self.staking = namespaces.Staking(self)
        self.subnets = namespaces.Subnets(self)
        self.weights = namespaces.Weights(self)

    # Generic accessors, pinned -------------------------------------------------

    async def query(self, item, params: Optional[list] = None) -> Any:
        """Read a storage item at this snapshot's block."""
        return await self._client.query(item, params, block=self.block)

    async def query_map(self, item, params: Optional[list] = None) -> list[tuple[Any, Any]]:
        """Read a whole storage map at this snapshot's block."""
        return await self._client.query_map(item, params, block=self.block)

    async def query_batch(self, item, param_sets: list[list]) -> list[Any]:
        """Read one storage map for many keys at this snapshot's block."""
        return await self._client.query_batch(item, param_sets, block=self.block)

    async def runtime(self, method, params: list) -> Any:
        """Call a runtime API at this snapshot's block."""
        return await self._client.runtime(method, params, block=self.block)

    async def constant(self, item) -> Any:
        """Read a pallet constant (constants only change with runtime upgrades)."""
        return await self._client.constant(item)

    # Registry reads, pinned -----------------------------------------------------

    async def read(self, name: str, **params: Any) -> Any:
        """Run a named typed read at this snapshot's block."""
        return await read_registry.dispatch(self, name, params)

    def reads(self) -> list[dict]:
        """Machine-readable catalog of every typed read (for agents)."""
        return read_registry.list_reads()

    # Chain metadata --------------------------------------------------------------

    async def timestamp(self) -> datetime:
        """UTC timestamp of this snapshot's block."""
        return await self._client.timestamp(block=self.block)

    async def block_time(self) -> float:
        """Seconds per block, detected from the chain."""
        return await self._client.block_time()

    async def is_fast_blocks(self) -> bool:
        """Whether the chain runs fast blocks (0.25s slots, local/e2e testing mode)."""
        return await self._client.is_fast_blocks()

    async def block_info(self, block: Optional[int] = None):
        """This block's header, timestamp, and decoded extrinsics (or another block's)."""
        return await self._client.block_info(self.block if block is None else block)

    # View plumbing ----------------------------------------------------------------

    def balance(self, rao: int, netuid: int = 0) -> Balance:
        """A :class:`Balance` tagged with the connection's token symbol."""
        return self._client.balance(rao, netuid)

    @property
    def token_symbols(self) -> dict[int, str]:
        return self._client.token_symbols

    async def at(self, block: Optional[int] = None) -> "Snapshot":
        """This snapshot (already pinned), or a sibling pinned to another block."""
        if block is None or block == self.block:
            return self
        return Snapshot(self._client, block)

    def __repr__(self) -> str:
        return f"Snapshot(block={self.block})"
