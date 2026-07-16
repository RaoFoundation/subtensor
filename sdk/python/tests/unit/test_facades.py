"""Facade/namespace parity.

The dynamic proxy (``sync._SyncNamespace``) and the hand-written blocking
facades (``SyncClient``, ``SyncSnapshot``) rest on invariants a type checker
cannot see — every namespace read is a coroutine, and every public async
method on ``Client``/``Snapshot`` has a blocking twin with the same
parameters. These tests pin those invariants so drift fails CI instead of
failing a caller at runtime.
"""

from __future__ import annotations

import asyncio
import inspect

import pytest

from bittensor.client import Client
from bittensor.namespaces import NAMESPACES
from bittensor.snapshot import Snapshot
from bittensor.sync import _NAMESPACES, SyncClient, SyncSnapshot, _SyncNamespace

NAMESPACE_CLASSES = NAMESPACES


def _public_functions(cls) -> dict[str, object]:
    return {
        name: fn
        for name, fn in inspect.getmembers(cls, predicate=inspect.isfunction)
        if not name.startswith("_")
    }


def _async_surface(cls) -> dict[str, object]:
    """Public coroutine methods and async generators of a class."""
    return {
        name: fn
        for name, fn in _public_functions(cls).items()
        if asyncio.iscoroutinefunction(fn) or inspect.isasyncgenfunction(fn)
    }


def _param_names(fn) -> list[str]:
    return [name for name in inspect.signature(fn).parameters if name != "self"]


@pytest.mark.parametrize("cls", NAMESPACE_CLASSES.values(), ids=NAMESPACE_CLASSES.keys())
def test_namespace_reads_are_coroutines(cls):
    """Every public namespace method must be async — the sync facade blocks on them.

    Some namespaces have no curated methods (their whole surface is dynamic
    registry dispatch, which always yields coroutines), so empty is fine.
    """
    methods = _public_functions(cls)
    not_async = [name for name, fn in methods.items() if not asyncio.iscoroutinefunction(fn)]
    assert not not_async, (
        f"{cls.__name__} has non-async public methods {not_async}; the sync "
        "facade proxies namespaces as coroutine methods"
    )


def test_namespace_lists_agree():
    """Client, Snapshot, and the sync facade all carry the same namespace set."""
    client = Client("finney")  # cold construction: no I/O
    for name, cls in NAMESPACE_CLASSES.items():
        assert isinstance(getattr(client, name), cls)
    snapshot = Snapshot(client, block=1)
    for name, cls in NAMESPACE_CLASSES.items():
        assert isinstance(getattr(snapshot, name), cls)
    assert set(_NAMESPACES) == set(NAMESPACE_CLASSES)


@pytest.mark.parametrize(
    ("async_cls", "sync_cls"),
    [(Client, SyncClient), (Snapshot, SyncSnapshot)],
    ids=["client", "snapshot"],
)
def test_sync_facade_mirrors_async_surface(async_cls, sync_cls):
    """Every public async method has a blocking twin with identical parameter names."""
    surface = _async_surface(async_cls)
    assert surface, f"{async_cls.__name__} exposes no public async methods?"
    for name, fn in surface.items():
        sync_fn = getattr(sync_cls, name, None)
        assert sync_fn is not None, f"{sync_cls.__name__} is missing {name!r}"
        assert not asyncio.iscoroutinefunction(sync_fn), (
            f"{sync_cls.__name__}.{name} must block, not return a coroutine"
        )
        assert _param_names(sync_fn) == _param_names(fn), (
            f"{sync_cls.__name__}.{name} parameters drifted from {async_cls.__name__}.{name}"
        )


def test_sync_namespace_passes_sync_members_through():
    """Non-coroutine attributes must not be fed to the event loop."""

    class Namespace:
        constant = 7

        def plain(self):
            return "plain"

        async def fetch(self):
            return "fetched"

    ns = _SyncNamespace(Namespace(), asyncio.run)
    assert ns.constant == 7
    assert ns.plain() == "plain"
    assert ns.fetch() == "fetched"


def test_sync_client_constructs_offline():
    """Cold construction: no connection until connect() or first use."""
    client = SyncClient("finney")
    try:
        assert client.network == "finney"
        assert client._state["connected"] is False
    finally:
        client.close()
