"""Tests for the controlled public ``client.substrate`` RPC surface."""

from __future__ import annotations

import asyncio
import inspect
from typing import Any, cast

import pytest

from bittensor import Client, ReadOnlySubstrate, Subtensor
from bittensor._substrate import RpcSubstrate
from bittensor._transport import SubstrateConnection
from bittensor._transport.errors import StateDiscardedError, SubstrateRequestException
from bittensor.result import BittensorError, ChainError, ConnectionNotReady
from tests.harness.fake_node import FakeNode
from tests.harness.fake_substrate import FakeSubstrate


class _StubConnection:
    def __init__(self, *, result: Any = None, error: Exception | None = None):
        self.result = result
        self.error = error
        self.requests: list[tuple[str, list | None]] = []

    async def rpc_request(self, method: str, params: list | None = None) -> Any:
        self.requests.append((method, params))
        if self.error is not None:
            raise self.error
        return self.result


class _BackendWithoutRpc(FakeSubstrate):
    """A complete injected backend without the optional low-level capability."""

    rpc_request = None


def _public_names(value: object) -> set[str]:
    return {name for name in dir(value) if not name.startswith("_")}


async def test_async_rpc_forwards_method_params_and_result():
    backend = FakeSubstrate()
    backend.seed_rpc("state_getStorageAt", lambda params: {"params": params})
    client = Client("local", substrate=backend)

    result = await client.substrate.rpc_request("state_getStorageAt", ["0xkey", "0xblock"])

    assert result == {"params": ["0xkey", "0xblock"]}
    assert backend.rpc_requests == [
        ("state_getStorageAt", ["0xkey", "0xblock"]),
    ]


def test_blocking_subtensor_connects_lazily_and_returns_value():
    backend = FakeSubstrate()
    backend.seed_rpc("system_health", {"isSyncing": False, "peers": 4})
    client = Subtensor("local", substrate=backend)
    try:
        assert backend.connected is False

        result = client.substrate.rpc_request("system_health")

        assert result == {"isSyncing": False, "peers": 4}
        assert not inspect.isawaitable(result)
        assert backend.connected is True
        assert backend.rpc_requests == [("system_health", None)]
        assert _public_names(client.substrate) == {"rpc_request"}
    finally:
        client.close()


@pytest.mark.parametrize(
    "method",
    [
        "author_submitExtrinsic",
        "chain_subscribeNewHeads",
        "chain_unsubscribeNewHeads",
        "custom_getThing",
    ],
)
async def test_unsafe_subscription_and_unknown_methods_are_rejected_before_dispatch(method):
    backend = FakeSubstrate()
    client = Client("local", substrate=backend)

    with pytest.raises(BittensorError, match="not an approved read-only method"):
        await client.substrate.rpc_request(method, [])

    assert backend.rpc_requests == []


async def test_injected_backend_with_rpc_capability_is_supported():
    backend = FakeSubstrate()
    backend.seed_rpc("account_nextIndex", 9)
    client = Client("local", substrate=backend)

    assert await client.substrate.rpc_request("account_nextIndex", ["5F..."]) == 9


async def test_injected_backend_without_rpc_capability_fails_only_when_used():
    backend = _BackendWithoutRpc()
    client = Client("local", substrate=backend)

    assert await client.block() == 100

    with pytest.raises(BittensorError, match=r"does not support.*read-only RPC capability"):
        await client.substrate.rpc_request("system_health")


async def test_public_view_does_not_expose_transport_or_submission_helpers():
    client = Client("local", substrate=FakeSubstrate())

    assert isinstance(client.substrate, ReadOnlySubstrate)
    assert _public_names(client.substrate) == {"rpc_request"}


async def test_rpc_requires_an_open_connection():
    client = Client("local", substrate=RpcSubstrate("ws://unused"))

    with pytest.raises(ConnectionNotReady, match="connection is not open"):
        await client.substrate.rpc_request("system_health")


async def test_rpc_normalizes_node_errors_to_chain_error():
    backend = RpcSubstrate("ws://unused")
    raw = _StubConnection(error=SubstrateRequestException("node rejected the request"))
    backend._substrate = cast(Any, raw)
    client = Client("local", substrate=backend)

    with pytest.raises(ChainError, match="node rejected the request"):
        await client.substrate.rpc_request("state_getStorage", ["0xkey"])


async def test_rpc_retries_discarded_state_on_archive_backend():
    backend = RpcSubstrate("ws://primary", archive_endpoints=["wss://archive"])
    primary = _StubConnection(error=StateDiscardedError("0xold"))
    archive = _StubConnection(result="0xvalue")
    backend._substrate = cast(Any, primary)
    backend._archive_substrate = cast(Any, archive)
    client = Client("local", substrate=backend)

    result = await client.substrate.rpc_request("state_getStorage", ["0xkey", "0xold"])

    assert result == "0xvalue"
    assert primary.requests == [("state_getStorage", ["0xkey", "0xold"])]
    assert archive.requests == [("state_getStorage", ["0xkey", "0xold"])]


async def test_public_rpc_uses_reconnecting_transport():
    node = FakeNode()
    calls = 0

    def flaky_health(fake: FakeNode, request):
        nonlocal calls
        calls += 1
        if calls == 1:
            fake.current.kill()
            return None
        return {"isSyncing": False, "peers": 3}

    node.handlers["system_health"] = flaky_health
    raw = SubstrateConnection(
        "ws://fake:1",
        connect_factory=node.connection,
        max_retries=2,
        response_timeout=0.5,
    )
    await raw._session.connect()
    backend = RpcSubstrate("ws://unused")
    backend._substrate = raw
    client = Client("local", substrate=backend)
    try:
        result = await asyncio.wait_for(
            client.substrate.rpc_request("system_health"),
            timeout=3,
        )
        assert result == {"isSyncing": False, "peers": 3}
        assert calls == 2
        assert len(node.connections) == 2
    finally:
        await backend.close()
