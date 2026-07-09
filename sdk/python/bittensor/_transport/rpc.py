"""JSON-RPC websocket session: correlation, subscriptions, reconnection.

This module knows nothing about SCALE, metadata, or Substrate semantics beyond
two things: JSON-RPC 2.0 framing (including batches and subscription
notifications), and the node's "state already discarded" error shape, which is
classified here so callers can retry against an archive node.

Design:

- One websocket per session. A single supervisor task owns the connection and
  the receive loop; requests are sent directly (under a lock) by the caller's
  task and awaited on a per-request future — there is no polling loop.
- Every pending request keeps its outgoing frame. When the connection drops,
  the supervisor reconnects and resubmits every pending frame; the caller's
  future simply resolves later. Callers never see a reconnect for plain
  requests.
- Subscriptions cannot be resumed across a reconnect (the server-side id dies
  with the connection), so each open subscription gets the failure pushed into
  its queue instead.
- Endpoint pool: the primary URL first, rotating through ``fallback_urls`` when
  an endpoint fails to connect or exhausts ``max_retries`` response timeouts.
  With ``retry_forever`` the pool is cycled indefinitely, with a growing pause
  between full cycles.
- The websocket connection itself is injectable (``connect_factory``) so tests
  drive the session against an in-process fake server.
"""

from __future__ import annotations

import asyncio
import contextlib
import itertools
import logging
from typing import Any, AsyncIterator, Awaitable, Callable, Optional, Protocol, Union

from websockets.asyncio.client import connect as ws_connect
from websockets.exceptions import ConnectionClosed, ConnectionClosedOK

from .errors import MaxRetriesExceeded, StateDiscardedError, SubstrateRequestException
from .utils import json

logger = logging.getLogger("bittensor.transport")
raw_logger = logging.getLogger("bittensor.transport.raw_websocket")

_STATE_DISCARDED_NEEDLE = "State already discarded for "

# Requests that must never be auto-resubmitted after a reconnect: a duplicate
# submission can hit the pool as "already imported" while the original is
# live, and the resulting error path would clear the nonce cache under a
# still-pending transaction.
_NON_IDEMPOTENT_METHODS = frozenset({"author_submitExtrinsic", "author_submitAndWatchExtrinsic"})

# Cap per-subscription buffering so a slow consumer cannot grow memory without
# bound; on overflow the oldest update is dropped in favor of the newest.
_SUBSCRIPTION_QUEUE_MAXSIZE = 2048


class WsConnection(Protocol):
    """The slice of a websocket connection the session uses (injectable)."""

    async def send(self, message: str) -> None: ...

    async def recv(self) -> Union[str, bytes]: ...

    async def close(self) -> None: ...


ConnectFactory = Callable[[str], Awaitable[WsConnection]]


async def _default_connect(url: str) -> WsConnection:
    return await asyncio.wait_for(ws_connect(url, max_size=2**32, write_limit=2**16), timeout=10.0)


def classify_rpc_error(error: dict) -> SubstrateRequestException:
    """Build the exception for a JSON-RPC error payload."""
    message = str(error.get("message", ""))
    data = error.get("data")
    if isinstance(data, str) and _STATE_DISCARDED_NEEDLE in data:
        return StateDiscardedError(data.split(_STATE_DISCARDED_NEEDLE)[1].strip())
    if _STATE_DISCARDED_NEEDLE in message:
        return StateDiscardedError(message.split(_STATE_DISCARDED_NEEDLE)[1].strip())
    return SubstrateRequestException({"error": error})


class Subscription:
    """An active server-push subscription, consumed with ``async for``."""

    def __init__(self, session: "RpcSession", subscription_id: str, unsubscribe_method: str):
        self.subscription_id = subscription_id
        self._session = session
        self._unsubscribe_method = unsubscribe_method
        self._queue: asyncio.Queue = asyncio.Queue(maxsize=_SUBSCRIPTION_QUEUE_MAXSIZE)
        self._closed = False

    def __aiter__(self) -> AsyncIterator[Any]:
        return self

    async def __anext__(self) -> Any:
        if self._closed:
            raise StopAsyncIteration
        item = await self._queue.get()
        if isinstance(item, Exception):
            self._closed = True
            raise item
        if item is _SUBSCRIPTION_END:
            self._closed = True
            raise StopAsyncIteration
        return item

    async def unsubscribe(self) -> None:
        """Tell the node to stop, and end local iteration."""
        if self._closed:
            return
        self._closed = True
        # Mark the id closed BEFORE the unsubscribe round-trip: a notification
        # already in flight must be dropped, not buffered as an early update.
        self._session._forget_subscription(self.subscription_id)
        with contextlib.suppress(Exception):
            await self._session.request(self._unsubscribe_method, [self.subscription_id])

    def _push(self, item: Any) -> None:
        while True:
            try:
                self._queue.put_nowait(item)
                return
            except asyncio.QueueFull:
                with contextlib.suppress(asyncio.QueueEmpty):
                    dropped = self._queue.get_nowait()
                    logger.warning(
                        f"Subscription {self.subscription_id}: consumer too slow; "
                        f"dropped oldest update {str(dropped)[:100]}"
                    )


_SUBSCRIPTION_END = object()

# Bounds for the transient routing state: how many recently-closed subscription
# ids to remember (late notifications for them are dropped), and how many
# unclaimed early-update buffers to keep before evicting the oldest.
_CLOSED_IDS_REMEMBERED = 256
_MAX_EARLY_BUFFERS = 64


class _Pending:
    __slots__ = ("frame", "future", "sent")

    def __init__(self, future: asyncio.Future, frame: dict):
        self.future = future
        self.frame = frame
        # True once the frame has gone out on the *current* connection. The
        # supervisor's resubmission marks and sends everything itself, so a
        # caller waking up from a reconnect must not send again.
        self.sent = False


class RpcSession:
    def __init__(
        self,
        url: str,
        *,
        fallback_urls: Optional[list[str]] = None,
        retry_forever: bool = False,
        max_retries: int = 5,
        response_timeout: float = 60.0,
        connect_factory: Optional[ConnectFactory] = None,
    ):
        """A JSON-RPC session over one websocket.

        Args:
            url: primary ``ws://`` / ``wss://`` endpoint.
            fallback_urls: same-chain endpoints rotated to on connection failure.
            retry_forever: never give up on connection failures; keep cycling
                the endpoint pool until one answers.
            max_retries: response timeouts tolerated per endpoint before
                rotating to the next one.
            response_timeout: seconds without *any* inbound frame (while
                requests are pending) before the connection is presumed dead
                and reestablished.
            connect_factory: replaces the real websocket dial (for tests).
        """
        self._urls = [url] + [u for u in (fallback_urls or []) if u != url]
        self._url_index = 0
        self._retry_forever = retry_forever
        self._max_retries = max_retries
        self._response_timeout = response_timeout
        self._connect = connect_factory or _default_connect

        self._ws: Optional[WsConnection] = None
        self._supervisor: Optional[asyncio.Task] = None
        self._connected = asyncio.Event()
        self._closing = False
        # Set when the supervisor gives up for good; ``_connected`` is then
        # opened so parked senders wake, observe the error, and fail instead
        # of waiting for a reconnect that will never come.
        self._fatal_error: Optional[Exception] = None
        self._ids = itertools.count(1)
        self._pending: dict[int, _Pending] = {}
        self._subscriptions: dict[str, Subscription] = {}
        # Updates that arrived before their subscribe() call claimed the id.
        self._early: dict[str, _EarlyUpdates] = {}
        # Recently closed subscription ids: late notifications are dropped.
        self._closed_ids: dict[str, None] = {}
        self._send_lock = asyncio.Lock()
        # Consecutive connection failures (timeouts and abnormal closes) with
        # no successfully received frame in between. Reset by any inbound
        # frame; drives endpoint rotation, backoff, and giving up.
        self._consecutive_failures = 0

    @property
    def url(self) -> str:
        """The endpoint currently in use (rotates through the pool on failure)."""
        return self._urls[self._url_index]

    # -- lifecycle -------------------------------------------------------------

    async def connect(self) -> None:
        if self._supervisor is not None and not self._supervisor.done():
            await self._connected.wait()
            return
        self._closing = False
        self._fatal_error = None
        self._connected.clear()
        self._supervisor = asyncio.create_task(self._run(), name="rpc-session")
        # Surface immediate connection failures to the caller instead of
        # parking them on the first request.
        connected = asyncio.create_task(self._connected.wait())
        done, _ = await asyncio.wait(
            {connected, self._supervisor}, return_when=asyncio.FIRST_COMPLETED
        )
        if self._supervisor in done:
            connected.cancel()
            error = self._supervisor.exception()
            if error is not None:
                raise error
            raise SubstrateRequestException("connection closed during connect")

    async def close(self) -> None:
        self._closing = True
        if self._supervisor is not None:
            self._supervisor.cancel()
            # A supervisor that already died with a terminal connection error
            # must not re-raise it out of close() and mask the app's error.
            with contextlib.suppress(asyncio.CancelledError, SubstrateRequestException):
                await self._supervisor
            self._supervisor = None
        if self._ws is not None:
            with contextlib.suppress(Exception):
                await self._ws.close()
            self._ws = None
        shutdown = SubstrateRequestException("RPC session closed")
        for pending in self._pending.values():
            if not pending.future.done():
                pending.future.set_exception(shutdown)
        self._pending.clear()
        # Open the gate so senders parked on ``_connected`` wake and observe
        # ``_closing``; the next connect() clears it again.
        self._connected.set()
        for sub in list(self._subscriptions.values()):
            sub._push(_SUBSCRIPTION_END)
        self._subscriptions.clear()
        self._early.clear()
        self._closed_ids.clear()

    async def __aenter__(self) -> "RpcSession":
        await self.connect()
        return self

    async def __aexit__(self, *_exc) -> None:
        await self.close()

    # -- requests ----------------------------------------------------------------

    async def request(self, method: str, params: Optional[list] = None) -> Any:
        """One JSON-RPC request; returns the ``result`` or raises."""
        frame = {"jsonrpc": "2.0", "id": next(self._ids), "method": method, "params": params or []}
        future = await self._submit(frame)
        response = await future
        return self._unwrap(response)

    async def request_batch(self, requests: list[tuple[str, Optional[list]]]) -> list[Any]:
        """Send several requests as one JSON-RPC 2.0 batch frame.

        Responses are unwrapped in request order; the first error raises.
        """
        if not requests:
            return []
        frames = [
            {"jsonrpc": "2.0", "id": next(self._ids), "method": method, "params": params or []}
            for method, params in requests
        ]
        futures = await self._submit_batch(frames)
        responses = await asyncio.gather(*futures)
        return [self._unwrap(response) for response in responses]

    async def subscribe(
        self, method: str, params: Optional[list], unsubscribe_method: str
    ) -> Subscription:
        """Open a subscription; iterate the returned object for updates.

        Each yielded item is the notification's ``params`` dict (with
        ``result`` and ``subscription`` keys). A connection drop while the
        subscription is open raises out of the iterator — server-side
        subscription state does not survive a reconnect.
        """
        subscription_id = await self.request(method, params)
        if not isinstance(subscription_id, (str, int)):
            raise SubstrateRequestException(f"unexpected subscription id: {subscription_id!r}")
        subscription_id = str(subscription_id)
        subscription = Subscription(self, subscription_id, unsubscribe_method)
        # Updates may have raced the subscribe response into the early buffer.
        early = self._early.pop(subscription_id, None)
        if early is not None:
            for item in early.items:
                subscription._push(item)
        self._subscriptions[subscription_id] = subscription
        return subscription

    def _forget_subscription(self, subscription_id: str) -> None:
        """Stop routing a subscription id; drop (don't buffer) late updates."""
        self._subscriptions.pop(subscription_id, None)
        self._early.pop(subscription_id, None)
        self._closed_ids[subscription_id] = None
        while len(self._closed_ids) > _CLOSED_IDS_REMEMBERED:
            self._closed_ids.pop(next(iter(self._closed_ids)))

    # -- internals -----------------------------------------------------------------

    @staticmethod
    def _unwrap(response: dict) -> Any:
        if "error" in response:
            raise classify_rpc_error(response["error"])
        if "result" not in response:
            raise SubstrateRequestException(response)
        return response["result"]

    async def _submit(self, frame: dict) -> asyncio.Future:
        future = asyncio.get_running_loop().create_future()
        pending = _Pending(future, frame)
        self._pending[frame["id"]] = pending
        try:
            await self._send_pendings([pending])
        except BaseException:
            self._discard_pendings([pending])
            raise
        return future

    async def _submit_batch(self, frames: list[dict]) -> list[asyncio.Future]:
        loop = asyncio.get_running_loop()
        pendings = []
        for frame in frames:
            pending = _Pending(loop.create_future(), frame)
            self._pending[frame["id"]] = pending
            pendings.append(pending)
        try:
            await self._send_pendings(pendings)
        except BaseException:
            self._discard_pendings(pendings)
            raise
        return [pending.future for pending in pendings]

    def _discard_pendings(self, pendings: list[_Pending]) -> None:
        """Drop entries whose submit failed, so a reconnect never resubmits a
        frame nobody is awaiting (the caller sees the submit exception)."""
        for pending in pendings:
            self._pending.pop(pending.frame["id"], None)
            if pending.future.done() and not pending.future.cancelled():
                pending.future.exception()  # consume; the submit error is what propagates

    async def _send_pendings(self, pendings: list[_Pending]) -> None:
        """Send the frames of ``pendings`` that nobody has sent yet.

        If a reconnect happens while this caller waits for the connection, the
        supervisor's resubmission sends the frame and marks it ``sent`` — the
        caller then skips, so a frame is transmitted exactly once per
        connection. A send failure is not fatal: the pending entry keeps its
        frame and the supervisor resubmits after reconnecting.
        """
        if self._supervisor is None or self._supervisor.done():
            await self.connect()
        while True:
            await self._connected.wait()
            if self._closing or self._fatal_error is not None:
                # The supervisor gave up (or close() ran) and opened the gate:
                # fail instead of waiting for a reconnect that will never come.
                error = self._fatal_error or SubstrateRequestException("RPC session closed")
                for pending in pendings:
                    self._pending.pop(pending.frame["id"], None)
                    if not pending.future.done():
                        pending.future.set_exception(error)
                return
            async with self._send_lock:
                if not self._connected.is_set():
                    continue  # connection died between the wait and the lock
                unsent = [p for p in pendings if not p.sent and not p.future.done()]
                if not unsent:
                    return
                for pending in unsent:
                    pending.sent = True
                frame = unsent[0].frame if len(unsent) == 1 else [p.frame for p in unsent]
                text = _dumps(frame)
                if raw_logger.isEnabledFor(logging.DEBUG):
                    raw_logger.debug(f"WEBSOCKET_SEND> {text}")
                try:
                    assert self._ws is not None
                    await self._ws.send(text)
                except Exception as error:  # supervisor will notice and resubmit
                    logger.debug(f"send failed ({error!r}); leaving frame for resubmission")
                return

    def _give_up(self, error: Exception) -> None:
        """Permanent failure: fail everything and wake parked senders."""
        self._fatal_error = error
        self._fail_all(error)
        self._connected.set()

    def _fail_all(self, error: Exception) -> None:
        for pending in self._pending.values():
            if not pending.future.done():
                pending.future.set_exception(error)
        self._pending.clear()
        self._fail_subscriptions(error)

    def _fail_subscriptions(self, error: Exception) -> None:
        # Unclaimed early buffers die with the connection like everything else.
        self._early.clear()
        if not self._subscriptions:
            return
        logger.warning(
            "Connection lost with %d open subscription(s); they cannot be resumed.",
            len(self._subscriptions),
        )
        for sub in self._subscriptions.values():
            sub._push(
                SubstrateRequestException(
                    "Connection lost while waiting for a subscription response "
                    "(e.g. extrinsic finalization). The transaction may already be "
                    "on chain — verify balances before retrying."
                )
            )
        self._subscriptions.clear()

    async def _run(self) -> None:
        """Supervisor: connect, pump frames, reconnect with resubmission."""
        while not self._closing:
            try:
                self._ws = await self._dial()
            except Exception as error:  # terminal: retries exhausted or fatal dial error
                self._give_up(error)
                raise
            # Resubmit before opening the gate, so callers parked on
            # ``_connected`` can never race the resubmission into a double send.
            await self._resubmit_pending()
            self._connected.set()
            reason = await self._pump()
            self._connected.clear()
            with contextlib.suppress(Exception):
                await self._ws.close()
            self._ws = None
            if self._closing or isinstance(reason, ConnectionClosedOK):
                return
            # Any abnormal end: subscriptions die, plain requests survive.
            self._fail_subscriptions(reason)
            # Only *consecutive* failures count — any received frame resets
            # this in _pump — so occasional blips over a long-lived session
            # never accumulate into giving up.
            self._consecutive_failures += 1
            if self._consecutive_failures % self._max_retries == 0:
                self._rotate_url()
            if not self._retry_forever and self._consecutive_failures >= self._max_retries * len(
                self._urls
            ):
                self._give_up(MaxRetriesExceeded("Max retries exceeded."))
                return
            logger.info(f"Connection to {self.url} lost ({reason!r}); reconnecting")
            # Backoff between consecutive failures: keeps a connect-then-drop
            # endpoint from turning into a busy loop.
            await asyncio.sleep(min(0.25 * self._consecutive_failures, 5.0))

    async def _dial(self) -> WsConnection:
        """Open a websocket to the first reachable endpoint in the pool."""
        cycle = 0
        last_error: Optional[Exception] = None
        while True:
            for _ in range(len(self._urls)):
                try:
                    return await self._connect(self.url)
                except Exception as error:
                    last_error = error
                    logger.info(f"Could not connect to {self.url}: {type(error).__name__}: {error}")
                    self._rotate_url()
            cycle += 1
            if not self._retry_forever:
                assert last_error is not None
                if isinstance(last_error, SubstrateRequestException):
                    raise last_error
                raise MaxRetriesExceeded(f"could not connect to any endpoint: {last_error}")
            delay = min(2.0 * cycle, 30.0)
            logger.warning(f"All endpoints unreachable; retrying in {delay:.0f}s")
            await asyncio.sleep(delay)

    def _rotate_url(self) -> None:
        if len(self._urls) == 1:
            return
        self._url_index = (self._url_index + 1) % len(self._urls)
        logger.info(f"Falling back to endpoint {self.url}")

    async def _resubmit_pending(self) -> None:
        """Send every unanswered request's frame on the fresh connection.

        Runs before ``_connected`` opens, so this is the only sender; entries
        are marked ``sent`` so callers waking afterwards don't send again.

        Extrinsic submissions that already went out on a previous connection
        are never resubmitted: the original may already sit in the pool (a
        duplicate errors as "already imported", which would clear the nonce
        cache under a live transaction). Their futures fail with an honest
        "fate unknown" error instead; a submission queued while disconnected
        (never transmitted) is still sent normally.
        """
        if not self._pending:
            return
        for pending in list(self._pending.values()):
            if pending.sent and pending.frame.get("method") in _NON_IDEMPOTENT_METHODS:
                self._pending.pop(pending.frame["id"], None)
                if not pending.future.done():
                    pending.future.set_exception(
                        SubstrateRequestException(
                            "Connection dropped while an extrinsic submission was in "
                            "flight; the transaction may already be in the pool or on "
                            "chain. Verify chain state before retrying."
                        )
                    )
        if not self._pending:
            return
        logger.debug(f"Resubmitting {len(self._pending)} in-flight request(s)")
        for pending in list(self._pending.values()):
            if pending.future.done():
                continue
            pending.sent = True
            try:
                async with self._send_lock:
                    assert self._ws is not None
                    await self._ws.send(_dumps(pending.frame))
            except Exception as error:
                logger.debug(f"resubmission failed ({error!r}); will retry on next connect")
                return

    async def _pump(self) -> Exception:
        """Receive frames until the connection ends; returns the reason."""
        assert self._ws is not None
        ws = self._ws
        while True:
            try:
                raw = await asyncio.wait_for(ws.recv(), timeout=self._response_timeout)
            except asyncio.TimeoutError:
                if not self._pending:
                    # Idle silence is fine — including a legitimately quiet
                    # subscription; only an unanswered request times out.
                    continue
                return TimeoutError(f"no response from {self.url} in {self._response_timeout}s")
            except ConnectionClosedOK as error:
                if self._pending or self._subscriptions:
                    # The server hung up mid-request: treat as abnormal.
                    return ConnectionClosed(error.rcvd, error.sent)
                return error
            except Exception as error:
                return error
            # Any inbound frame proves the connection is alive.
            self._consecutive_failures = 0
            if isinstance(raw, bytes):
                raw = raw.decode()
            if raw_logger.isEnabledFor(logging.DEBUG):
                raw_logger.debug(f"WEBSOCKET_RECEIVE> {raw}")
            try:
                message = json.loads(raw)
            except ValueError:
                logger.warning("Dropping unparseable frame from node")
                continue
            if isinstance(message, list):
                for item in message:
                    self._dispatch(item)
            else:
                self._dispatch(message)

    def _dispatch(self, message: dict) -> None:
        if "id" in message and message["id"] is not None:
            pending = self._pending.pop(message["id"], None)
            if pending is None:
                logger.debug(f"Response for unknown id {message['id']} (late reply?); dropped")
                return
            if not pending.future.done():
                pending.future.set_result(message)
            return
        params = message.get("params")
        if isinstance(params, dict) and "subscription" in params:
            sub_id = str(params["subscription"])
            target = self._subscriptions.get(sub_id)
            if target is not None:
                target._push(params)
                return
            if sub_id in self._closed_ids:
                return  # late notification for an unsubscribed id
            # Update raced ahead of the subscribe response: buffer it (bounded,
            # so an unclaimed id can never leak indefinitely).
            buffer = self._early.get(sub_id)
            if buffer is None:
                while len(self._early) >= _MAX_EARLY_BUFFERS:
                    self._early.pop(next(iter(self._early)))
                buffer = _EarlyUpdates()
                self._early[sub_id] = buffer
            buffer.push(params)
            return
        logger.debug(f"Unroutable frame dropped: {str(message)[:200]}")


class _EarlyUpdates:
    """Buffer for subscription updates that arrive before ``subscribe`` returns."""

    def __init__(self):
        self.items: list = []

    def push(self, item: Any) -> None:
        self.items.append(item)


def _dumps(frame: Any) -> str:
    """Serialize a frame to text — some JSON accelerators return bytes."""
    text = json.dumps(frame)
    if isinstance(text, (bytes, bytearray)):
        return text.decode()
    return text
