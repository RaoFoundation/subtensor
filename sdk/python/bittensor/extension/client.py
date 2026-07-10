"""WebSocket client for the extension bridge."""

from __future__ import annotations

import asyncio
import json
import uuid
from dataclasses import dataclass
from typing import Any, Optional

from websockets.asyncio.client import connect as ws_connect

from .tokens import read_bridge_token


class BridgeError(RuntimeError):
    """The bridge or extension rejected a request."""


@dataclass(frozen=True)
class ExtensionAccount:
    address: str
    name: str
    source: str
    type: str


class BridgeClient:
    """RPC client for a running :class:`BridgeServer`."""

    def __init__(self, url: str, *, token: Optional[str] = None):
        self.url = url
        self._token = token if token is not None else read_bridge_token()
        self._ws: Optional[Any] = None
        self._lock = asyncio.Lock()

    async def connect(self) -> None:
        if self._ws is not None:
            return
        if not self._token:
            raise BridgeError(
                "extension bridge client token is missing; "
                "start the bridge via btcli --signer extension"
            )
        self._ws = await ws_connect(self.url, open_timeout=5)
        await self._ws.send(json.dumps({"role": "client", "token": self._token}))
        try:
            raw = await asyncio.wait_for(self._ws.recv(), timeout=5)
        except Exception as error:
            await self._ws.close()
            self._ws = None
            raise BridgeError("extension bridge rejected the client token") from error
        hello = json.loads(raw)
        if hello.get("role") != "ok":
            await self._ws.close()
            self._ws = None
            raise BridgeError("extension bridge rejected the client token")

    async def close(self) -> None:
        if self._ws is not None:
            await self._ws.close()
            self._ws = None

    async def __aenter__(self) -> BridgeClient:
        await self.connect()
        return self

    async def __aexit__(self, *args: object) -> None:
        await self.close()

    async def request(self, method: str, params: Optional[dict[str, Any]] = None) -> Any:
        async with self._lock:
            await self.connect()
            assert self._ws is not None
            request_id = uuid.uuid4().hex
            await self._ws.send(
                json.dumps({"id": request_id, "method": method, "params": params or {}})
            )
            while True:
                raw = await asyncio.wait_for(self._ws.recv(), timeout=300)
                message = json.loads(raw)
                if message.get("id") != request_id:
                    continue
                if "error" in message:
                    error = message["error"]
                    text = error.get("message") if isinstance(error, dict) else str(error)
                    raise BridgeError(text or "bridge request failed")
                return message.get("result")

    async def list_accounts(self) -> list[ExtensionAccount]:
        result = await self.request("accounts.list")
        accounts = result.get("accounts", []) if isinstance(result, dict) else []
        out: list[ExtensionAccount] = []
        for entry in accounts:
            if not isinstance(entry, dict):
                continue
            address = entry.get("address")
            if not isinstance(address, str):
                continue
            meta = entry.get("meta") if isinstance(entry.get("meta"), dict) else {}
            out.append(
                ExtensionAccount(
                    address=address,
                    name=str(meta.get("name") or entry.get("name") or address),
                    source=str(meta.get("source") or entry.get("source") or "unknown"),
                    type=str(entry.get("type") or "sr25519"),
                )
            )
        return out

    async def sign_extrinsic_payload(self, payload: dict[str, Any]) -> dict[str, Any]:
        result = await self.request("extrinsic.sign", {"payload": payload})
        if not isinstance(result, dict):
            raise BridgeError("bridge returned an invalid extrinsic signature")
        return result

    async def sign_bytes(self, address: str, data_hex: str) -> dict[str, Any]:
        result = await self.request(
            "bytes.sign",
            {"address": address, "data": data_hex, "type": "bytes"},
        )
        if not isinstance(result, dict):
            raise BridgeError("bridge returned an invalid bytes signature")
        return result

    async def report_transaction_result(self, success: bool) -> None:
        """Tell the bridge page that the submitted transaction has finished."""
        await self.request("transaction.result", {"success": success})
