"""Local HTTP + WebSocket bridge between Python and browser extensions."""

from __future__ import annotations

import asyncio
import json
import logging
import secrets
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

from websockets.asyncio.server import serve as ws_serve

from .browser import open_bridge_page

DEFAULT_BRIDGE_HOST = "127.0.0.1"
DEFAULT_BRIDGE_PORT = 39295

_STATIC_DIR = Path(__file__).resolve().parent / "static"
_BRIDGE_HTML = _STATIC_DIR / "bridge.html"

logger = logging.getLogger(__name__)


@dataclass
class _BridgeState:
    browser_ws: Any = None
    pending: dict[str, asyncio.Future] = field(default_factory=dict)
    session_id: str = field(default_factory=lambda: uuid.uuid4().hex)
    # Known only to the spawning CLI (written to ~/.bittensor/extension_bridge.token).
    client_token: str = field(default_factory=lambda: secrets.token_urlsafe(32))


class BridgeServer:
    """Serve the bridge page and route extension RPC between browser and Python."""

    def __init__(self, host: str = DEFAULT_BRIDGE_HOST, port: int = DEFAULT_BRIDGE_PORT):
        self.host = host
        self.port = port
        self.state = _BridgeState()
        self._ws_server: Optional[Any] = None

    @property
    def http_url(self) -> str:
        return f"http://{self.host}:{self.port}/"

    @property
    def ws_url(self) -> str:
        return f"ws://{self.host}:{self.port}/ws"

    async def start(self, *, open_browser: bool = False, browser: Optional[str] = None) -> None:
        if self._ws_server is None:
            self._ws_server = await ws_serve(
                self._handle_connection,
                self.host,
                self.port,
                process_request=self._process_request,
            )
            logger.debug(f"Bridge server listening on {self.http_url}")
        if open_browser:
            open_bridge_page(f"{self.http_url}?session={self.state.session_id}", browser=browser)

    async def stop(self) -> None:
        if self._ws_server is not None:
            self._ws_server.close()
            await self._ws_server.wait_closed()
            self._ws_server = None
            logger.debug("Bridge server stopped")

    async def wait_until_ready(self, timeout: float = 30.0) -> None:
        deadline = asyncio.get_running_loop().time() + timeout
        while asyncio.get_running_loop().time() < deadline:
            if self.state.browser_ws is not None:
                return
            await asyncio.sleep(0.1)
        raise TimeoutError(
            "extension bridge page is not connected; open "
            f"{self.http_url} and authorize your extension"
        )

    async def _process_request(self, connection: Any, request: Any) -> Optional[Any]:
        path = request.path.split("?", 1)[0]
        if path == "/ws":
            return None
        if path in ("/", "/index.html"):
            body = _BRIDGE_HTML.read_text(encoding="utf-8")
            response = connection.respond(200, body)
            response.headers["Content-Type"] = "text/html; charset=utf-8"
            return response
        return connection.respond(404, "Not Found")

    async def _handle_connection(self, websocket: Any) -> None:
        role: Optional[str] = None
        try:
            first = await asyncio.wait_for(websocket.recv(), timeout=10)
            hello = json.loads(first)
            role = hello.get("role")
        except Exception:
            return

        if role == "bridge":
            await self._handle_browser(websocket, hello)
        elif role == "client":
            token = hello.get("token")
            if not isinstance(token, str) or not secrets.compare_digest(
                token, self.state.client_token
            ):
                await websocket.close(code=1008, reason="unauthorized client")
                return
            await websocket.send(json.dumps({"role": "ok"}))
            await self._handle_client(websocket)
        else:
            await websocket.close(code=1008, reason="unknown role")

    async def _handle_browser(self, websocket: Any, hello: dict[str, Any]) -> None:
        session = hello.get("session")
        if session != self.state.session_id:
            await websocket.close(code=1008, reason="stale bridge session")
            return

        previous = self.state.browser_ws
        self.state.browser_ws = websocket
        logger.debug("Browser bridge page connected")
        if previous is not None:
            await previous.close(code=1012, reason="replaced by new bridge tab")
        try:
            async for raw in websocket:
                message = json.loads(raw)
                request_id = message.get("id")
                future = self.state.pending.pop(request_id, None) if request_id else None
                if future is not None and not future.done():
                    if "error" in message:
                        error = message["error"]
                        text = error.get("message") if isinstance(error, dict) else str(error)
                        future.set_exception(RuntimeError(text or "bridge error"))
                    else:
                        future.set_result(message.get("result"))
        finally:
            if self.state.browser_ws is websocket:
                self.state.browser_ws = None
                logger.debug("Browser bridge page disconnected")
            for request_id, future in list(self.state.pending.items()):
                if not future.done():
                    future.set_exception(RuntimeError("bridge page disconnected"))
                self.state.pending.pop(request_id, None)

    async def _handle_client(self, websocket: Any) -> None:
        async for raw in websocket:
            request = json.loads(raw)
            request_id = request.get("id")
            method = request.get("method")
            if not request_id or not method:
                continue
            if method == "bridge.status":
                await websocket.send(
                    json.dumps(
                        {
                            "id": request_id,
                            "result": await self._bridge_status(),
                        }
                    )
                )
                continue
            if self.state.browser_ws is None:
                await websocket.send(
                    json.dumps(
                        {
                            "id": request_id,
                            "error": {
                                "message": (
                                    "extension bridge page is not connected; "
                                    "a browser tab should open automatically"
                                )
                            },
                        }
                    )
                )
                continue

            future: asyncio.Future = asyncio.get_running_loop().create_future()
            self.state.pending[request_id] = future
            await self.state.browser_ws.send(raw)
            try:
                result = await asyncio.wait_for(future, timeout=300)
            except Exception as error:
                await websocket.send(
                    json.dumps({"id": request_id, "error": {"message": str(error)}})
                )
                continue
            await websocket.send(json.dumps({"id": request_id, "result": result}))

    async def _bridge_status(self) -> dict[str, Any]:
        browser_connected = self.state.browser_ws is not None
        account_count = 0
        if browser_connected:
            try:
                result = await self._forward_to_browser(
                    {
                        "id": f"status-accounts-{uuid.uuid4().hex}",
                        "method": "accounts.list",
                        "params": {},
                    }
                )
                accounts = result.get("accounts", []) if isinstance(result, dict) else []
                account_count = len(accounts)
            except Exception:
                account_count = 0
        return {
            "browser_connected": browser_connected,
            "account_count": account_count,
            "http_url": self.http_url,
            "session_id": self.state.session_id,
        }

    async def _forward_to_browser(self, request: dict[str, Any]) -> Any:
        if self.state.browser_ws is None:
            raise RuntimeError("bridge page is not connected")
        request_id = request["id"]
        future: asyncio.Future = asyncio.get_running_loop().create_future()
        self.state.pending[request_id] = future
        await self.state.browser_ws.send(json.dumps(request))
        result = await asyncio.wait_for(future, timeout=30)
        return result


async def run_bridge(
    *,
    host: str = DEFAULT_BRIDGE_HOST,
    port: int = DEFAULT_BRIDGE_PORT,
    open_browser: bool = True,
    browser: Optional[str] = None,
) -> BridgeServer:
    server = BridgeServer(host=host, port=port)
    await server.start(open_browser=open_browser, browser=browser)
    return server
