"""The classic entry point: one class, sync or async depending on how you use it.

    import bittensor as bt

    # Inline, blocking (connects lazily; no close() needed)
    sub = bt.Subtensor()                    # defaults to finney
    print(sub.block())
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

import threading
from typing import Any, Optional, Union

from .client import Client
from .intents.weights import SetWeights
from .result import BittensorError, ExtrinsicResult
from .settings import DEFAULT_NETWORK, resolve_endpoint
from .signing import WalletLike
from .sync import SyncClient
from .wallet import Wallet


class Subtensor:
    """Chain access for both worlds: blocking when used directly, async when awaited.

    Construction is cold — no thread, loop, or socket exists until first use —
    so building one is free and the same instance can be handed to either kind
    of code. Accepts everything :class:`Client` accepts (``policy``,
    ``fallback_endpoints``, ``archive_endpoints``, ``retry_forever``,
    ``substrate``).

    Blocking mode needs no ``close()``: the connection opens on first call and
    is cleaned up when the instance is garbage collected (or at process exit).
    Async mode follows normal asyncio hygiene — prefer ``async with``, or call
    ``await client.close()`` on the awaited client for deterministic teardown.
    """

    def __init__(
        self,
        network: str = DEFAULT_NETWORK,
        *,
        policy=None,
        fallback_endpoints: Optional[list[str]] = None,
        archive_endpoints: Optional[list[str]] = None,
        retry_forever: bool = False,
        substrate=None,
    ):
        self.network, self.endpoint = resolve_endpoint(network)
        self._network_arg = network
        self._options = dict(
            policy=policy,
            fallback_endpoints=fallback_endpoints,
            archive_endpoints=archive_endpoints,
            retry_forever=retry_forever,
            substrate=substrate,
        )
        self._sync: Optional[SyncClient] = None
        self._async: Optional[Client] = None

    # Blocking surface ---------------------------------------------------------

    def _sync_client(self) -> SyncClient:
        if self._async is not None:
            raise BittensorError(
                "This Subtensor was awaited (async mode); call methods on the "
                "awaited client, or create a fresh bt.Subtensor() for blocking use."
            )
        if self._sync is None:
            self._sync = SyncClient(self._network_arg, **self._options)
        return self._sync

    def __getattr__(self, name: str) -> Any:
        # Everything not defined here is the blocking client's surface
        # (namespaces, reads, execute/plan, ...), created on first touch.
        return getattr(self._sync_client(), name)

    def __enter__(self) -> SyncClient:
        return self._sync_client().connect()

    def __exit__(self, *_exc) -> None:
        if self._sync is not None:
            self._sync.close()
            self._sync = None

    # Async surface ------------------------------------------------------------

    async def _aconnect(self) -> Client:
        if self._sync is not None:
            raise BittensorError(
                "This Subtensor is already in blocking mode; create a fresh "
                "bt.Subtensor() to use it with await / async with."
            )
        if self._async is None:
            client = Client(self._network_arg, **self._options)
            await client.connect()
            self._async = client
        return self._async

    def __await__(self):
        return self._aconnect().__await__()

    async def __aenter__(self) -> Client:
        return await self._aconnect()

    async def __aexit__(self, *_exc) -> None:
        if self._async is not None:
            await self._async.close()
            self._async = None

    def __repr__(self) -> str:
        mode = "async" if self._async else ("blocking" if self._sync else "cold")
        return f"Subtensor(network={self.network!r}, endpoint={self.endpoint!r}, mode={mode})"


# Shared blocking clients for the module-level convenience functions: one per
# network, connected on first use, cleaned up by SyncClient's GC finalizer at
# process exit. A validator loop calling set_weights() every tempo reuses one
# connection instead of redialing.
_shared_lock = threading.Lock()
_shared_clients: dict[str, SyncClient] = {}


def _shared_client(network: str) -> SyncClient:
    with _shared_lock:
        client = _shared_clients.get(network)
        if client is None:
            client = SyncClient(network)
            _shared_clients[network] = client
        return client


def _resolve_wallet(wallet: Union[WalletLike, str, None], hotkey: Optional[str]) -> WalletLike:
    if wallet is None or isinstance(wallet, str):
        return Wallet(name=wallet or "default", hotkey=hotkey or "default")
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
