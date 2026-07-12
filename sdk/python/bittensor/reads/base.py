"""The read registry: named, catalog-able, block-pinnable typed reads.

Every read is an async function of a **view** — anything exposing the client's
generic read surface (``query`` / ``query_map`` / ``query_batch`` / ``runtime``
/ ``constant`` / ``balance`` / ``at`` / ``timestamp`` / ``block_time``). Both
:class:`~bittensor.client.Client` (reads at the chain head) and
:class:`~bittensor.snapshot.Snapshot` (reads pinned to one block) are views, so
a read written once works at the head via ``client.read(...)`` and at any block
via ``(await client.at(block)).read(...)`` with no per-read block plumbing.

A read that needs several queries to land on the *same* block pins itself with
``view = await view.at()``: on a client that snapshots the current head, on a
snapshot it is a no-op. The pinned view carries its block as ``view.block``.

Reads register under a stable machine name via the :func:`read` decorator and
inherit dispatch (``client.read``), the agent catalog (``client.reads``), and
the generated CLI ``query`` command. The typed namespaces
(``client.balances`` / ``staking`` / ``subnets`` / ``neurons``) are thin
projections over the same fetch functions — one implementation, two surfaces.
"""

from __future__ import annotations

import inspect
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Optional


@dataclass
class ReadSpec:
    name: str
    doc: str  # the read's full docstring (first line = summary)
    params: dict[str, str]  # param name -> JSON type, for the catalog/CLI
    fetch: Callable[..., Awaitable[Any]]
    category: str  # topical grouping, rendered as a help panel by `query --help`
    param_docs: dict[str, str]  # param name -> meaning, for --help and the catalog

    @property
    def summary(self) -> str:
        return self.doc.split("\n")[0]


REGISTRY: dict[str, ReadSpec] = {}


def read(
    name: str,
    params: Optional[dict[str, str]] = None,
    *,
    category: str,
    param_docs: Optional[dict[str, str]] = None,
):
    """Register a read under a stable machine name.

    ``param_docs`` documents what each param means; it feeds the generated
    ``query`` command's option help and the agent catalog. Address params
    (``*_ss58``) get their accepted-input-shapes note appended automatically.
    """

    def decorate(fn: Callable[..., Awaitable[Any]]) -> Callable[..., Awaitable[Any]]:
        if name in REGISTRY:
            raise ValueError(f"Duplicate read: {name}")
        doc = inspect.cleandoc(fn.__doc__ or "").replace("``", "`")
        unknown = set(param_docs or {}) - set(params or {})
        if unknown:
            raise ValueError(f"param_docs for unknown params of read {name!r}: {sorted(unknown)}")
        REGISTRY[name] = ReadSpec(name, doc, params or {}, fn, category, param_docs or {})
        return fn

    return decorate


async def dispatch(view, name: str, params: dict[str, Any]) -> Any:
    """Run a registered read against a view (the client or a snapshot)."""
    try:
        spec = REGISTRY[name]
    except KeyError:
        raise ValueError(f"Unknown read {name!r}. Known reads: {sorted(REGISTRY)}") from None
    return await spec.fetch(view, **params)


def list_reads() -> list[dict[str, Any]]:
    """Machine-readable catalog of every read (for agents / the CLI)."""
    return [
        {
            "name": s.name,
            "summary": s.summary,
            "description": s.doc,
            "params": s.params,
            "param_docs": s.param_docs,
            "category": s.category,
        }
        for s in sorted(REGISTRY.values(), key=lambda s: s.name)
    ]


def scalar_read(name: str, item, *, per_netuid: bool, doc: str, category: str):
    """Register a one-storage-value read (query + int cast) in one line."""
    params = {"netuid": "integer"} if per_netuid else {}
    param_docs = {"netuid": "Subnet to query."} if per_netuid else {}

    async def fetch(view, netuid: Optional[int] = None) -> int:
        keys = [netuid] if per_netuid else None
        return int(await view.query(item, keys))

    fetch.__doc__ = doc
    read(name, params, category=category, param_docs=param_docs)(fetch)


def utf8_text(value: Any) -> Any:
    """Decode chain byte-strings (hex or bytes) to text where possible."""
    if isinstance(value, str) and value.startswith("0x"):
        value = bytes.fromhex(value[2:])
    if isinstance(value, (bytes, bytearray)):
        try:
            return value.decode("utf-8")
        except UnicodeDecodeError:
            return "0x" + bytes(value).hex()
    return value
