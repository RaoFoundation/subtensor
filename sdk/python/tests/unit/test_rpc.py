"""Unit tests for the RPC session: correlation, batches, subscriptions,
reconnect-with-resubmission, fallback rotation, retry_forever, and error
classification — all against the in-process fake node."""

from __future__ import annotations

import asyncio

import pytest
from websockets.datastructures import Headers
from websockets.exceptions import ConnectionClosedError, InvalidStatus
from websockets.frames import Close
from websockets.http11 import Response

from bittensor import RpcConnectionError
from bittensor import RpcPolicyError as PublicRpcPolicyError
from bittensor._substrate import RpcSubstrate
from bittensor._transport import rpc
from bittensor._transport.errors import (
    MaxRetriesExceeded,
    RpcPolicyRejection,
    StateDiscardedError,
    SubstrateRequestException,
)
from bittensor._transport.rpc import RpcSession
from tests.harness.fake_node import FakeNode

pytestmark = pytest.mark.asyncio


def make_session(node: FakeNode, **kwargs) -> RpcSession:
    kwargs.setdefault("response_timeout", 0.5)
    return RpcSession("ws://fake:1", connect_factory=node.connection, **kwargs)


async def test_request_response_roundtrip():
    node = FakeNode()
    node.handlers["system_chain"] = lambda n, req: "Bittensor"
    async with make_session(node) as session:
        assert await session.request("system_chain", []) == "Bittensor"


async def test_concurrent_requests_correlate_by_id():
    node = FakeNode()
    node.handlers["echo"] = lambda n, req: req["params"][0]
    async with make_session(node) as session:
        results = await asyncio.gather(*(session.request("echo", [i]) for i in range(50)))
        assert results == list(range(50))


async def test_batch_preserves_order_and_demuxes():
    node = FakeNode()
    node.handlers["echo"] = lambda n, req: req["params"][0]
    async with make_session(node) as session:
        results = await session.request_batch([("echo", [i]) for i in range(10)])
        assert results == list(range(10))
        # One frame carried the whole batch: all 10 requests, 1 connection.
        assert len(node.requests) == 10
        assert len(node.connections) == 1


async def test_rpc_error_raises_substrate_request_exception():
    node = FakeNode()
    node.handlers["bad"] = lambda n, req: {"error": {"code": 1, "message": "boom"}}
    async with make_session(node) as session:
        with pytest.raises(SubstrateRequestException, match="boom"):
            await session.request("bad", [])


async def test_policy_rpc_error_preserves_server_details():
    node = FakeNode()
    node.handlers["heavy"] = lambda n, req: {
        "error": {
            "code": -32004,
            "message": "request rate exceeded",
            "data": {"policy": "public-rpc", "reason": "weighted_rate", "retry_after": 30},
        }
    }
    async with make_session(node) as session:
        with pytest.raises(RpcPolicyRejection) as excinfo:
            await session.request("heavy", [])
    assert "request rate exceeded" in str(excinfo.value)
    assert excinfo.value.policy == "public-rpc"
    assert excinfo.value.reason == "weighted_rate"
    assert excinfo.value.retry_after == "30"


async def test_websocket_rate_limit_preserves_handshake_response(monkeypatch):
    async def reject(*args, **kwargs):
        raise InvalidStatus(
            Response(
                429,
                "Too Many Requests",
                Headers(
                    {
                        "X-RateLimit-Policy": "ws-upgrade",
                        "X-RateLimit-Reason": "request-rate",
                        "Retry-After": "60",
                    }
                ),
                b'{"error":"Too many requests from this source."}',
            )
        )

    monkeypatch.setattr(rpc, "ws_connect", reject)
    with pytest.raises(RpcPolicyRejection) as excinfo:
        await rpc._default_connect("wss://rpc.example")
    assert "HTTP 429" in str(excinfo.value)
    assert "Too many requests from this source" in str(excinfo.value)
    assert excinfo.value.policy == "ws-upgrade"
    assert excinfo.value.reason == "request-rate"
    assert excinfo.value.retry_after == "60"


async def test_default_websocket_connect_returns_connection(monkeypatch):
    connection = object()

    async def connect(*args, **kwargs):
        return connection

    monkeypatch.setattr(rpc, "ws_connect", connect)
    assert await rpc._default_connect("wss://rpc.example") is connection


async def test_duplicate_policy_headers_remain_a_terminal_refusal(monkeypatch):
    async def reject(*args, **kwargs):
        raise InvalidStatus(
            Response(
                429,
                "Too Many Requests",
                Headers(
                    [
                        ("X-RateLimit-Policy", "ws-upgrade"),
                        ("X-RateLimit-Policy", "duplicate-value"),
                    ]
                ),
                b"",
            )
        )

    monkeypatch.setattr(rpc, "ws_connect", reject)
    with pytest.raises(RpcPolicyRejection) as excinfo:
        await rpc._default_connect("wss://rpc.example")
    assert excinfo.value.policy == "ws-upgrade"


async def test_websocket_access_policy_preserves_handshake_response(monkeypatch):
    async def reject(*args, **kwargs):
        raise InvalidStatus(
            Response(
                403,
                "Forbidden",
                Headers({"X-RateLimit-Reason": "blocked-source"}),
                b'{"error":"This source is blocked."}',
            )
        )

    monkeypatch.setattr(rpc, "ws_connect", reject)
    with pytest.raises(RpcPolicyRejection) as excinfo:
        await rpc._default_connect("wss://rpc.example")
    assert "access policy (HTTP 403)" in str(excinfo.value)
    assert "This source is blocked" in str(excinfo.value)
    assert excinfo.value.reason == "blocked-source"


async def test_non_policy_websocket_rejection_preserves_status_and_body(monkeypatch):
    async def reject(*args, **kwargs):
        raise InvalidStatus(
            Response(502, "Bad Gateway", Headers(), b"upstream temporarily unavailable")
        )

    monkeypatch.setattr(rpc, "ws_connect", reject)
    with pytest.raises(SubstrateRequestException) as excinfo:
        await rpc._default_connect("wss://rpc.example")
    assert "HTTP 502" in str(excinfo.value)
    assert "upstream temporarily unavailable" in str(excinfo.value)


async def test_websocket_handshake_timeout_has_a_message(monkeypatch):
    async def time_out(*args, **kwargs):
        raise asyncio.TimeoutError

    monkeypatch.setattr(rpc, "ws_connect", time_out)
    with pytest.raises(TimeoutError, match="no response during the WebSocket handshake"):
        await rpc._default_connect("wss://rpc.example")


async def test_websocket_handshake_timeout_redacts_endpoint_secrets(monkeypatch):
    async def time_out(*args, **kwargs):
        raise asyncio.TimeoutError

    monkeypatch.setattr(rpc, "ws_connect", time_out)
    with pytest.raises(TimeoutError) as excinfo:
        await rpc._default_connect("wss://user:password@rpc.example/private?api_key=secret")
    message = str(excinfo.value)
    assert "wss://rpc.example" in message
    for secret in ("user", "password", "private", "api_key", "secret"):
        assert secret not in message


@pytest.mark.parametrize(
    ("body", "expected"),
    [
        (b"", ""),
        (b"temporarily unavailable", "temporarily unavailable"),
        (b'"busy"', "busy"),
        (b'{"message":"try later"}', "try later"),
        (b'{"other":"detail"}', "{'other': 'detail'}"),
    ],
)
async def test_response_detail_shapes(body, expected):
    assert rpc._response_detail(body) == expected


async def test_handshake_diagnostics_strip_terminal_controls(monkeypatch):
    async def reject(*args, **kwargs):
        raise InvalidStatus(
            Response(
                429,
                "Too Many Requests",
                Headers({"X-RateLimit-Policy": "public\x1b[2J-\u202erpc"}),
                b'{"error":"slow down\\u001b]8;;https://evil.example\\u0007\\u202eclick"}',
            )
        )

    monkeypatch.setattr(rpc, "ws_connect", reject)
    with pytest.raises(RpcPolicyRejection) as excinfo:
        await rpc._default_connect("wss://rpc.example")
    assert "\x1b" not in str(excinfo.value)
    assert "\x07" not in str(excinfo.value)
    assert "\u202e" not in str(excinfo.value)
    assert "\x1b" not in (excinfo.value.policy or "")
    assert "\u202e" not in (excinfo.value.policy or "")


async def test_load_shed_policy_supports_camel_case_retry_metadata():
    error = rpc.classify_rpc_error(
        {
            "code": -32005,
            "message": "upstream capacity exceeded",
            "data": {"retryAfter": 15},
        }
    )
    assert isinstance(error, RpcPolicyRejection)
    assert error.retry_after == "15"
    assert error.policy is None


@pytest.mark.parametrize("code", [-32004, -32005])
async def test_provider_defined_code_without_policy_contract_is_not_terminal(code):
    error = rpc.classify_rpc_error(
        {
            "code": code,
            "message": "provider-specific server error",
            "data": {"detail": "not a traffic policy"},
        }
    )
    assert type(error) is SubstrateRequestException


async def test_known_storage_work_policy_without_metadata_remains_terminal():
    error = rpc.classify_rpc_error(
        {
            "code": -32004,
            "message": "Storage work rate limit exceeded",
        }
    )
    assert isinstance(error, RpcPolicyRejection)


async def test_json_rpc_policy_diagnostics_strip_terminal_controls():
    error = rpc.classify_rpc_error(
        {
            "code": -32004,
            "message": "slow\x1b[2Jdown",
            "data": {
                "policy": "public\x1b]8;;https://evil.example\x07rpc",
                "reason": "weighted\x00rate",
            },
        }
    )
    assert isinstance(error, RpcPolicyRejection)
    assert "\x1b" not in str(error)
    assert "\x1b" not in (error.policy or "")
    assert "\x00" not in (error.reason or "")


async def test_policy_refusal_is_not_retried_even_with_retry_forever():
    attempts = 0

    async def reject(url):
        nonlocal attempts
        attempts += 1
        raise RpcPolicyRejection("RPC endpoint rate limited this source (HTTP 429)")

    session = RpcSession(
        "wss://primary.example",
        fallback_urls=["wss://fallback.example"],
        retry_forever=True,
        connect_factory=reject,
    )
    with pytest.raises(RpcPolicyRejection):
        await session.connect()
    assert attempts == 1


async def test_policy_close_is_sanitized_and_not_retried():
    attempts = 0

    class PolicyClosedConnection:
        def __init__(self):
            self.sent = asyncio.Event()

        async def send(self, message):
            self.sent.set()

        async def recv(self):
            await self.sent.wait()
            raise ConnectionClosedError(Close(1008, "slow\x1b[2J\u202edown"), None)

        async def close(self):
            pass

    async def connect(url):
        nonlocal attempts
        attempts += 1
        return PolicyClosedConnection()

    session = RpcSession(
        "wss://rpc.example",
        retry_forever=True,
        connect_factory=connect,
    )
    async with session:
        with pytest.raises(RpcPolicyRejection) as excinfo:
            await session.request("system_chain", [])
    assert "WebSocket 1008" in str(excinfo.value)
    assert "\x1b" not in str(excinfo.value)
    assert "\u202e" not in str(excinfo.value)
    assert attempts == 1


async def test_rpc_substrate_exposes_public_policy_error(monkeypatch):
    class RefusedConnection:
        closed = False

        async def initialize(self):
            raise RpcPolicyRejection(
                "RPC endpoint rate limited this source (HTTP 429)",
                retry_after="60",
            )

        async def close(self):
            self.closed = True

    connection = RefusedConnection()
    substrate = RpcSubstrate("wss://rpc.example", fallback_endpoints=[])
    monkeypatch.setattr(substrate, "_interface", lambda *args: connection)
    with pytest.raises(PublicRpcPolicyError) as excinfo:
        await substrate.connect()
    assert excinfo.value.retry_after == "60"
    assert connection.closed is True


async def test_rpc_substrate_exposes_public_blank_timeout_error(monkeypatch):
    class TimedOutConnection:
        async def initialize(self):
            raise TimeoutError

    substrate = RpcSubstrate("wss://rpc.example", fallback_endpoints=[])
    monkeypatch.setattr(substrate, "_interface", lambda *args: TimedOutConnection())
    with pytest.raises(RpcConnectionError, match="the connection timed out without a response"):
        await substrate.connect()


async def test_rpc_substrate_stream_preserves_public_policy_metadata():
    class RefusedStream:
        async def subscribe_heads(self, **kwargs):
            if False:
                yield None
            raise RpcPolicyRejection("stream rate exceeded", retry_after="12")

    substrate = RpcSubstrate("wss://rpc.example", fallback_endpoints=[])
    substrate._substrate = RefusedStream()
    with pytest.raises(PublicRpcPolicyError) as excinfo:
        await anext(substrate.blocks())
    assert excinfo.value.retry_after == "12"


async def test_rpc_substrate_write_helper_preserves_public_policy_metadata():
    class RefusedWrite:
        async def resolve_extrinsic(self, *args):
            raise RpcPolicyRejection("write rate exceeded", retry_after="9")

    substrate = RpcSubstrate("wss://rpc.example", fallback_endpoints=[])
    substrate._substrate = RefusedWrite()
    with pytest.raises(PublicRpcPolicyError) as excinfo:
        await substrate.find_extrinsic("0x01", "0x02")
    assert excinfo.value.retry_after == "9"


async def test_rpc_substrate_submit_preserves_policy_metadata_and_clears_nonce():
    class RefusedSubmit:
        def __init__(self):
            self.cleared = []

        async def submit_extrinsic(self, *args, **kwargs):
            raise RpcPolicyRejection("submit rate exceeded", retry_after="7")

        def clear_nonce_cache_for_account(self, address):
            self.cleared.append(address)

    raw = RefusedSubmit()
    substrate = RpcSubstrate("wss://rpc.example", fallback_endpoints=[])
    substrate._substrate = raw
    with pytest.raises(PublicRpcPolicyError) as excinfo:
        await substrate._submit_and_report(
            object(),
            signer_address="5Signer",
            wait_for_inclusion=True,
            wait_for_finalization=True,
        )
    assert excinfo.value.retry_after == "7"
    assert raw.cleared == ["5Signer"]


async def test_rpc_substrate_submit_timeout_is_a_public_connection_error():
    class TimedOutSubmit:
        def __init__(self):
            self.cleared = []

        async def submit_extrinsic(self, *args, **kwargs):
            raise MaxRetriesExceeded("no response from wss://rpc.example in 60s")

        def clear_nonce_cache_for_account(self, address):
            self.cleared.append(address)

    raw = TimedOutSubmit()
    substrate = RpcSubstrate("wss://rpc.example", fallback_endpoints=[])
    substrate._substrate = raw
    with pytest.raises(RpcConnectionError, match=r"no response from wss://rpc\.example in 60s"):
        await substrate._submit_and_report(
            object(),
            signer_address="5Signer",
            wait_for_inclusion=True,
            wait_for_finalization=True,
        )
    assert raw.cleared == ["5Signer"]


async def test_state_discarded_classified():
    node = FakeNode()
    node.handlers["state_getStorage"] = lambda n, req: {
        "error": {
            "code": 4003,
            "message": "Client error: Api called for an unknown Block: "
            "State already discarded for 0xabc123",
        }
    }
    async with make_session(node) as session:
        with pytest.raises(StateDiscardedError) as excinfo:
            await session.request("state_getStorage", ["0x"])
        assert excinfo.value.block_hash == "0xabc123"


async def test_subscription_streams_updates():
    node = FakeNode()

    def subscribe(n: FakeNode, req):
        sub_id = n.new_subscription_id()
        # Push two updates right behind the subscribe response.
        asyncio.get_running_loop().call_soon(n.push_subscription_update, sub_id, {"number": 1})
        asyncio.get_running_loop().call_soon(n.push_subscription_update, sub_id, {"number": 2})
        return sub_id

    node.handlers["chain_subscribeNewHeads"] = subscribe
    node.handlers["chain_unsubscribeNewHeads"] = lambda n, req: True
    async with make_session(node) as session:
        sub = await session.subscribe("chain_subscribeNewHeads", [], "chain_unsubscribeNewHeads")
        first = await anext(aiter(sub))
        second = await anext(aiter(sub))
        assert [first["result"], second["result"]] == [{"number": 1}, {"number": 2}]
        await sub.unsubscribe()
        assert any(r["method"] == "chain_unsubscribeNewHeads" for r in node.requests)


async def test_subscription_update_racing_ahead_of_response_is_buffered():
    node = FakeNode()

    def subscribe(n: FakeNode, req):
        # Deliver the update BEFORE the subscribe response.
        n.current.push(
            {
                "jsonrpc": "2.0",
                "method": "subscription",
                "params": {"subscription": "sub-early", "result": {"number": 7}},
            }
        )
        return "sub-early"

    node.handlers["chain_subscribeNewHeads"] = subscribe
    async with make_session(node) as session:
        sub = await session.subscribe("chain_subscribeNewHeads", [], "chain_unsubscribeNewHeads")
        update = await asyncio.wait_for(anext(aiter(sub)), timeout=1)
        assert update["result"] == {"number": 7}


async def test_reconnect_resubmits_pending_request():
    node = FakeNode()
    answered = asyncio.Event()

    def slow_echo(n: FakeNode, req):
        # First attempt: swallow the request (no answer) — the connection will
        # be killed. Second attempt (after resubmission): answer.
        if not answered.is_set():
            answered.set()
            return None
        return req["params"][0]

    node.handlers["echo"] = slow_echo
    async with make_session(node) as session:
        task = asyncio.create_task(session.request("echo", ["hello"]))
        await answered.wait()
        node.drop_connection()
        assert await asyncio.wait_for(task, timeout=2) == "hello"
        assert len(node.connections) == 2  # reconnected once


async def test_reconnect_fails_open_subscriptions():
    node = FakeNode()
    node.handlers["chain_subscribeNewHeads"] = lambda n, req: n.new_subscription_id()
    async with make_session(node) as session:
        sub = await session.subscribe("chain_subscribeNewHeads", [], "chain_unsubscribeNewHeads")
        node.drop_connection()
        with pytest.raises(SubstrateRequestException, match="may already be on chain"):
            await asyncio.wait_for(anext(aiter(sub)), timeout=2)


async def test_connection_refused_rotates_to_fallback():
    node = FakeNode()
    node.handlers["ping"] = lambda n, req: "pong"
    node.refuse_next(1)  # primary fails once; fallback answers
    session = RpcSession(
        "ws://primary:1",
        fallback_urls=["ws://fallback:1"],
        connect_factory=node.connection,
        response_timeout=0.5,
    )
    async with session:
        assert await session.request("ping", []) == "pong"
        assert session.url == "ws://fallback:1"
    assert node.connect_attempts == 2


async def test_all_endpoints_down_raises_without_retry_forever():
    node = FakeNode()
    node.refuse_next(100)
    session = RpcSession(
        "ws://primary:1",
        fallback_urls=["ws://fallback:1"],
        connect_factory=node.connection,
        response_timeout=0.5,
    )
    with pytest.raises(MaxRetriesExceeded):
        await session.connect()
    assert node.connect_attempts == 2  # each endpoint tried once


async def test_retry_forever_keeps_cycling_until_a_connect_lands():
    node = FakeNode()
    node.handlers["ping"] = lambda n, req: "pong"
    node.refuse_next(3)  # 1.5 cycles of a 2-endpoint pool
    session = RpcSession(
        "ws://primary:1",
        fallback_urls=["ws://fallback:1"],
        connect_factory=node.connection,
        retry_forever=True,
        response_timeout=0.5,
    )
    async with session:
        assert await session.request("ping", []) == "pong"
    assert node.connect_attempts == 4


async def test_response_timeout_triggers_reconnect_and_resubmission():
    node = FakeNode()
    calls = {"n": 0}

    def echo(n: FakeNode, req):
        calls["n"] += 1
        if calls["n"] == 1:
            n.silence()  # swallow this and go quiet: forces a response timeout
            return None
        return req["params"][0]

    node.handlers["echo"] = echo

    async def reconnect_unsilences(url):
        conn = await node.connection(url)
        if node.connect_attempts > 1:
            node.silence(False)
        return conn

    session = RpcSession("ws://fake:1", connect_factory=reconnect_unsilences, response_timeout=0.2)
    async with session:
        result = await asyncio.wait_for(session.request("echo", ["back"]), timeout=5)
        assert result == "back"
    assert node.connect_attempts >= 2


async def test_timeouts_exhaust_into_max_retries_exceeded():
    node = FakeNode()
    node.handlers["echo"] = lambda n, req: None  # never answers
    session = RpcSession(
        "ws://fake:1",
        connect_factory=node.connection,
        response_timeout=0.05,
        max_retries=2,
    )
    async with session:
        with pytest.raises(MaxRetriesExceeded, match="no response from ws://fake:1"):
            await asyncio.wait_for(session.request("echo", ["x"]), timeout=5)


async def test_no_double_send_when_request_races_a_reconnect():
    """A frame submitted while the connection is down must be sent exactly once
    (by the supervisor's resubmission), not once by resubmission and once by
    the caller waking from the reconnect."""
    node = FakeNode()
    node.handlers["echo"] = lambda n, req: req["params"][0]
    async with make_session(node) as session:
        node.drop_connection()
        # Submit while the supervisor is mid-reconnect.
        result = await asyncio.wait_for(session.request("echo", ["once"]), timeout=2)
        assert result == "once"
        echo_frames = [r for r in node.requests if r["method"] == "echo"]
        assert len(echo_frames) == 1, f"frame sent {len(echo_frames)} times"


async def test_late_notification_after_unsubscribe_is_dropped():
    """A notification racing an unsubscribe must be discarded, not buffered as
    an early update that poisons the idle check forever."""
    node = FakeNode()
    node.handlers["chain_subscribeNewHeads"] = lambda n, req: n.new_subscription_id()
    node.handlers["chain_unsubscribeNewHeads"] = lambda n, req: True
    async with make_session(node) as session:
        sub = await session.subscribe("chain_subscribeNewHeads", [], "chain_unsubscribeNewHeads")
        await sub.unsubscribe()
        node.push_subscription_update(sub.subscription_id, {"number": 99})
        await asyncio.sleep(0.05)
        assert session._early == {}
        assert session._subscriptions == {}


async def test_failure_counter_resets_on_healthy_traffic():
    """Only *consecutive* failures count: a long-lived session that answers
    between timeouts never accumulates into MaxRetriesExceeded."""
    node = FakeNode()
    calls = {"n": 0}

    def flaky_echo(n: FakeNode, req):
        calls["n"] += 1
        if calls["n"] % 2 == 1:
            n.current.kill()  # every other request: drop the connection
            return None
        return req["params"][0]

    node.handlers["echo"] = flaky_echo
    session = RpcSession(
        "ws://fake:1", connect_factory=node.connection, response_timeout=0.5, max_retries=2
    )
    async with session:
        # 6 failures total (> max_retries * len(urls) = 2), interleaved with
        # successes — must keep working because the counter resets on traffic.
        for _ in range(6):
            assert await asyncio.wait_for(session.request("echo", ["ok"]), timeout=3) == "ok"


async def test_close_fails_pending_requests():
    node = FakeNode()
    node.handlers["echo"] = lambda n, req: None  # never answers
    session = make_session(node)
    await session.connect()
    task = asyncio.create_task(session.request("echo", ["x"]))
    await asyncio.sleep(0.05)
    await session.close()
    with pytest.raises(SubstrateRequestException, match="closed"):
        await task
