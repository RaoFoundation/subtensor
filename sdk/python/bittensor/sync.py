"""Synchronous facade over the async ``Client``.

There is exactly one implementation of the SDK (the async one). ``SyncClient``
runs a private event loop on a background thread and proxies every coroutine
method on the client's namespaces to it, so synchronous callers get a blocking
API without a second hand-written codebase.

Construction is cold: no network I/O happens until ``connect()`` or the first
chain-touching call (whichever comes first), so a ``SyncClient`` can be built
offline — e.g. in tests, or with an injected fake ``substrate``.
"""

from __future__ import annotations

import asyncio
import contextlib
import threading
import weakref
from datetime import datetime
from typing import Any, Callable, Iterator, Optional

from ._substrate import Substrate
from .client import BlockHeader, Client
from .intents import Policy
from .namespaces import NAMESPACES
from .settings import DEFAULT_NETWORK
from .snapshot import Snapshot

_NAMESPACES = tuple(NAMESPACES)


def _teardown_sync(loop: "_Loop", client: Client, state: dict) -> None:
    """One teardown path for explicit ``close()`` and the GC finalizer.

    Always shuts the loop down, even when ``client.close()`` raises — otherwise
    a failed substrate close would leave the background thread running with no
    finalizer left to clean it up.
    """
    if loop.is_shutdown:
        return
    try:
        # The cycle collector can run this on any thread — including the loop's
        # own (it allocates constantly). Blocking on the loop from its own thread
        # would deadlock; just stop the loop and let the daemon thread drop the
        # socket with it.
        if state.get("connected") and not loop.owns_current_thread:
            with contextlib.suppress(Exception):
                loop.call(client.close(), timeout=5)
            state["connected"] = False
    finally:
        loop.shutdown()


def _finalize_sync(loop: "_Loop", client: Client, state: dict) -> None:
    """GC/exit fallback for a SyncClient that was never close()d.

    Runs via ``weakref.finalize`` (so also at interpreter exit, before threads
    die). Receives only the internals — never the SyncClient itself, which
    would keep it alive forever.
    """
    _teardown_sync(loop, client, state)


class _Loop:
    def __init__(self):
        self._loop = asyncio.new_event_loop()
        self._shutdown = False
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        asyncio.set_event_loop(self._loop)
        self._loop.run_forever()

    @property
    def is_shutdown(self) -> bool:
        return self._shutdown

    @property
    def owns_current_thread(self) -> bool:
        """Whether the caller is already ON the loop thread — where blocking on
        ``call`` (or joining the thread) would deadlock."""
        return threading.current_thread() is self._thread

    def call(self, coro, timeout: Optional[float] = None) -> Any:
        if self._shutdown:
            # A stopped loop never runs the coroutine, so .result() would
            # hang forever; fail loudly instead.
            coro.close()
            raise RuntimeError("SyncClient is closed; its event loop has shut down")
        return asyncio.run_coroutine_threadsafe(coro, self._loop).result(timeout)

    def shutdown(self) -> None:
        self._shutdown = True
        self._loop.call_soon_threadsafe(self._loop.stop)
        if not self.owns_current_thread:
            self._thread.join(timeout=5)


class _SyncNamespace:
    """Wraps an async namespace, turning each coroutine method into a blocking call.

    Non-coroutine attributes (plain methods, properties) pass through untouched
    instead of being fed to the event loop, so a sync method added to a
    namespace later cannot break the facade.
    """

    def __init__(self, target: Any, call: Callable[[Any], Any]):
        self._target = target
        self._call = call

    def __getattr__(self, name: str):
        attr = getattr(self._target, name)
        if not asyncio.iscoroutinefunction(attr):
            return attr

        def wrapper(*args, **kwargs):
            return self._call(attr(*args, **kwargs))

        wrapper.__name__ = name
        return wrapper


class SyncSnapshot:
    """Blocking facade over a block-pinned :class:`Snapshot`.

    Mirrors the snapshot's read surface: generic accessors, registry reads,
    and the typed namespaces, all resolving at ``block``.
    """

    def __init__(self, snapshot: Snapshot, call: Callable[[Any], Any]):
        self._snapshot = snapshot
        self._call = call
        self.block = snapshot.block
        for ns in _NAMESPACES:
            setattr(self, ns, _SyncNamespace(getattr(snapshot, ns), call))

    def read(self, name: str, **params):
        return self._call(self._snapshot.read(name, **params))

    def reads(self) -> list[dict]:
        return self._snapshot.reads()

    def query(self, item, params: Optional[list] = None):
        return self._call(self._snapshot.query(item, params))

    def query_map(self, item, params: Optional[list] = None):
        return self._call(self._snapshot.query_map(item, params))

    def query_batch(self, item, param_sets: list):
        return self._call(self._snapshot.query_batch(item, param_sets))

    def runtime(self, method, params: list):
        return self._call(self._snapshot.runtime(method, params))

    def constant(self, item):
        return self._call(self._snapshot.constant(item))

    def timestamp(self):
        return self._call(self._snapshot.timestamp())

    def block_time(self) -> float:
        return self._call(self._snapshot.block_time())

    def is_fast_blocks(self) -> bool:
        return self._call(self._snapshot.is_fast_blocks())

    def block_info(self, block: Optional[int] = None):
        return self._call(self._snapshot.block_info(block))

    def at(self, block: Optional[int] = None) -> "SyncSnapshot":
        """This snapshot (already pinned), or a sibling pinned to another block."""
        snapshot = self._call(self._snapshot.at(block))
        return self if snapshot is self._snapshot else SyncSnapshot(snapshot, self._call)

    def balance(self, rao: int, netuid: int = 0):
        return self._snapshot.balance(rao, netuid)

    def __repr__(self) -> str:
        return f"SyncSnapshot(block={self.block})"


class SyncClient:
    # Explicit so type checkers see the same namespace surface as ``Client``
    # (a loop of setattr alone is invisible to them).
    balances: _SyncNamespace
    chain: _SyncNamespace
    delegation: _SyncNamespace
    epochs: _SyncNamespace
    hyperparameters: _SyncNamespace
    identity: _SyncNamespace
    leasing: _SyncNamespace
    locks: _SyncNamespace
    neurons: _SyncNamespace
    prices: _SyncNamespace
    staking: _SyncNamespace
    subnets: _SyncNamespace
    weights: _SyncNamespace

    def __init__(
        self,
        network: str = DEFAULT_NETWORK,
        *,
        policy: Optional[Policy] = None,
        fallback_endpoints: Optional[list[str]] = None,
        archive_endpoints: Optional[list[str]] = None,
        retry_forever: bool = False,
        substrate: Optional[Substrate] = None,
    ):
        self._client = Client(
            network,
            policy=policy,
            fallback_endpoints=fallback_endpoints,
            archive_endpoints=archive_endpoints,
            retry_forever=retry_forever,
            substrate=substrate,
        )
        self.network = self._client.network
        self.endpoint = self._client.endpoint
        # Shared with the GC finalizer, which must not hold the instance itself.
        self._state = {"connected": False}
        # The loop thread is lazy: nothing is spawned until the first blocking
        # call, so construction (and async use by the Subtensor subclass) stays
        # completely cold.
        self._loop_thread: Optional[_Loop] = None
        self._finalizer: Optional[weakref.finalize] = None
        # Keep in sync with namespaces.NAMESPACES / Client (explicit so type
        # checkers see each attribute; a setattr loop is invisible to them).
        self.balances = _SyncNamespace(self._client.balances, self._call)
        self.chain = _SyncNamespace(self._client.chain, self._call)
        self.delegation = _SyncNamespace(self._client.delegation, self._call)
        self.epochs = _SyncNamespace(self._client.epochs, self._call)
        self.hyperparameters = _SyncNamespace(self._client.hyperparameters, self._call)
        self.identity = _SyncNamespace(self._client.identity, self._call)
        self.leasing = _SyncNamespace(self._client.leasing, self._call)
        self.locks = _SyncNamespace(self._client.locks, self._call)
        self.neurons = _SyncNamespace(self._client.neurons, self._call)
        self.prices = _SyncNamespace(self._client.prices, self._call)
        self.staking = _SyncNamespace(self._client.staking, self._call)
        self.subnets = _SyncNamespace(self._client.subnets, self._call)
        self.weights = _SyncNamespace(self._client.weights, self._call)

    # Lifecycle ----------------------------------------------------------------

    def _ensure_loop(self) -> _Loop:
        """The private event loop, started on first blocking use (with the GC
        finalizer that makes ``close()`` optional). Overridden by ``Subtensor``
        to lock the instance into blocking mode."""
        if self._loop_thread is None:
            self._loop_thread = _Loop()
            self._finalizer = weakref.finalize(
                self, _finalize_sync, self._loop_thread, self._client, self._state
            )
        return self._loop_thread

    @property
    def _loop(self) -> _Loop:
        return self._ensure_loop()

    def connect(self) -> "SyncClient":
        """Open the connection. Idempotent; also runs implicitly on first use."""
        if not self._state["connected"]:
            self._loop.call(self._client.connect())
            self._state["connected"] = True
        return self

    def _call(self, coro) -> Any:
        """The facade's single choke point: connect on first use, then block on
        the coroutine in the background loop."""
        try:
            self.connect()
            loop = self._loop
        except BaseException:
            coro.close()  # never ran; avoid the un-awaited coroutine warning
            raise
        return loop.call(coro)

    def close(self) -> None:
        """Deterministic teardown. Optional: garbage collection (or process
        exit) triggers the same cleanup for clients that are never closed."""
        loop = self._loop_thread
        finalizer = self._finalizer
        self._finalizer = None
        try:
            # Teardown first — never detach the finalizer before the loop is
            # stopped, or a failed substrate close leaves a live thread with
            # no cleanup path.
            if loop is not None:
                _teardown_sync(loop, self._client, self._state)
        finally:
            if finalizer is not None:
                finalizer.detach()

    def __enter__(self) -> "SyncClient":
        return self.connect()

    def __exit__(self, *_exc) -> None:
        self.close()

    @property
    def policy(self) -> Optional[Policy]:
        return self._client.policy

    @policy.setter
    def policy(self, value: Optional[Policy]) -> None:
        self._client.policy = value

    @property
    def token_symbols(self) -> dict[int, str]:
        return self._client.token_symbols

    def balance(self, rao: int, netuid: int = 0):
        """A Balance tagged with this connection's token symbol (see ``Client.balance``)."""
        return self._client.balance(rao, netuid)

    @property
    def block(self) -> int:
        """Current chain block number. Reads the chain on every access."""
        return self._call(self._client.block())

    @property
    def time(self) -> datetime:
        """UTC timestamp of the current chain head block. Reads the chain on
        every access; ``timestamp(block=...)`` reads another block's time."""
        return self._call(self._client.timestamp())

    @property
    def spec_version(self) -> int:
        """The connected runtime's ``spec_version``. Reads the chain on access
        (TTL-cached in the transport, so a runtime upgrade shows up within
        about a block)."""
        return self._call(self._client.spec_version())

    def timestamp(self, block=None):
        return self._call(self._client.timestamp(block=block))

    def block_time(self) -> float:
        return self._call(self._client.block_time())

    def is_fast_blocks(self) -> bool:
        return self._call(self._client.is_fast_blocks())

    def block_info(self, block=None):
        return self._call(self._client.block_info(block))

    def wait_for_block(self, block=None, *, timeout=None):
        return self._call(self._client.wait_for_block(block, timeout=timeout))

    def wait_for_timestamp(self, when, *, timeout=None):
        return self._call(self._client.wait_for_timestamp(when, timeout=timeout))

    def wait_for_epoch(self, netuid: int, *, timeout=None):
        return self._call(self._client.wait_for_epoch(netuid, timeout=timeout))

    def query(self, item, params: Optional[list] = None, *, block: Optional[int] = None):
        return self._call(self._client.query(item, params, block=block))

    def query_map(self, item, params: Optional[list] = None, *, block: Optional[int] = None):
        return self._call(self._client.query_map(item, params, block=block))

    def query_batch(self, item, param_sets: list, *, block: Optional[int] = None):
        return self._call(self._client.query_batch(item, param_sets, block=block))

    def runtime(self, method, params: list, *, block: Optional[int] = None):
        return self._call(self._client.runtime(method, params, block=block))

    def constant(self, item):
        return self._call(self._client.constant(item))

    def decode_scale(self, type_string: str, data):
        return self._call(self._client.decode_scale(type_string, data))

    def at(self, block: Optional[int] = None) -> SyncSnapshot:
        """A read-only view pinned to ``block``: blocking namespaces, reads,
        and generic accessors, all resolving at that block."""
        return SyncSnapshot(self._call(self._client.at(block)), self._call)

    def blocks(self, *, finalized: bool = False) -> Iterator[BlockHeader]:
        """Iterate new block headers, blocking between blocks.

        Break out of the loop to cancel the underlying subscription.
        """
        agen = self._client.blocks(finalized=finalized)
        try:
            while True:
                try:
                    yield self._call(agen.__anext__())
                except StopAsyncIteration:
                    return
        finally:
            # Closing the generator needs the loop, not a connection. After
            # close() the loop is gone and there is nothing left to clean up.
            if not self._loop.is_shutdown:
                self._loop.call(agen.aclose())

    def read(self, name: str, **params):
        return self._call(self._client.read(name, **params))

    def reads(self) -> list[dict]:
        return self._client.reads()

    def submit_call(self, call, wallet, **kwargs):
        return self._call(self._client.submit_call(call, wallet, **kwargs))

    def prepare_call(self, call, *, address, **kwargs):
        return self._call(self._client.prepare_call(call, address=address, **kwargs))

    def submit_signature(self, unsigned, signature, **kwargs):
        return self._call(self._client.submit_signature(unsigned, signature, **kwargs))

    def compose(self, call):
        return self._call(self._client.compose(call))

    def estimate_weight(self, call, *, address):
        return self._call(self._client.estimate_weight(call, address=address))

    def multisig(self, signatories, threshold):
        multi = self._call(self._client.multisig(signatories, threshold))
        return _SyncNamespace(multi, self._call)

    def plan(self, intent, wallet, **kwargs):
        return self._call(self._client.plan(intent, wallet, **kwargs))

    def execute(self, intent, wallet, **kwargs):
        return self._call(self._client.execute(intent, wallet, **kwargs))

    def execute_tool(self, op, args, wallet, **kwargs):
        return self._call(self._client.execute_tool(op, args, wallet, **kwargs))

    def submit_shielded(self, intent, wallet, **kwargs):
        return self._call(self._client.submit_shielded(intent, wallet, **kwargs))

    def tools(self) -> list[dict]:
        return self._client.tools()

    def __repr__(self) -> str:
        return f"SyncClient(network={self.network!r}, endpoint={self.endpoint!r})"
