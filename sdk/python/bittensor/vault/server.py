"""Ephemeral local page server for one Polkadot Vault QR round-trip.

Same shape as the extension bridge (``extension/bridge.py``) but simpler in
every dimension: it lives for a single CLI invocation, binds an ephemeral
loopback port, serves one static page, and routes exactly one request —
either "show these QR frames and give me back the signature the page
scanned" (signing) or "just scan whatever QR the phone shows" (address
import).

Messages to the page:  ``{"type": "sign-request", ...}``,
``{"type": "scan-request", ...}``, ``{"type": "result", ...}``
Messages from the page: ``{"type": "signature", "signature": "0x..."}``,
``{"type": "scanned", "text": ...}``, ``{"type": "error", "message": ...}``
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import uuid
from pathlib import Path
from typing import Any, Optional

from websockets.asyncio.server import serve as ws_serve

from ..extension.browser import open_bridge_page
from ..result import BittensorError

_STATIC_DIR = Path(__file__).resolve().parent / "static"
_VAULT_HTML = _STATIC_DIR / "vault.html"
# The one-time add-network QR (signed by the Opentensor Foundation verifier),
# served so the page can offer it when Vault reports an unknown network.
_SPECS_PNG = _STATIC_DIR / "bittensor-chain-specs.png"

logger = logging.getLogger(__name__)


class VaultPageError(BittensorError):
    """The vault page reported a failure (camera denied, user cancelled, ...)."""


class VaultSessionServer:
    """Serve the vault page on 127.0.0.1 and collect one scanned signature."""

    def __init__(self, host: str = "127.0.0.1", port: int = 0):
        self.host = host
        self.port = port  # 0 = ephemeral; the real port is known after start()
        self.session_id = uuid.uuid4().hex
        self._server: Optional[Any] = None
        self._page_ws: Optional[Any] = None
        self._page_connected = asyncio.Event()
        # Resolves with the page's answer: a signature hex (sign-request) or
        # the raw decoded QR text (scan-request).
        self._result: Optional[asyncio.Future[str]] = None
        self._pending_request: Optional[dict[str, Any]] = None

    @property
    def http_url(self) -> str:
        return f"http://{self.host}:{self.port}/?session={self.session_id}"

    async def start(self, *, open_browser: bool = True, browser: Optional[str] = None) -> None:
        self._server = await ws_serve(
            self._handle_connection,
            self.host,
            self.port,
            process_request=self._process_request,
        )
        self.port = self._server.sockets[0].getsockname()[1]
        logger.debug(f"vault page server listening on {self.http_url}")
        if open_browser:
            open_bridge_page(self.http_url, browser=browser)

    async def stop(self) -> None:
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()
            self._server = None

    async def request_signature(
        self,
        *,
        frames: list[str],
        frames_hex: list[str],
        summary: dict[str, Any],
        connect_timeout: float = 120.0,
        sign_timeout: float = 900.0,
    ) -> str:
        """Push the QR frames to the page and wait for the scanned signature.

        ``frames`` are SVG data URIs (cycled by the page when there are
        several); ``frames_hex`` carries the same UOS frames as raw hex for
        programmatic consumers (simulated phones in tests, debugging);
        ``summary`` is display-only transaction context.
        """
        return await self._request(
            {
                "type": "sign-request",
                "frames": frames,
                "framesHex": frames_hex,
                "summary": summary,
            },
            connect_timeout=connect_timeout,
            answer_timeout=sign_timeout,
            timeout_message="timed out waiting for the Vault signature scan; the "
            "transaction's era has likely expired — run the command again",
        )

    async def request_scan(
        self,
        *,
        prompt: str,
        connect_timeout: float = 120.0,
        scan_timeout: float = 600.0,
    ) -> str:
        """Ask the page to scan one QR from the phone and return its text.

        ``prompt`` tells the user what to show the camera (e.g. which Vault
        screen holds the address QR).
        """
        return await self._request(
            {"type": "scan-request", "prompt": prompt},
            connect_timeout=connect_timeout,
            answer_timeout=scan_timeout,
            timeout_message="timed out waiting for a QR scan — run the command again",
        )

    async def warm_up(self, *, connect_timeout: float = 120.0) -> None:
        """Get the page ready before a time-critical flow (MEV-shielded
        signing): wait for it to connect and start its camera, so scanning
        can begin the moment the first QR appears."""
        try:
            await asyncio.wait_for(self._page_connected.wait(), timeout=connect_timeout)
        except asyncio.TimeoutError:
            raise VaultPageError(
                f"the vault page never connected; open {self.http_url} manually"
            ) from None
        if self._page_ws is not None:
            with contextlib.suppress(Exception):
                await self._page_ws.send(json.dumps({"type": "warm-up"}))

    async def _request(
        self,
        request: dict[str, Any],
        *,
        connect_timeout: float,
        answer_timeout: float,
        timeout_message: str,
    ) -> str:
        # One future per request: sequential requests (MEV shielding signs an
        # inner and an outer extrinsic) each get a fresh answer slot.
        self._result = asyncio.get_running_loop().create_future()
        self._pending_request = request
        if self._page_ws is not None:
            await self._page_ws.send(json.dumps(request))
        try:
            await asyncio.wait_for(self._page_connected.wait(), timeout=connect_timeout)
        except asyncio.TimeoutError:
            raise VaultPageError(
                f"the vault page never connected; open {self.http_url} manually"
            ) from None
        try:
            return await asyncio.wait_for(asyncio.shield(self._result), timeout=answer_timeout)
        except asyncio.TimeoutError:
            raise VaultPageError(timeout_message) from None

    async def report_result(self, success: bool) -> None:
        """Tell the page how the submission went (best effort)."""
        if self._page_ws is not None:
            with contextlib.suppress(Exception):
                await self._page_ws.send(json.dumps({"type": "result", "success": success}))

    async def _process_request(self, connection: Any, request: Any) -> Optional[Any]:
        path = request.path.split("?", 1)[0]
        if path == "/ws":
            return None
        if path in ("/", "/index.html"):
            body = _VAULT_HTML.read_text(encoding="utf-8")
            response = connection.respond(200, body)
            response.headers["Content-Type"] = "text/html; charset=utf-8"
            return response
        if path == "/bittensor-chain-specs.png":
            response = connection.respond(200, "")
            response.body = _SPECS_PNG.read_bytes()
            response.headers["Content-Type"] = "image/png"
            del response.headers["Content-Length"]
            response.headers["Content-Length"] = str(len(response.body))
            return response
        return connection.respond(404, "Not Found")

    async def _handle_connection(self, websocket: Any) -> None:
        try:
            hello = json.loads(await asyncio.wait_for(websocket.recv(), timeout=10))
        except Exception:
            return
        if hello.get("role") != "vault" or hello.get("session") != self.session_id:
            await websocket.close(code=1008, reason="stale vault session")
            return

        previous = self._page_ws
        self._page_ws = websocket
        if previous is not None:
            await previous.close(code=1012, reason="replaced by new vault tab")
        # (Re)send the pending request so a reloaded tab picks up where it was.
        if (
            self._pending_request is not None
            and self._result is not None
            and not self._result.done()
        ):
            await websocket.send(json.dumps(self._pending_request))
        self._page_connected.set()

        try:
            async for raw in websocket:
                try:
                    message = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                kind = message.get("type")
                result = self._result
                if result is None or result.done():
                    continue
                if kind in ("signature", "scanned"):
                    answer = message.get("signature" if kind == "signature" else "text")
                    if isinstance(answer, str) and answer:
                        result.set_result(answer)
                elif kind == "error":
                    result.set_exception(
                        VaultPageError(str(message.get("message") or "vault page error"))
                    )
        finally:
            if self._page_ws is websocket:
                self._page_ws = None
                self._page_connected.clear()
