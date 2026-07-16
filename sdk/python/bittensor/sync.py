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
from typing import Any, Callable, Iterator, Optional

from ._substrate import Substrate
from .client import BlockHeader, Client
from .intents import Policy
from .namespaces import NAMESPACES
from .settings import DEFAULT_NETWORK
from .snapshot import Snapshot

_NAMESPACES = tuple(NAMESPACES)


def _finalize_sync(loop: "_Loop", client: Client, state: dict) -> None:
    """GC/exit fallback for a SyncClient that was never close()d: close the
    connection on the private loop, then stop the loop thread. Runs via
    ``weakref.finalize`` (so also at interpreter exit, before threads die).
    Receives only the internals — never the SyncClient itself, which would
    keep it alive forever."""
    if loop.is_shutdown:
        return
    if state.get("connected"):
        with contextlib.suppress(Exception):
            loop.call(client.close())
    loop.shutdown()


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

    def call(self, coro) -> Any:
        if self._shutdown:
            # A stopped loop never runs the coroutine, so .result() would
            # hang forever; fail loudly instead.
            coro.close()
            raise RuntimeError("SyncClient is closed; its event loop has shut down")
        return asyncio.run_coroutine_threadsafe(coro, self._loop).result()

    def shutdown(self) -> None:
        self._shutdown = True
        self._loop.call_soon_threadsafe(self._loop.stop)
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
        self._loop = _Loop()
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
        self._finalizer = weakref.finalize(
            self, _finalize_sync, self._loop, self._client, self._state
        )
        for ns in _NAMESPACES:
            setattr(self, ns, _SyncNamespace(getattr(self._client, ns), self._call))

    # Lifecycle ----------------------------------------------------------------

    def connect(self) -> "SyncClient":
        """Open the connection. Idempotent; also runs implicitly on first use."""
        if not self._state["connected"]:
            self._loop.call(self._client.connect())
            self._state["connected"] = True
        return self

    def _call(self, coro) -> Any:
        """The facade's single choke point: connect on first use, then block on
        the coroutine in the background loop."""
        self.connect()
        return self._loop.call(coro)

    def close(self) -> None:
        """Deterministic teardown. Optional: garbage collection (or process
        exit) triggers the same cleanup for clients that are never closed."""
        self._finalizer.detach()
        if self._state["connected"]:
            self._loop.call(self._client.close())
            self._state["connected"] = False
        self._loop.shutdown()

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

    def block(self) -> int:
        return self._call(self._client.block())

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
