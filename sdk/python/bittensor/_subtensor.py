"""The classic entry point: one class, sync or async depending on how you use it.

    import bittensor as bt

    # Inline, blocking (connects lazily; no close() needed)
    sub = bt.Subtensor()                    # defaults to finney
    print(sub.block)                        # current block number
    print(sub.time)                         # UTC time of the head block
    print(sub.spec_version)                 # runtime spec_version
    print(sub.balances.balance(coldkey_ss58="5F..."))

    # Inline, async — awaiting the instance connects and yields the async client
    sub = await bt.Subtensor()
    print(await sub.balances.balance(coldkey_ss58="5F..."))

    # Scoped forms
    with bt.Subtensor("test") as client: ...
    async with bt.Subtensor("test") as client: ...

The mode is declared by syntax, never guessed from the environment: a bare
instance is the blocking client (backed by a private event loop on a daemon
thread, torn down automatically when the object is garbage collected), while
``await`` — on the instance or via ``async with`` — yields the async
:class:`~bittensor.Client` running on the caller's own loop.
"""

from __future__ import annotations

import atexit
import contextlib
import threading
from typing import Generator, Optional, Union

from .client import Client
from .intents.weights import SetWeights
from .result import BittensorError, ExtrinsicResult
from .settings import DEFAULT_NETWORK, resolve_endpoint
from .signing import WalletLike, as_wallet
from .sync import SyncClient
from .wallet import Wallet


class Subtensor(SyncClient):
    """Chain access for both worlds: blocking when used directly, async when awaited.

    A :class:`SyncClient` (every blocking method and namespace is inherited,
    real, and typed) that also implements the async protocols: awaiting the
    instance — or entering ``async with`` — connects the underlying async
    :class:`Client` on the caller's own loop and returns it.

    Construction is cold — no thread, loop, or socket exists until first use —
    so building one is free and the same instance can be handed to either kind
    of code. The first blocking call locks the instance into blocking mode;
    the first ``await`` locks it into async mode. Attribute *inspection* has
    no side effects (safe to repr, probe, or autocomplete a cold instance) —
    except the chain-reading properties ``block``, ``time``, and
    ``spec_version``, whose *evaluation* is a blocking read.

    Blocking mode needs no ``close()``: the connection opens on first call and
    is cleaned up when the instance is garbage collected (or at process exit).
    Async mode follows normal asyncio hygiene — prefer ``async with``, or call
    ``await client.close()`` on the awaited client for deterministic teardown.
    """

    _mode: Optional[str] = None  # None (cold) | "blocking" | "async"

    # Blocking surface: inherited from SyncClient. The only hook is the loop
    # bootstrap, which every blocking path funnels through.

    def _ensure_loop(self):
        if self._mode == "async":
            raise BittensorError(
                "This Subtensor was awaited (async mode); call methods on the "
                "awaited client, or create a fresh bt.Subtensor() for blocking use."
            )
        self._mode = "blocking"
        return super()._ensure_loop()

    def close(self) -> None:
        if self._mode == "async":
            raise BittensorError(
                "This Subtensor is async; close the awaited client with `await client.close()`."
            )
        if self._mode == "blocking":
            super().close()
        # cold: nothing to tear down

    # Async surface ------------------------------------------------------------

    async def _aconnect(self) -> Client:
        if self._mode == "blocking":
            raise BittensorError(
                "This Subtensor is already in blocking mode; create a fresh "
                "bt.Subtensor() to use it with await / async with."
            )
        self._mode = "async"
        if not self._state["connected"]:
            await self._client.connect()
            self._state["connected"] = True
        return self._client

    def __await__(self) -> Generator[object, None, Client]:
        return self._aconnect().__await__()

    async def __aenter__(self) -> Client:
        return await self._aconnect()

    async def __aexit__(self, *_exc) -> None:
        if self._mode == "async" and self._state["connected"]:
            await self._client.close()
            self._state["connected"] = False

    def __repr__(self) -> str:
        return (
            f"Subtensor(network={self.network!r}, endpoint={self.endpoint!r}, "
            f"mode={self._mode or 'cold'})"
        )


# Shared blocking clients for the module-level convenience functions: one per
# canonical endpoint, connected on first use. Prefer a caller-owned Subtensor/
# SyncClient for long-lived processes; this registry exists so a validator loop
# calling set_weights() every tempo reuses one connection instead of redialing.
# Closed explicitly via close_shared_clients() or at process exit.
_shared_lock = threading.Lock()
_shared_clients: dict[str, SyncClient] = {}


def _shared_client(network: str) -> SyncClient:
    # Canonicalize so "finney" and its default wss URL share one entry.
    _, endpoint = resolve_endpoint(network)
    with _shared_lock:
        client = _shared_clients.get(endpoint)
        if client is None:
            client = SyncClient(network)
            _shared_clients[endpoint] = client
        return client


def close_shared_clients() -> None:
    """Close every client cached for ``set_weights`` (and clear the registry)."""
    with _shared_lock:
        clients = list(_shared_clients.values())
        _shared_clients.clear()
    for client in clients:
        with contextlib.suppress(Exception):
            client.close()


atexit.register(close_shared_clients)


def _resolve_wallet(wallet: Union[WalletLike, None], hotkey: Optional[str]) -> WalletLike:
    if wallet is None or isinstance(wallet, str):
        name = wallet or "default"
        # Wallet itself rejects a slash-name combined with an explicit hotkey.
        return Wallet(name, hotkey) if hotkey is not None else as_wallet(name)
    if hotkey is not None:
        raise BittensorError(
            "hotkey= only combines with a wallet *name*; a wallet object already "
            "carries its hotkey."
        )
    return wallet


def set_weights(
    netuid: int,
    weights,
    *,
    uids: Optional[list[int]] = None,
    wallet: Union[WalletLike, str, None] = None,
    hotkey: Optional[str] = None,
    mechid: int = 0,
    version_key: int = 0,
    network: str = DEFAULT_NETWORK,
    retries: int = 2,
) -> ExtrinsicResult:
    """Set validator weights in one call, with everything handled.

        import bittensor as bt

        bt.set_weights(1, {0: 0.1, 1: 0.7, 2: 0.2}, wallet="my_coldkey", hotkey="my_hotkey")

    Blocking. Connects (once — the connection is shared across calls and
    networks are cached), conforms the weights to the subnet's hyperparameters
    (clip, normalize, u16-quantize), picks plaintext or timelocked commit-reveal
    automatically, preflights registration and the rate limit, signs with the
    hotkey, retries transient pool rejections, and **raises** ``ChainError`` on
    failure — so a bare call in a loop cannot fail silently. Returns the
    :class:`ExtrinsicResult` on success.

    ``weights`` is a ``{uid: weight}`` mapping (preferred; values are relative,
    only proportions matter) or a list parallel to ``uids=``. ``wallet`` is a
    wallet name (with ``hotkey=`` for the hotkey name), a ``Wallet``, or omitted
    for the default wallet. Full control (plan preview, policies, proxies,
    async) lives on the client: ``client.execute(bt.SetWeights(...), wallet)``.
    """
    intent = SetWeights(
        netuid=netuid, uids=uids, weights=weights, mechid=mechid, version_key=version_key
    )
    result = _shared_client(network).execute(
        intent, _resolve_wallet(wallet, hotkey), retries=retries
    )
    return result.raise_for_failure()
