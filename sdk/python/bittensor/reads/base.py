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
inherit dispatch (``client.read``), the agent catalog (``client.reads``), the
generated CLI ``query`` command, and a typed namespace method
(``client.subnets.subnet_registration_cost()`` — see ``bittensor.namespaces``)
— one implementation, several surfaces.
"""

from __future__ import annotations

import functools
import inspect
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Optional, Union

from ..settings import query_docs_url
from ..signing import coerce_address, is_address_param


@dataclass(frozen=True)
class Grouped:
    """Render hint for ``{group: [record, ...]}`` results: flatten to a table
    with ``key`` as the leading column (e.g. epoch for weight commits)."""

    key: str


@dataclass(frozen=True)
class Matrix:
    """Render hint for ``{row: {col: value}}`` results: flatten to a long-form
    three-column table with these column names."""

    row: str
    col: str
    value: str


# A read whose result isn't naturally a flat record/list declares its shape
# here, and the generated `query` command renders it as a table instead of
# falling back to a key/value dump of the raw mapping.
RenderHint = Union[Grouped, Matrix]


@dataclass
class ReadSpec:
    name: str
    doc: str  # the read's full docstring (first line = summary)
    params: dict[str, str]  # param name -> JSON type, for the catalog/CLI
    fetch: Callable[..., Awaitable[Any]]
    category: str  # topical grouping, rendered as a help panel by `query --help`
    param_docs: dict[str, str]  # param name -> meaning, for --help and the catalog
    render: Optional[RenderHint] = None  # table shape for non-record results

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
    render: Optional[RenderHint] = None,
):
    """Register a read under a stable machine name.

    ``param_docs`` documents what each param means; it feeds the generated
    ``query`` command's option help and the agent catalog. Address params
    (``*_ss58``) get their accepted-input-shapes note appended automatically.
    ``render`` declares the table shape for results that aren't flat records
    (see :data:`RenderHint`).
    """

    def decorate(fn: Callable[..., Awaitable[Any]]) -> Callable[..., Awaitable[Any]]:
        if name in REGISTRY:
            raise ValueError(f"Duplicate read: {name}")
        doc = inspect.cleandoc(fn.__doc__ or "").replace("``", "`")
        unknown = set(param_docs or {}) - set(params or {})
        if unknown:
            raise ValueError(f"param_docs for unknown params of read {name!r}: {sorted(unknown)}")
        registered = _coercing(fn, params or {})
        REGISTRY[name] = ReadSpec(
            name, doc, params or {}, registered, category, param_docs or {}, render
        )
        return registered

    return decorate


def _coercing(fn: Callable[..., Awaitable[Any]], params: dict[str, str]):
    """Wrap a read so its ``*_ss58`` parameters also accept key-bearing objects
    (a ``Wallet``, keypair, or signer — normalized by
    :func:`~bittensor.signing.coerce_address`, the same seam intents use).
    Reads without address parameters are registered untouched."""
    ss58_params = [p for p in params if is_address_param(p)]
    if not ss58_params:
        return fn
    sig = inspect.signature(fn)

    @functools.wraps(fn)
    async def wrapped(*args, **kwargs):
        bound = sig.bind_partial(*args, **kwargs)
        for p in ss58_params:
            value = bound.arguments.get(p)
            if value is None or isinstance(value, str):
                continue
            bound.arguments[p] = coerce_address(value, p)
        return await fn(*bound.args, **bound.kwargs)

    return wrapped


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
            "docs_url": query_docs_url(s.name),
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
    """Decode chain byte-strings (hex, bytes, or byte-value lists) to text
    where possible. Compact-u16 vectors (e.g. the metagraph's ``name`` and
    ``symbol``) arrive as lists of ints carrying utf-8 bytes."""
    if isinstance(value, str) and value.startswith("0x"):
        value = bytes.fromhex(value[2:])
    if isinstance(value, (list, tuple)):
        try:
            value = bytes(int(b) for b in value)
        except (TypeError, ValueError):
            return value
    if isinstance(value, (bytes, bytearray)):
        try:
            return value.decode("utf-8")
        except UnicodeDecodeError:
            return "0x" + bytes(value).hex()
    return value
