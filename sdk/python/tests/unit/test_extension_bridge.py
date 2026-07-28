"""Unit tests for the extension bridge client auth token."""

from __future__ import annotations

import asyncio
import json

import pytest

from bittensor.extension.bridge import BridgeServer
from bittensor.extension.client import BridgeClient, BridgeError
from bittensor.extension.tokens import write_bridge_token


@pytest.mark.asyncio
async def test_bridge_rejects_client_without_token() -> None:
    server = BridgeServer(host="127.0.0.1", port=0)
    await server.start(open_browser=False)
    try:
        host, port = server._ws_server.sockets[0].getsockname()[:2]
        url = f"ws://{host}:{port}/ws"
        client = BridgeClient(url, token="")
        with pytest.raises(BridgeError, match="token is missing"):
            await client.connect()
    finally:
        await server.stop()


@pytest.mark.asyncio
async def test_bridge_rejects_client_with_wrong_token() -> None:
    server = BridgeServer(host="127.0.0.1", port=0)
    await server.start(open_browser=False)
    try:
        host, port = server._ws_server.sockets[0].getsockname()[:2]
        url = f"ws://{host}:{port}/ws"
        client = BridgeClient(url, token="not-the-server-token")
        with pytest.raises(BridgeError, match="rejected the client token"):
            await client.connect()
    finally:
        await server.stop()


@pytest.mark.asyncio
async def test_bridge_accepts_client_with_matching_token(tmp_path, monkeypatch) -> None:
    token_path = tmp_path / "extension_bridge.token"
    monkeypatch.setenv("BITTENSOR_EXTENSION_BRIDGE_TOKEN", str(token_path))

    server = BridgeServer(host="127.0.0.1", port=0)
    await server.start(open_browser=False)
    try:
        host, port = server._ws_server.sockets[0].getsockname()[:2]
        url = f"ws://{host}:{port}/ws"
        write_bridge_token(server.state.client_token)
        async with BridgeClient(url) as client:
            status = await client.request("bridge.status")
        assert isinstance(status, dict)
        assert status.get("session_id") == server.state.session_id
    finally:
        await server.stop()


@pytest.mark.asyncio
async def test_bridge_browser_requires_matching_session() -> None:
    server = BridgeServer(host="127.0.0.1", port=0)
    await server.start(open_browser=False)
    try:
        host, port = server._ws_server.sockets[0].getsockname()[:2]
        url = f"ws://{host}:{port}/ws"

        import websockets

        async with websockets.connect(url) as ws:
            await ws.send(json.dumps({"role": "bridge", "session": "stale-session-id"}))
            with pytest.raises(websockets.exceptions.ConnectionClosedError):
                await asyncio.wait_for(ws.recv(), timeout=2)
    finally:
        await server.stop()


@pytest.mark.asyncio
@pytest.mark.parametrize("success", [True, False])
async def test_client_reports_transaction_result(monkeypatch, success: bool) -> None:
    client = BridgeClient("ws://127.0.0.1:1/ws", token="test-token")
    requests = []

    async def fake_request(method, params=None):
        requests.append((method, params))
        return {"acknowledged": True}

    monkeypatch.setattr(client, "request", fake_request)

    await client.report_transaction_result(success)

    assert requests == [("transaction.result", {"success": success})]
