"""In-process fake JSON-RPC websocket node for transport unit tests.

``FakeNode`` speaks the same frames as a Substrate node's JSON-RPC endpoint:
single requests, batches, and subscription notifications. Its ``connection()``
factory plugs into ``RpcSession(connect_factory=...)``, so the session runs its
real send/receive/reconnect machinery against scripted behavior — no network,
no mocks of the session itself.

Failure injection:
- ``node.drop_connection()`` — abruptly closes the current connection.
- ``node.refuse_next(n)`` — makes the next n connection attempts fail.
- ``node.silence()`` — stops answering (for response-timeout tests).
- per-method handlers can be replaced at any time via ``node.handlers``.
"""

from __future__ import annotations

import asyncio
import itertools
import json
from typing import Any, Callable, Optional

from websockets.exceptions import ConnectionClosed, ConnectionClosedOK
from websockets.frames import Close


def _closed_error(code: int = 1006, reason: str = "abnormal closure") -> ConnectionClosed:
    close = Close(code, reason)
    if code == 1000:
        return ConnectionClosedOK(close, None)
    return ConnectionClosed(close, None)


class FakeConnection:
    def __init__(self, node: "FakeNode"):
        self._node = node
        self._inbox: asyncio.Queue = asyncio.Queue()  # frames the client will recv
        self.closed = False

    async def send(self, message: str) -> None:
        if self.closed:
            raise _closed_error()
        await self._node._on_client_frame(self, message)

    async def recv(self) -> str:
        if self.closed and self._inbox.empty():
            raise _closed_error()
        item = await self._inbox.get()
        if isinstance(item, Exception):
            raise item
        return item

    async def close(self) -> None:
        self.closed = True
        self._inbox.put_nowait(_closed_error(1000, "client close"))

    # -- node-side helpers ---------------------------------------------------

    def push(self, frame: dict | list) -> None:
        self._inbox.put_nowait(json.dumps(frame))

    def kill(self, code: int = 1006) -> None:
        """Simulate the server dropping the connection."""
        self.closed = True
        self._inbox.put_nowait(_closed_error(code))


Handler = Callable[["FakeNode", dict], Any]


class FakeNode:
    def __init__(self):
        self.handlers: dict[str, Handler] = {}
        self.requests: list[dict] = []  # every request frame seen, in order
        self.connections: list[FakeConnection] = []
        self.connect_attempts = 0
        self._refuse = 0
        self._silent = False
        self._sub_ids = itertools.count(1)

    # -- connect factory -------------------------------------------------------

    async def connection(self, url: str) -> FakeConnection:
        self.connect_attempts += 1
        if self._refuse > 0:
            self._refuse -= 1
            raise ConnectionRefusedError(f"refused connection to {url}")
        conn = FakeConnection(self)
        self.connections.append(conn)
        return conn

    @property
    def current(self) -> FakeConnection:
        return self.connections[-1]

    # -- failure injection -------------------------------------------------------

    def drop_connection(self) -> None:
        self.current.kill()

    def refuse_next(self, n: int) -> None:
        self._refuse = n

    def silence(self, silent: bool = True) -> None:
        self._silent = silent

    # -- subscriptions -------------------------------------------------------------

    def new_subscription_id(self) -> str:
        return f"sub{next(self._sub_ids)}"

    def push_subscription_update(self, subscription_id: str, result: Any) -> None:
        self.current.push(
            {
                "jsonrpc": "2.0",
                "method": "subscription",
                "params": {"subscription": subscription_id, "result": result},
            }
        )

    # -- request handling ------------------------------------------------------------

    async def _on_client_frame(self, conn: FakeConnection, message: str) -> None:
        frame = json.loads(message)
        if self._silent:
            if isinstance(frame, list):
                self.requests.extend(frame)
            else:
                self.requests.append(frame)
            return
        if isinstance(frame, list):
            responses = [self._respond(item) for item in frame]
            conn.push([r for r in responses if r is not None])
        else:
            response = self._respond(frame)
            if response is not None:
                conn.push(response)

    def _respond(self, request: dict) -> Optional[dict]:
        self.requests.append(request)
        handler = self.handlers.get(request["method"])
        if handler is None:
            return {
                "jsonrpc": "2.0",
                "id": request["id"],
                "error": {"code": -32601, "message": f"Method not found: {request['method']}"},
            }
        outcome = handler(self, request)
        if outcome is None:
            return None  # handler chose not to answer (or pushed frames itself)
        if isinstance(outcome, dict) and ("result" in outcome or "error" in outcome):
            return {"jsonrpc": "2.0", "id": request["id"], **outcome}
        return {"jsonrpc": "2.0", "id": request["id"], "result": outcome}
