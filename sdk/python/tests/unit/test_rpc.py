"""Unit tests for the RPC session: correlation, batches, subscriptions,
reconnect-with-resubmission, fallback rotation, retry_forever, and error
classification — all against the in-process fake node."""

from __future__ import annotations

import asyncio

import pytest

from bittensor._transport.errors import (
    MaxRetriesExceeded,
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
        with pytest.raises(MaxRetriesExceeded):
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
