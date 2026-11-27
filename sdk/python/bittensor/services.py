"""Call the services that validators and subnet owners publish on a subnet.

A published service is a url, on chain, to its document (see the services
guide). The document names OpenAPI 3 specs, in JSON or YAML, as ``openapi``
services whose ``endpoint`` is the spec or ``http`` services with the spec at
``spec``, and each spec's servers are where its API is. An operation a spec
secures with the ``btauth/1`` scheme (``apiKey`` headers, ``X-Bittensor-Signature``
among them) is signed by the caller's hotkey, optionally when it also allows
``{}``, and one with no security requirement is open to anyone; when a spec
declares no such scheme, every operation it secures is signed, and one it says
nothing about is signed when a signer is given. Each operation is called by its
``operationId`` (``"GET /path"`` when it has none, or shares it) or that name as
a command line writes it (``rent-by-spec``), with one flat set of named inputs:
its path, query, header and cookie parameters and the fields of its body, JSON,
a form or multipart, so the same names serve code, forms, command lines and
agent tools.

The document, the spec and the responses come from the service and are not
trusted: the document must list its on-chain hotkey in ``publishers``, every
url must be https to a public host (http and private hosts only when the chain
itself is on this machine, as on a localnet), every request has a deadline, a
signed request is never redirected, and it names the service's hotkey as its
receiver, so no other service can replay it.

    published = await bittensor.services.discover(client)
    service = await bittensor.services.load(client, netuid=51)
    response = await service.call("listModels")
    signer = await bittensor.http_auth.prepare_signer(wallet, client=client)
    response = await service.call("rentBySpec", {"gpu_type": "RTX 5060"}, signer=signer)
    tools = service.tools()  # each operation as a tool definition, for an agent

A :class:`ServiceError` says what kind it is in ``code``: ``input`` (the call does
not fit the operation), ``no_service``, ``ambiguous`` (name one by its hotkey),
``unusable`` (the document or spec cannot be used as published), ``unreachable``,
``timeout``, ``too_large`` and ``http`` (with ``status``).
"""

from __future__ import annotations

import asyncio
import base64
import binascii
import contextlib
import dataclasses
import difflib
import functools
import gzip
import hashlib
import http.client
import io
import ipaddress
import itertools
import json
import mimetypes
import os
import re
import secrets
import socket
import ssl
import tempfile
import threading
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
import zlib
from dataclasses import dataclass, field
from email.utils import parsedate_to_datetime
from pathlib import Path
from typing import IO, Any, Callable, Iterable, Mapping, Optional, TypeVar, Union

from . import http_auth
from ._generated import storage
from .result import BittensorError
from .wallets import is_bittensor_address

__all__ = [
    "Input",
    "Operation",
    "Published",
    "Response",
    "Service",
    "ServiceDocument",
    "ServiceError",
    "Skipped",
    "discover",
    "load",
    "load_document",
    "load_url",
]

MAX_DOCUMENT_BYTES = 8 << 20
# A service can publish a large spec, such as Microsoft Graph's 74 MB spec.
MAX_SPEC_BYTES = 128 << 20
MAX_SPECS = 8
MAX_SPEC_TOKENS = 4_500_000
# A response held in memory; one written to a file as it arrives has no limit.
MAX_RESPONSE_BYTES = 64 << 20
# Seconds a request may take in all, however slowly the host answers: a call or
# the fetch of a spec, which can be megabytes, and the fetch of a document or a balance.
TIMEOUT = 60.0
FETCH_TIMEOUT = 20.0
# A document or spec is kept as long as its Cache-Control max-age says, up to this.
CACHE_MAX_SECONDS = 600
# The bytes a cache directory holds at most; the least recently written entries go first.
CACHE_MAX_BYTES = 256 << 20
# A directory for documents and specs their Cache-Control lets a client keep; None keeps none.
CacheDir = Optional[Union[str, Path]]
# How many schema nodes one resolve expands, at most, when it replaces $refs.
RESOLVE_BUDGET = 200_000
RESOLVE_DEPTH = 32
MAX_TOOL_NODES = 500_000

_METHODS = ("get", "put", "post", "delete", "patch", "head", "options", "trace")
_JSON = "application/json"
_NOT_GIVEN = object()
# JSON Schema keywords whose values contain schemas, rather than literal instance data.
_SCHEMA_COLLECTIONS = frozenset(
    [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
        "dependencies",
        "allOf",
        "oneOf",
        "anyOf",
        "prefixItems",
    ]
)
_SCHEMA_KEYWORDS = frozenset(
    [
        "items",
        "additionalProperties",
        "unevaluatedProperties",
        "additionalItems",
        "unevaluatedItems",
        "contains",
        "propertyNames",
        "if",
        "then",
        "else",
        "not",
        "contentSchema",
    ]
)
# Keywords a standalone tool schema cannot use: identifiers and dynamic references
# that resolve against the spec, and the spec's dialect.
_UNPORTABLE_KEYWORDS = frozenset(
    [
        "$schema",
        "$id",
        "$anchor",
        "$dynamicAnchor",
        "$dynamicRef",
        "$recursiveAnchor",
        "$recursiveRef",
    ]
)
_BTAUTH_HEADERS = frozenset(
    h.lower()
    for h in (
        http_auth.HEADER_SIGNATURE,
        http_auth.HEADER_VERSION,
        http_auth.HEADER_HOTKEY,
        http_auth.HEADER_RECEIVER,
        http_auth.HEADER_NONCE,
        http_auth.HEADER_CRYPTO,
    )
)
# Headers the client writes itself, and those OpenAPI says a parameter cannot name.
_RESERVED_HEADERS = {
    "accept",
    "authorization",
    "connection",
    "content-length",
    "content-type",
    "host",
    "transfer-encoding",
}
# Security schemes a signature cannot stand in for.
_FOREIGN_SCHEMES = frozenset(["http", "oauth2", "openIdConnect", "mutualTLS"])
_TOKEN = re.compile(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+")
_USER_AGENT = "bittensor-services/1"


class ServiceError(BittensorError):
    """A service cannot be used as published, or a call does not fit its operation."""

    def __init__(self, message: str, *, status: Optional[int] = None, code: str = "unusable"):
        # The message quotes what the service sent, which a terminal must not act
        # on, and one line of it must not pass for another.
        super().__init__(_line(message))
        self.status = status  # the HTTP status, when the service answered
        self.code = code  # its kind, as the module lists them


@dataclass(frozen=True)
class Input:
    """A named input of an operation."""

    name: str
    # path, query, header or cookie: a parameter; field: a field of the body object;
    # body: the whole body, when it is not an object of named fields
    location: str
    required: bool
    description: str
    schema: Mapping[str, Any] = field(repr=False)
    # How a parameter or a form field is written, by its OpenAPI style and explode;
    # "json" for one whose content is JSON
    style: str = field(default="", repr=False)
    explode: bool = field(default=False, repr=False)
    # A path parameter the JSON body has a field of the same name for, which it fills too
    in_body: bool = field(default=False, repr=False)
    required_in_body: bool = field(default=False, repr=False)
    # The name it is sent by, when it shares one with another input and was renamed
    wire: str = field(default="", repr=False)
    # A file: bytes, a (filename, bytes) pair, a binary file, or base64 text
    binary: bool = field(default=False, repr=False)
    content_type: str = field(default="", repr=False)  # of its multipart part
    flag: str = field(default="", repr=False)  # its command name, when its own is taken

    @property
    def must_supply(self) -> bool:
        """Whether a caller must provide this value explicitly."""
        return self.required and (
            self.location == "body" or not any(k in self.schema for k in ("const", "default"))
        )

    @property
    def sent_as(self) -> str:
        return self.wire or self.name

    @property
    def command(self) -> str:
        """The name as a command line writes it: ``gpu_type`` is ``gpu-type``."""
        return self.flag or _kebab(self.name)


@dataclass(frozen=True)
class Operation:
    """An operation a service offers, called with :meth:`Service.call`."""

    id: str  # its operationId, or "GET /path" when it has none or shares it
    method: str
    path: str
    # hotkey: signed by the caller's hotkey; optional: signed when a signer is
    # given, as for a paid tier over a free one; open: never signed
    auth: str
    summary: str
    description: str
    inputs: tuple[Input, ...] = field(repr=False)
    api_url: str = field(repr=False)  # its server: its own, its path's, or the spec's
    body_required: bool = field(default=False, repr=False)
    body_type: str = field(default="", repr=False)  # the Content-Type of its body
    tags: tuple[str, ...] = ()
    body_input: Optional[Input] = field(default=None, repr=False)
    # json, form (urlencoded), multipart, or raw: bytes or text as they are
    body_kind: str = field(default="json", repr=False)
    # The path as it is sent and the query it fixes: a path's # part is not sent,
    # and its ? part is a query of its own (/?Action=List)
    route: str = field(default="", repr=False)
    fixed_query: tuple[tuple[str, str], ...] = field(default=(), repr=False)
    spec: Mapping[str, Any] = field(default_factory=dict, repr=False, compare=False)

    @property
    def tool_inputs(self) -> tuple[Input, ...]:
        """Parameters and an exact body, as named by the tool schema."""
        params = tuple(i for i in self.inputs if i.location not in ("field", "body"))
        return (*params, self.body_input) if self.body_input is not None else params

    def prepare(self, inputs: Optional[Mapping[str, Any]] = None, *, body: Any = _NOT_GIVEN):
        """Construct the unsigned request without network or wallet access.

        Pass body to supply the exact body, including JSON null, instead of its fields.
        """
        return _request(self, {} if inputs is None else inputs, body)

    @property
    def command(self) -> str:
        """The name as a command line writes it: ``rentBySpec`` is ``rent-by-spec``."""
        return _kebab(self.id)

    def resolve(self, schema: Any) -> Any:
        """``schema`` with the local ``$ref`` of this operation's spec replaced by their
        targets, as :meth:`Service.resolve` does."""
        return _resolve(self.spec, schema, [RESOLVE_BUDGET])

    def _resolver(self) -> Callable[[Any], Any]:
        """Resolve several inputs under one work budget."""
        budget = [RESOLVE_BUDGET]
        return functools.partial(_resolve, self.spec, budget=budget, strict=True)

    @property
    def tool_name(self) -> str:
        """Its command name within the 64 characters agent tools allow."""
        command = self.command
        if len(command) <= 64:
            return command
        return command[:55].rstrip("-") + "-" + hashlib.sha256(command.encode()).hexdigest()[:8]


@dataclass(frozen=True)
class Skipped:
    """An operation of the spec that is not offered, and why."""

    method: str  # "" when it is the whole path
    path: str
    id: str  # its operationId, "" when it has none
    reason: str

    def __str__(self) -> str:
        name = f"{self.method} {self.path}".strip()
        return f"{name} ({self.id}) {self.reason}" if self.id else f"{name} {self.reason}"


@dataclass(frozen=True)
class Response:
    """What a service answered, whatever its status."""

    status: int
    headers: Mapping[str, str]  # by lowercase name
    content: bytes = field(repr=False)
    url: str = ""  # effective URL after redirects

    @property
    def ok(self) -> bool:
        return 200 <= self.status < 300

    @property
    def content_type(self) -> str:
        return self.headers.get("content-type", "").split(";")[0].strip().lower()

    @property
    def is_json(self) -> bool:
        return _is_json(self.content_type)

    @property
    def text(self) -> str:
        charset = re.search(r"charset=\"?([\w.:-]+)", self.headers.get("content-type", ""))
        try:
            return self.content.decode(charset.group(1) if charset else "utf-8", "replace")
        except (LookupError, UnicodeError):  # a charset that is not one, or not text
            return self.content.decode("utf-8", "replace")

    @property
    def message(self) -> str:
        """What an error response says: its message when it is JSON with one, else its text."""
        try:
            parsed = self.json() if self.is_json else None
        except (ValueError, RecursionError):
            parsed = None
        if isinstance(parsed, dict):
            error = parsed.get("error")
            for message in (
                error.get("message") if isinstance(error, dict) else None,
                parsed.get("message"),
                parsed.get("detail"),
                error,
            ):
                if isinstance(message, str) and message.strip():
                    return message.strip()
        return self.text.strip()

    def json(self) -> Any:
        """The body as JSON; NaN and Infinity are not JSON."""
        return json.loads(self.content, parse_constant=_not_json)

    def raise_for_status(self) -> Response:
        """The response, or a ServiceError with its ``status`` when it is not a 2xx."""
        if self.ok:
            return self
        message = self.message[:200]
        raise ServiceError(
            f"HTTP {self.status}" + (f": {message}" if message else ""),
            status=self.status,
            code="http",
        )


@dataclass(frozen=True, kw_only=True)
class ServiceDocument:
    """A publisher's payment and identity metadata, independent of its API spec."""

    netuid: int
    hotkey: str
    url: str
    name: str
    description: str = field(repr=False)
    balance_url: str = field(repr=False)
    document_url: str = field(repr=False)
    document: Mapping[str, Any] = field(repr=False)
    local: bool = field(default=False, repr=False)
    owner: Optional[bool] = field(default=None, repr=False)

    @property
    def signable(self) -> bool:
        """Whether requests to it may be signed. The chain ties a published service's
        hotkey to its url. A document read by its url can name any hotkey, and a
        request signed for that hotkey could be passed on to its real service, so
        URL mode signs only when both the document and request stay on this machine."""
        return self.owner is not None or (self.local and _is_local(self.document_url))

    def _check_signable(self, url: str) -> None:
        if self.owner is None and not (self.signable and _is_local(url)):
            raise ServiceError(
                f"{self.url} was read by its url, so nothing ties hotkey {self.hotkey} to it; "
                "signed requests require a published service or a local document and endpoint",
                code="input",
            )

    async def balance(self, hotkey_ss58: Optional[str] = None, *, signer: Any = None) -> dict:
        """The hotkey's balance at the service, as its ``balance`` url answers it.

        The url is read unsigned, then signed by ``signer`` (whose hotkey is the
        default) when the service answers that it needs a signature.
        """
        if signer is not None:
            signer = http_auth.resolve_signer(signer, role="hotkey")
            hotkey_ss58 = hotkey_ss58 or signer.ss58_address
        if not hotkey_ss58:
            raise ServiceError("name the hotkey whose balance to read", code="input")
        if not is_bittensor_address(hotkey_ss58):
            raise ServiceError("the balance needs a valid Bittensor hotkey address", code="input")
        split = urllib.parse.urlsplit(self.balance_url)
        params = urllib.parse.parse_qsl(split.query, keep_blank_values=True)
        query = urllib.parse.urlencode(
            [(k, v) for k, v in params if k != "hotkey"] + [("hotkey", hotkey_ss58)]
        )
        url = urllib.parse.urlunsplit(split._replace(query=query))
        target = urllib.parse.urlunsplit(("", "", split.path or "/", query, ""))

        def read(key=None) -> Response:
            headers = {"User-Agent": _USER_AGENT, "Accept": _JSON}
            if key is not None:
                headers.update(
                    http_auth.sign(key, method="GET", path=target, receiver_ss58=self.hotkey)
                )
            return _send("GET", url, headers, b"", self.local, MAX_DOCUMENT_BYTES, FETCH_TIMEOUT)

        response = await _threaded(read)
        if response.status in (401, 403) and signer is not None:
            if signer.ss58_address != hotkey_ss58:
                raise ServiceError(
                    "the balance must be signed by the requested hotkey", code="input"
                )
            self._check_signable(url)
            response = await _threaded(read, signer)
        record = _json_object(response, url, "the balance")
        if "hotkey" in record and record["hotkey"] != hotkey_ss58:
            raise ServiceError(f"the balance at {url} is {record['hotkey']}'s, not {hotkey_ss58}'s")
        if not isinstance(record.get("remaining_usd"), str):
            raise ServiceError(f"the balance at {url} has no remaining_usd")
        return record


@dataclass(frozen=True)
class Service(ServiceDocument):
    """A published service with the operations its API specs offer or skip."""

    spec_url: str = field(repr=False)  # its first spec, whose servers place ``api_url``
    api_url: str
    operations: Mapping[str, Operation] = field(repr=False)
    tags: Mapping[str, str] = field(default_factory=dict, repr=False)
    skipped: tuple[Skipped, ...] = field(default=(), repr=False)
    spec: Mapping[str, Any] = field(default_factory=dict, repr=False)
    spec_urls: tuple[str, ...] = field(default=(), repr=False)  # every spec it offers from

    def operation(self, name: str) -> Operation:
        """An operation by its id, its command name (``rent-by-spec``) or its tool name."""
        wanted = _kebab(name)
        op = self.operations.get(name) or next(
            (op for op in self.operations.values() if wanted in (op.command, op.tool_name)), None
        )
        if op is None:
            for skip in self.skipped:
                if wanted and wanted in (_kebab(f"{skip.method} {skip.path}"), _kebab(skip.id)):
                    raise ServiceError(
                        f"{name!r} is not offered by {self.name}: {skip}", code="input"
                    )
            commands = [op.command for op in self.operations.values()]
            close = [c for c in commands if wanted and wanted in c][
                :3
            ] or difflib.get_close_matches(wanted, commands, n=3)
            hint = f"; did you mean {', '.join(close)}?" if close else ""
            raise ServiceError(f"no operation {name!r} at {self.name}{hint}", code="input")
        return op

    def tool(self, operation: str) -> dict:
        """An operation as a tool definition, as agent frameworks take one: its
        ``name`` (which :meth:`call` takes back), its ``description`` and
        ``input_schema``, the JSON Schema of its inputs. A file input is base64 text."""
        return self._tools((self.operation(operation),))[0]

    def tools(self) -> list[dict]:
        """Every operation's standalone tool definition."""
        return self._tools(self.operations.values())

    def _tools(self, operations: Iterable[Operation]) -> list[dict]:
        definitions: dict[tuple[int, str], tuple[Any, set[str]]] = {}
        work = [MAX_TOOL_NODES]
        remaining = MAX_TOOL_NODES
        tools = []
        for op in operations:
            tool = self._tool(op, definitions, work)
            pending = [tool]
            while pending:
                node = pending.pop()
                remaining -= _node_cost(node)
                if remaining < 0:
                    raise ServiceError(
                        "the combined tool schemas are too large; request one operation's schema",
                        code="too_large",
                    )
                if isinstance(node, dict):
                    pending.extend(node.keys())
                    pending.extend(node.values())
                elif isinstance(node, list):
                    pending.extend(node)
            tools.append(tool)
        return tools

    def _tool(
        self,
        op: Operation,
        definitions: dict[tuple[int, str], tuple[Any, set[str]]],
        budget: list[int],
    ) -> dict:
        # Each schema a reference reaches is copied once into ``definitions``, which the
        # tools of one call share, and every tool takes those it reaches as its $defs,
        # so recursive schemas stay finite and each tool stands alone. A copy keeps the
        # spec's literals as they are and only drops what a tool cannot use. It
        # describes a request: a read-only property is the service's to fill, and
        # OpenAPI 3.0's ``nullable`` is JSON Schema's null type.
        spec = op.spec or self.spec

        def copy(node: Any, refs: set[str], mode="schema", depth=0) -> Any:
            budget[0] -= _node_cost(node)
            if budget[0] < 0:
                raise ServiceError(
                    "the tool schemas are too large; request one operation's schema",
                    code="too_large",
                )
            if depth > 128:
                return {} if mode == "schema" else node
            if mode == "text":
                return _plain(node)
            child = "schema" if mode == "schemas" else "literal"
            if isinstance(node, list):
                return [copy(v, refs, child, depth + 1) for v in node]
            if not isinstance(node, dict):
                return node
            out = {}
            for key, value in node.items():
                budget[0] -= _node_cost(key)
                child = "schema" if mode == "schemas" else "literal"
                if mode == "schema":
                    if key == "$ref":
                        if isinstance(value, str) and value.startswith("#/"):
                            refs.add(value)
                            out[key] = "#/$defs/" + _definition_name(value)
                        continue  # a reference outside the spec says nothing here
                    if key in _UNPORTABLE_KEYWORDS:
                        continue
                    if key == "format" and value == "binary":
                        out["contentEncoding"] = "base64"
                        continue
                    if key in ("description", "title") and isinstance(value, str):
                        child = "text"
                    elif key in _SCHEMA_COLLECTIONS:
                        child = "schemas"
                    elif key in _SCHEMA_KEYWORDS:
                        child = "schemas" if isinstance(value, list) else "schema"
                out[key] = copy(value, refs, child, depth + 1)
            if mode == "schema":
                properties = node.get("properties")
                hidden = {
                    name
                    for name, prop in (properties.items() if isinstance(properties, dict) else ())
                    if _get(_deref(spec, prop), "readOnly") is True
                }
                if hidden and isinstance(out.get("properties"), dict):
                    out["properties"] = {
                        k: v for k, v in out["properties"].items() if k not in hidden
                    }
                    if isinstance(out.get("required"), list):
                        out["required"] = [r for r in out["required"] if r not in hidden]
                if node.get("nullable") is True:
                    out.pop("nullable", None)
                    if isinstance(out.get("enum"), list) and None not in out["enum"]:
                        out["enum"] = [*out["enum"], None]
                    if isinstance(out.get("type"), str):
                        out["type"] = [out["type"], "null"]
                    elif "type" not in out:
                        out = {"anyOf": [out, {"type": "null"}]}
            return out

        refs: set[str] = set()
        properties = {}
        for item in op.tool_inputs:
            budget[0] -= _node_cost(item.name) + _node_cost(item.description)
            schema = copy(item.schema, refs)
            if isinstance(schema, dict) and item.description:
                schema.setdefault("description", _plain(item.description))
            properties[item.name] = schema
        input_schema = {
            "type": "object",
            "additionalProperties": False,
            "properties": properties,
            "required": [item.name for item in op.tool_inputs if item.must_supply],
        }
        reached: dict[str, Any] = {}
        pending = sorted(refs)
        while pending:
            ref = pending.pop()
            if ref in reached:
                continue
            key = (id(spec), ref)
            if key not in definitions:
                try:
                    target = _pointer(spec, ref)
                except ServiceError:
                    target = {}
                found: set[str] = set()
                definitions[key] = (copy(target, found), found)
            reached[ref], more = definitions[key]
            pending.extend(more - reached.keys())
        if reached:
            input_schema["$defs"] = {_definition_name(ref): reached[ref] for ref in sorted(reached)}
        description = "\n\n".join(
            t for t in (op.summary, op.description if op.description != op.summary else "") if t
        )
        budget[0] -= _node_cost(description)
        if budget[0] < 0:
            raise ServiceError("the tool descriptions are too large", code="too_large")
        return {
            "name": op.tool_name,
            "description": _plain(description),
            "input_schema": input_schema,
        }

    def resolve(self, schema: Any) -> Any:
        """``schema`` with the first spec's local ``$ref`` replaced by their targets
        (:meth:`Operation.resolve` for an operation's own spec), up to
        RESOLVE_BUDGET nodes; past it, and for a cyclic ref, a ref stays a ref."""
        return _resolve(self.spec, schema, [RESOLVE_BUDGET])

    async def call(
        self,
        operation: str,
        inputs: Optional[Mapping[str, Any]] = None,
        *,
        signer: Any = None,
        body: Any = _NOT_GIVEN,
        timeout: float = TIMEOUT,
        into: Optional[Callable[[Response], Optional[IO[bytes]]]] = None,
    ) -> Response:
        """Call an operation with its inputs by name; returns the response whatever its status.

        A list repeats a query parameter, and a file input takes bytes, a
        ``(filename, bytes)`` pair, a binary file or base64 text. A ``hotkey``
        operation needs ``signer``, and an ``optional`` one is signed when it is
        given: anything :func:`bittensor.http_auth.sign` accepts, prepared with
        :func:`bittensor.http_auth.prepare_signer`.

        ``into`` streams a successful body: called with the response before its
        body, it returns a binary file to write the body to as it arrives, or None
        to read it into ``content``. A streamed body has no size limit and no
        deadline of its own, each wait for more taking at most ``timeout``, so a
        long answer, as a stream of events, is not cut off; its ``content`` is empty.
        """
        op = self.operation(operation)
        return await self._send_prepared(
            op, op.prepare(inputs, body=body), signer=signer, timeout=timeout, into=into
        )

    async def _send_prepared(
        self,
        op: Operation,
        prepared,
        *,
        signer: Any = None,
        timeout: float = TIMEOUT,
        into: Optional[Callable[[Response], Optional[IO[bytes]]]] = None,
    ) -> Response:
        _check_timeout(timeout)
        method, url, target, headers, data = prepared
        if op.auth == "hotkey" and signer is None:
            raise ServiceError(f"{op.id} is signed by a hotkey; pass a signer", code="input")
        if signer is not None and op.auth != "open":
            self._check_signable(url)

        def send() -> Response:
            # Signed as it is sent: a signature made before a wait for a thread goes stale.
            if signer is not None and op.auth != "open":
                headers.update(
                    http_auth.sign(
                        signer, method=method, path=target, body=data, receiver_ss58=self.hotkey
                    )
                )
            return _send(method, url, headers, data, self.local, MAX_RESPONSE_BYTES, timeout, into)

        return await _threaded(send)


async def load(
    client, netuid: int, hotkey_ss58: Optional[str] = None, *, cache: CacheDir = None
) -> Service:
    """Read the service a hotkey published on a subnet, with its document and spec.

    Without ``hotkey_ss58`` it is the subnet owner's service.
    A validator can publish a service too, under any name, so another is used only when
    named by its hotkey. With ``cache``, a directory, a document or spec is kept there
    as long as its Cache-Control max-age allows, up to CACHE_MAX_SECONDS.
    """
    return await _load_published(client, netuid, hotkey_ss58, cache, _fetch_service)


async def load_document(
    client, netuid: int, hotkey_ss58: Optional[str] = None, *, cache: CacheDir = None
) -> ServiceDocument:
    """Read current publisher and payment metadata without fetching its OpenAPI spec."""
    return await _load_published(client, netuid, hotkey_ss58, cache, _document)


_Document = TypeVar("_Document", bound=ServiceDocument)


async def _load_published(
    client,
    netuid: int,
    hotkey_ss58: Optional[str],
    cache: CacheDir,
    fetch: Callable[[int, str, str, bool, CacheDir], _Document],
) -> _Document:
    snapshot, local = await asyncio.gather(client.at(), _on_local_chain(client))
    published = await snapshot.read("services", netuid=netuid)
    if hotkey_ss58 is None:
        if not published:
            raise ServiceError(f"netuid {netuid} has no services", code="no_service")
        hotkey_ss58 = next((hotkey for hotkey, entry in published.items() if entry["owner"]), None)
        if hotkey_ss58 is None:
            raise ServiceError(
                f"netuid {netuid} has no service from its subnet owner; name a validator's "
                "hotkey: " + ", ".join(sorted(published)),
                code="ambiguous",
            )
    entry = published.get(hotkey_ss58)
    if entry is None:
        raise ServiceError(
            f"{hotkey_ss58} has published no service on netuid {netuid}", code="no_service"
        )
    service = await _threaded(fetch, netuid, hotkey_ss58, entry["url"], local, cache)
    return dataclasses.replace(service, owner=entry["owner"])


async def load_url(
    url: str,
    netuid: Optional[int] = None,
    hotkey_ss58: Optional[str] = None,
    *,
    cache: CacheDir = None,
) -> Service:
    """Read the service a document describes, by its url, without the chain: to
    check a document before it is published, or to use one known by its url.

    The document is held to the rules of a published one: a url on this machine
    may use http and private hosts, as on a localnet, any other only https to
    public hosts. It must list ``netuid`` and ``hotkey_ss58`` in ``publishers``,
    and without them it must list exactly one publisher, whose service it is.
    Signed requests require both the document and endpoint to be on this machine.
    """
    return await _threaded(_fetch_service, netuid, hotkey_ss58, url, _is_local(url), cache)


@dataclass(frozen=True)
class Published:
    """A service published on chain, as :func:`discover` finds it."""

    netuid: int
    hotkey: str
    url: str
    service: Optional[Service] = None  # None when it cannot be read
    problem: str = ""  # why it cannot be used, as when it offers no operation
    owner: bool = False  # published by the subnet owner's hotkey, not a validator's


async def discover(
    client,
    netuid: Optional[int] = None,
    *,
    cache: CacheDir = None,
    select: Optional[Callable[[Published], Any]] = None,
) -> list:
    """Read the services published on ``netuid``, or on every subnet, as
    :class:`Published`, or as what ``select`` makes of each, to keep only what a
    caller needs."""
    snapshot, local = await asyncio.gather(client.at(), _on_local_chain(client))
    if netuid is None:
        rows = await snapshot.query_map(storage.SubtensorModule.Services)
        netuids = sorted({int(n) for (n, _), _ in rows})
    else:
        netuids = [netuid]
    listings = await asyncio.gather(*(snapshot.read("services", netuid=n) for n in netuids))
    entries = [
        (n, hotkey, entry["url"], entry["owner"])
        for n, published in zip(netuids, listings)
        for hotkey, entry in published.items()
    ]

    pending = iter(enumerate(sorted(entries)))
    found = [None] * len(entries)

    async def worker():
        for index, (n, hotkey, url, owner) in pending:
            try:
                service = await _threaded(_fetch_service, n, hotkey, url, local, cache)
            except Exception as error:  # one service's failure is never the listing's
                item = Published(n, hotkey, url, problem=_line(str(error)), owner=owner)
            else:
                problem = (
                    ""
                    if service.operations
                    else f"offers no operation ({len(service.skipped)} not offered)"
                )
                item = Published(
                    n,
                    hotkey,
                    url,
                    dataclasses.replace(service, owner=owner),
                    problem=problem,
                    owner=owner,
                )
            found[index] = select(item) if select else item

    await asyncio.gather(*(worker() for _ in range(min(8, len(entries)))))
    return found


async def _on_local_chain(client) -> bool:
    """Whether the chain is a localnet on this machine, whose services may use it and
    private hosts. Mainnet, testnet or a devnet through a local node or tunnel is
    not: only a localnet spec has the chain type ``Local``."""
    if not _is_local(client.endpoint):
        return False
    return await client._substrate.chain_type() == "Local"


def _plain(value: Any) -> Any:
    """``value`` with control and format characters removed from every string. A spec's
    text reaches people and agents, and these hide or reorder what they read: bidi
    overrides, zero-width characters, the invisible tag characters."""
    if isinstance(value, str):
        if value.isprintable() or value.replace("\n", "").replace("\t", "").isprintable():
            return value
        return "".join(ch for ch in value if ch in "\n\t" or unicodedata.category(ch)[0] != "C")
    if isinstance(value, dict):
        return {_plain(key): _plain(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_plain(item) for item in value]
    return value


def _fetch_service(
    netuid: Optional[int], hotkey: Optional[str], url: str, local: bool, cache: CacheDir = None
) -> Service:
    try:
        return _service(netuid, hotkey, url, local, cache)
    except ValueError as error:  # a url the service wrote that is not one
        raise ServiceError(f"the service at {url} has a bad url: {error}") from None


def _service(
    netuid: Optional[int], hotkey: Optional[str], url: str, local: bool, cache: CacheDir
) -> Service:
    metadata = _document(netuid, hotkey, url, local, cache)
    named = _spec_urls(metadata.document, metadata.document_url)
    operations: dict[str, Operation] = {}
    skipped: list[Skipped] = []
    tags: dict[str, str] = {}
    first = None
    loaded: list[str] = []
    remaining_bytes = MAX_SPEC_BYTES
    tokens = [MAX_SPEC_TOKENS]
    deadline = time.monotonic() + TIMEOUT
    for spec_url in named:
        # A spec that cannot be read loses only its own operations.
        try:
            if remaining_bytes <= 0 or tokens[0] <= 0:
                raise ServiceError("the combined OpenAPI specs are too large", code="too_large")
            timeout = deadline - time.monotonic()
            if timeout <= 0:
                raise ServiceError("timed out reading the service's OpenAPI specs", code="timeout")
            spec, spec_url, size = _get_json(
                spec_url,
                "the OpenAPI spec",
                local,
                cache,
                timeout,
                remaining_bytes,
                spec_tokens=tokens,
            )
            remaining_bytes -= size
            api_url = _checked_url(_server(spec_url, spec), "the API", local)
            skipped.extend(_operations(spec, spec_url, local, operations))
        except (ServiceError, ValueError) as error:
            if len(named) == 1:
                raise
            skipped.append(Skipped("", _line(spec_url), "", _line(str(error))))
            continue
        if first is None:
            first = spec_url, spec, api_url
        loaded.append(spec_url)
        for tag in _list(spec.get("tags")):
            if isinstance(tag, dict) and isinstance(tag.get("name"), str):
                tags.setdefault(tag["name"], _text(tag.get("description")))
    if first is None:
        raise ServiceError(f"none of the specs {url} names can be read: {skipped[0].reason}")
    spec_url, spec, api_url = first
    return Service(
        **vars(metadata),
        spec_url=spec_url,
        api_url=api_url,
        operations=operations,
        tags=tags,
        skipped=tuple(skipped),
        spec=spec,
        spec_urls=tuple(loaded),
    )


def _document(
    netuid: Optional[int], hotkey: Optional[str], url: str, local: bool, cache: CacheDir
) -> ServiceDocument:
    try:
        document, document_url, _ = _get_json(url, "the services document", local, cache)
        netuid, hotkey = _publisher(document, url, netuid, hotkey)
        balance = _checked_url(_gamma_balance(document, document_url), "the balance", local)
    except ValueError as error:
        raise ServiceError(f"the service at {url} has a bad url: {error}") from None
    return ServiceDocument(
        netuid=netuid,
        hotkey=hotkey,
        url=url,
        name=_text(document.get("name")) or url,
        description=_text(document.get("description")),
        balance_url=balance,
        document_url=document_url,
        document=document,
        local=local,
    )


def _publisher(
    document: Mapping[str, Any], url: str, netuid: Optional[int], hotkey: Optional[str]
) -> tuple[int, str]:
    """The one publisher in the document that has ``netuid`` and ``hotkey``, when given."""
    listed = [
        (p["netuid"], p["hotkey"])
        for p in _list(document.get("publishers"))
        if isinstance(p, dict)
        and type(p.get("netuid")) is int
        and 0 <= p["netuid"] <= 65535
        and isinstance(p.get("hotkey"), str)
        and is_bittensor_address(p["hotkey"])
        and netuid in (None, p["netuid"])
        and hotkey in (None, p["hotkey"])
    ]
    if len(listed) == 1:
        return listed[0]
    if listed:
        raise ServiceError(
            f"the document at {url} lists {len(listed)} publishers; name one by its netuid "
            "and hotkey: " + ", ".join(f"{n} {h}" for n, h in listed),
            code="ambiguous",
        )
    which = " ".join(
        part
        for part in (
            f"netuid {netuid}" if netuid is not None else "",
            f"hotkey {hotkey}" if hotkey else "",
        )
        if part
    )
    raise ServiceError(
        f"the document at {url} does not list {which or 'a publisher'} in publishers"
    )


def _gamma_balance(document: Mapping[str, Any], url: str) -> str:
    payments = document.get("payments")
    for payment in payments if isinstance(payments, list) else []:
        if (
            isinstance(payment, dict)
            and payment.get("type") == "gamma"
            and payment.get("signature") == http_auth.PROTOCOL
        ):
            balance = payment.get("balance")
            if not isinstance(balance, str):
                raise ServiceError(f"the gamma payment in {url} has no balance url")
            return urllib.parse.urljoin(url, balance)
    raise ServiceError(f"the document at {url} has no gamma payment signed with btauth/1")


def _spec_urls(document: Mapping[str, Any], url: str) -> list[str]:
    """The urls of the OpenAPI specs the document names, each once."""
    specs: dict[str, None] = {}
    services = document.get("services")
    for service in services if isinstance(services, list) else []:
        if not isinstance(service, dict):
            continue
        if service.get("type") not in ("openapi", "http"):
            continue
        # An openapi service's endpoint is its spec unless it names the spec apart.
        spec = service.get("spec")
        if not isinstance(spec, str) and service.get("type") == "openapi":
            spec = service.get("endpoint")
        if isinstance(spec, str):
            specs[urllib.parse.urljoin(url, spec)] = None
            if len(specs) > MAX_SPECS:
                raise ServiceError(
                    f"the document at {url} names more than {MAX_SPECS} OpenAPI specs",
                    code="too_large",
                )
    if not specs:
        raise ServiceError(f"the document at {url} names no OpenAPI spec")
    return list(specs)


class _Unusable(Exception):
    """Why an operation cannot be offered."""


def _operations(
    spec: Mapping[str, Any], spec_url: str, local: bool, operations: dict[str, Operation]
) -> list[Skipped]:
    """Add the operations of ``spec`` to ``operations``, by names of their own among
    them, and return those it does not offer, and why."""
    version = spec.get("openapi")
    if isinstance(version, float):  # YAML reads an unquoted 3.1 as a number
        version = str(version)
    if not isinstance(version, str) or not version.startswith("3."):
        found = (
            f"swagger {spec['swagger']!r}"
            if "swagger" in spec
            else f"openapi {version!r}"
            if version is not None
            else "no openapi version"
        )
        raise ServiceError(f"{spec_url} is not an OpenAPI 3 spec ({found})")
    schemes = _get(spec, "components", "securitySchemes")
    schemes = schemes if isinstance(schemes, dict) else {}

    def btauth(requirement: Any) -> bool:
        """A requirement of btauth/1 headers alone, in one scheme or split over several."""
        if not isinstance(requirement, dict) or not requirement:
            return False
        names = [_btauth_header(_deref(spec, schemes.get(name))) for name in requirement]
        return http_auth.HEADER_SIGNATURE.lower() in names and all(names)

    def foreign(requirement: Any) -> bool:
        """A requirement only other credentials meet, as a bearer token or OAuth."""
        return (
            isinstance(requirement, dict)
            and bool(requirement)
            and all(
                _get(_deref(spec, schemes.get(name)), "type") in _FOREIGN_SCHEMES
                for name in requirement
            )
        )

    declared = any(btauth({name: []}) for name in schemes)
    root_security = spec.get("security")
    commands = {op.command for op in operations.values()}
    suffixes: dict[str, int] = {}
    skipped: list[Skipped] = []
    budget = [RESOLVE_BUDGET]  # schema nodes the bodies of the whole spec may take to read
    paths = spec.get("paths")
    for path, item in paths.items() if isinstance(paths, dict) else []:
        path = str(path)
        try:
            item = _deref(spec, item, strict=True)
        except ServiceError as error:
            skipped.append(Skipped("", _line(path), "", str(error)))
            continue
        if not isinstance(item, dict):
            continue
        for method in _METHODS:
            op = item.get(method)
            if not isinstance(op, dict):
                continue
            name = f"{method.upper()} {path}"
            op_id = op.get("operationId")
            op_id = op_id.strip() if isinstance(op_id, str) and _kebab(op_id) else name
            given = "" if op_id == name else _line(op_id)
            skip = functools.partial(Skipped, method.upper(), _line(path), given)
            try:
                route, fixed_query = _route(path)
                security = op.get("security", root_security)
                if not isinstance(security, list):  # malformed, so as if it said nothing
                    security = root_security if isinstance(root_security, list) else None
                requirements = security or []
                anonymous = not requirements or {} in requirements
                api_url = _checked_url(_server(spec_url, op, item, spec), "the API", local)
                inputs, body_input, body_type, body_kind, signs = _inputs(
                    spec,
                    [*_list(item.get("parameters")), *_list(op.get("parameters"))],
                    op,
                    budget,
                    route,
                )
                if any(btauth(r) for r in requirements):
                    auth = "optional" if anonymous else "hotkey"
                elif not declared:
                    # Without the scheme, every operation the spec secures is signed, as
                    # is one that takes the btauth/1 headers as parameters.
                    if signs is not None:
                        auth = "hotkey" if signs else "optional"
                    elif any(map(foreign, requirements)) and all(
                        r == {} or foreign(r) for r in requirements
                    ):
                        if not anonymous:
                            raise _Unusable("needs credentials other than btauth/1")
                        auth = "open"
                    else:
                        auth = "open" if security == [] else "optional" if anonymous else "hotkey"
                elif anonymous:
                    auth = "open"
                else:
                    raise _Unusable("needs credentials other than btauth/1")
            except (_Unusable, ServiceError) as reason:
                skipped.append(skip(str(reason)))
                continue
            except ValueError as error:
                skipped.append(skip(f"has a bad url: {_line(str(error))}"))
                continue
            except Exception:  # whatever else the spec gets wrong is this operation's
                skipped.append(skip("has a malformed definition"))
                continue
            # A name another operation has, or writes the same way, goes by its method
            # and path instead, so every operation is called by a name of its own.
            command = _kebab(op_id)
            if command in commands:
                base = _kebab(name)
                op_id, command = name, base
                if command in commands:
                    n = _free_suffix(base, "-", commands, suffixes)
                    op_id, command = f"{name} {n}", f"{base}-{n}"
            commands.add(command)
            operations[op_id] = Operation(
                id=op_id,
                method=method.upper(),
                path=path,
                auth=auth,
                summary=_text(op.get("summary")),
                description=_text(op.get("description")),
                inputs=inputs,
                api_url=api_url,
                body_input=body_input,
                body_required=body_input is not None and body_input.required,
                body_type=body_type,
                body_kind=body_kind,
                tags=tuple(tag for tag in _list(op.get("tags")) if isinstance(tag, str)),
                route=route,
                fixed_query=fixed_query,
                spec=spec,
            )
    return skipped


def _btauth_header(scheme: Any) -> str:
    """The lowercase header name of a btauth/1 ``apiKey`` scheme, else ""."""
    if isinstance(scheme, dict) and scheme.get("type") == "apiKey" and scheme.get("in") == "header":
        name = str(scheme.get("name", "")).lower()
        return name if name in _BTAUTH_HEADERS else ""
    return ""


def _server(spec_url: str, *nodes: Any) -> str:
    """The first server of the first of ``nodes`` that names any, with its variables
    at their defaults, as a url; ``/`` from the spec when none does, as OpenAPI says."""
    servers = next(
        (n["servers"] for n in nodes if isinstance(n, dict) and _list(n.get("servers"))),
        [{"url": "/"}],
    )
    first = servers[0] if isinstance(servers[0], dict) else {}
    url, variables = first.get("url"), first.get("variables")
    if not isinstance(url, str):
        raise ServiceError(f"{spec_url} has a server without a url")

    def default(match: re.Match) -> str:
        variable = _get(variables, match.group(1))
        value = _get(variable, "default")
        if not isinstance(value, str):
            choices = _list(_get(variable, "enum"))
            value = choices[0] if choices and isinstance(choices[0], str) else None
        if value is None:
            raise ServiceError(
                f"{spec_url} has a server variable {match.group(1)!r} with no default"
            )
        return value

    return urllib.parse.urljoin(spec_url, re.sub(r"\{([^{}]*)\}", default, url))


def _route(path: str) -> tuple[str, tuple[tuple[str, str], ...]]:
    """A spec's path as it is sent, and the query it fixes: what follows ``#`` is not
    sent, as in a url, and ``?`` starts a query of its own (``/?Action=List``)."""
    path, _, query = path.partition("#")[0].partition("?")
    if not path.startswith("/"):
        path = "/" + path
    return path, tuple(urllib.parse.parse_qsl(query, keep_blank_values=True))


def _list(value: Any) -> list:
    return value if isinstance(value, list) else []


def _inputs(
    spec, parameters: list, op: Mapping[str, Any], budget: list, route: str
) -> tuple[tuple[Input, ...], Optional[Input], str, str, Optional[bool]]:
    """An operation's inputs, each with a name of its own, its body as one input, the
    Content-Type and kind of its body, and whether it takes the btauth/1 headers as
    parameters (None when it does not, else whether it requires them)."""
    # Operation-level definitions override path-level parameters.
    definitions = {}
    for raw in parameters:
        param = _deref(spec, raw, strict=True)
        if isinstance(param, dict) and isinstance(param.get("name"), str):
            definitions[(param["name"], str(param.get("in")))] = param
    in_route = set(re.findall(r"\{([^{}]*)\}", route))
    params: list[Input] = []
    signs: Optional[bool] = None
    for (name, location), param in definitions.items():
        if location == "header" and name.lower() in _BTAUTH_HEADERS:
            signs = bool(signs) or bool(param.get("required"))
            continue
        if (
            location not in ("path", "query", "header", "cookie")
            or (location == "path" and name not in in_route)
            or (location in ("header", "cookie") and not _TOKEN.fullmatch(name))
            or (location == "header" and not _sendable_header(name))
        ):
            continue  # nowhere to send it, or a header the client writes itself
        content = param.get("content")
        media = next(iter(content), "") if isinstance(content, dict) and content else ""
        raw_schema = _get(content, media, "schema") if media else param.get("schema")
        schema = _deref(spec, raw_schema or {}, schema=True)
        style = param.get("style")
        if media:
            style = "json" if _is_json(str(media)) else "simple"
        elif not isinstance(style, str):
            style = "form" if location in ("query", "cookie") else "simple"
        explode = param.get("explode")
        params.append(
            Input(
                name=name,
                location=location,
                required=location == "path" or bool(param.get("required")),
                description=_text(param.get("description")),
                schema=schema if isinstance(schema, dict) else {},
                style=style,
                explode=explode if isinstance(explode, bool) else style == "form",
            )
        )
    declared = {item.name for item in params if item.location == "path"}
    for name in sorted(in_route - declared):  # a path parameter the spec forgot to declare
        params.append(Input(name, "path", True, "", {"type": "string"}, style="simple"))
    body = _deref(spec, op.get("requestBody"), strict=True)
    content = body.get("content") if isinstance(body, dict) else None
    body_required = isinstance(body, dict) and bool(body.get("required"))
    fields: list[Input] = []
    body_type, kind, body_input = "", "json", None
    if isinstance(content, dict) and content:
        key, body_type, kind = _media(content)
        media = content[key] if isinstance(content[key], dict) else {}
        schema = _deref(spec, media.get("schema", {}), schema=True)
        if schema is False:
            schema = {"not": {}}
        elif schema is True or not isinstance(schema, dict):
            schema = {}
        found = _object(spec, schema, budget) if kind != "raw" else None
        encoding = media.get("encoding") if isinstance(media.get("encoding"), dict) else {}
        properties, required = found if found and found[0] else ({}, set())
        path_inputs = {item.name: at for at, item in enumerate(params) if item.location == "path"}
        for name, raw in properties.items():
            prop = _deref(spec, raw, schema=True)
            prop = prop if isinstance(prop, dict) else {}
            if not isinstance(name, str) or prop.get("readOnly") is True:
                continue
            at = path_inputs.get(name)
            if kind == "json" and at is not None and name in required and params[at].schema == prop:
                # PUT /items/{id} with the id in the body too: one input fills both.
                params[at] = dataclasses.replace(params[at], in_body=True)
                continue
            how = encoding.get(name) if isinstance(encoding.get(name), dict) else {}
            style = how.get("style") if isinstance(how.get("style"), str) else "form"
            explode = how.get("explode")
            content_type = how.get("contentType")
            fields.append(
                Input(
                    name=name,
                    location="field",
                    required=body_required and name in required,
                    required_in_body=name in required,
                    description=_text(prop.get("description")),
                    schema=prop,
                    style="json"
                    if isinstance(content_type, str) and _is_json(content_type)
                    else style,
                    explode=explode if isinstance(explode, bool) else style == "form",
                    binary=kind == "multipart" and _is_binary(spec, prop),
                    content_type=content_type if isinstance(content_type, str) else "",
                )
            )
        binary = kind == "raw" and not (_is_text(body_type) and not _is_binary(spec, schema))
        if binary and not _is_binary(spec, schema):
            schema = {"type": "string", "format": "binary", **schema}
        body_input = Input(
            "body", "body", body_required, _text(body.get("description")), schema, binary=binary
        )
    inputs = _named([*params, *fields])
    if body_input is not None:
        names = {item.name for item in inputs}
        commands = {item.command for item in inputs}
        name = next(
            n
            for n in itertools.chain(["body"], (f"request_body_{i}" for i in itertools.count(2)))
            if n not in names and _kebab(n) not in commands
        )
        body_input = dataclasses.replace(body_input, name=name)
        if not fields:
            inputs = (*inputs, body_input)
    return tuple(inputs), body_input, body_type, kind, signs


def _named(inputs: list[Input]) -> list[Input]:
    """``inputs`` each with a name and a command name of its own. One that repeats an
    earlier name is renamed after where it is sent (``query_id``), and still sent by
    its own; one whose command name is taken, or has no letter or digit, gets one."""
    order = {"path": 0, "query": 1, "header": 2, "cookie": 3, "field": 4}
    names: set[str] = set()
    commands: set[str] = set()
    name_suffixes: dict[str, int] = {}
    command_suffixes: dict[str, int] = {}
    named = []
    for item in sorted(inputs, key=lambda i: order[i.location]):
        name = item.name
        if name in names:
            name = f"{item.location}_{item.name}"
            if name in names:
                name = f"{name}_{_free_suffix(name, '_', names, name_suffixes)}"
        command, flag = _kebab(name), ""
        if not command or command in commands:
            base = _kebab(f"{item.location} {name}")
            if base in commands:
                base = f"{base}-{_free_suffix(base, '-', commands, command_suffixes)}"
            flag = command = base
        names.add(name)
        commands.add(command)
        named.append(
            dataclasses.replace(item, name=name, wire=item.sent_as, flag=flag)
            if name != item.name or flag
            else item
        )
    return named


def _free_suffix(base: str, separator: str, taken: set[str], suffixes: dict[str, int]) -> int:
    n = suffixes.get(base, 2)
    while f"{base}{separator}{n}" in taken:
        n += 1
    suffixes[base] = n + 1
    return n


def _media(content: Mapping[str, Any]) -> tuple[str, str, str]:
    """Which of a body's media types to send, the Content-Type it goes as, and its kind:
    JSON first, then a form, then multipart, else the first as bytes or text."""
    kinds = [key for key in content if isinstance(key, str)]
    for kind, fits in (
        ("json", _is_json),
        ("form", lambda t: _base_type(t) == "application/x-www-form-urlencoded"),
        ("multipart", lambda t: _base_type(t).startswith("multipart/")),
    ):
        for key in kinds:
            if fits(key):
                sent = _base_type(key) if kind == "multipart" else key.strip()
                return key, sent if _header_safe(sent) else _JSON, kind
    key = kinds[0] if kinds else next(iter(content))
    sent = str(key).strip()
    if "*" in sent or "/" not in sent or not _header_safe(sent):
        sent = "application/octet-stream"
    return key, sent, "raw"


def _base_type(media_type: str) -> str:
    return media_type.split(";")[0].strip().lower()


def _header_safe(text: str) -> bool:
    return text.isprintable() and text.isascii()


def _is_text(media_type: str) -> bool:
    kind = _base_type(media_type)
    return (
        kind.startswith("text/")
        or _is_json(kind)
        or kind.endswith(("+xml", "/xml", "+yaml", "/yaml", "/x-yaml", "/csv", "ndjson", "jsonl"))
        or kind in ("application/javascript", "application/graphql", "application/sql")
    )


def _is_binary(spec, schema: Any) -> bool:
    """Whether a schema describes a file: ``format: binary``, a ``contentMediaType``
    without an encoding, or an array of them."""
    schema = _deref(spec, schema, schema=True)
    if not isinstance(schema, dict):
        return False
    if schema.get("format") == "binary":
        return True
    if "contentMediaType" in schema and "contentEncoding" not in schema:
        return True
    return schema.get("type") == "array" and _is_binary(spec, schema.get("items"))


def _object(spec, schema: Any, budget: list, depth: int = 0) -> Optional[tuple[dict, set]]:
    """The fields of an object schema and which are required, with ``allOf`` merged and
    a nullable ``anyOf`` or ``oneOf`` read as its one other choice; None when it is
    not an object, is one of several shapes, or takes more than ``budget[0]`` nodes."""
    schema = _deref(spec, schema, schema=True)
    budget[0] -= 1
    if not isinstance(schema, dict) or depth > RESOLVE_DEPTH or budget[0] < 0:
        return None
    kind = schema.get("type")
    if kind is not None and kind != "object" and not (isinstance(kind, list) and "object" in kind):
        return None
    own = schema.get("properties")
    properties = dict(own) if isinstance(own, dict) else {}
    names = schema.get("required")
    required = {n for n in names if isinstance(n, str)} if isinstance(names, list) else set()
    parts = list(_list(schema.get("allOf")))
    for key in ("anyOf", "oneOf"):
        choices = [c for c in _list(schema.get(key)) if _get(_deref(spec, c), "type") != "null"]
        if len(choices) > 1:
            return None
        parts.extend(choices)
    for part in parts:
        fields = _object(spec, part, budget, depth + 1)
        if fields is None:
            return None
        properties.update(fields[0])
        required |= fields[1]
    return properties, required


def _request(op: Operation, inputs: Mapping[str, Any], body: Any = _NOT_GIVEN):
    if not isinstance(inputs, Mapping):
        raise ServiceError(
            f"{op.id} takes its inputs as a mapping of names to values", code="input"
        )
    known = {i.name: i for i in op.inputs}
    if op.body_input is not None:
        known[op.body_input.name] = op.body_input
    by_command = {i.command: i.name for i in known.values()}
    given: dict[str, Any] = {}
    for name, value in inputs.items():
        name = name if name in known else by_command.get(_kebab(str(name)), name)
        if name not in known:
            close = difflib.get_close_matches(str(name), list(known), n=2)
            hint = f"; did you mean {', '.join(close)}?" if close else ""
            raise ServiceError(f"{op.id} has no input {name!r}{hint}", code="input")
        if name in given:
            raise ServiceError(f"{op.id} received {name!r} twice", code="input")
        given[name] = value
    inputs = given
    if op.body_input is not None and op.body_input.name in given:
        if body is not _NOT_GIVEN:
            raise ServiceError(f"{op.id} received its body twice", code="input")
        body = given.pop(op.body_input.name)
    raw = body is not _NOT_GIVEN
    body_inputs = {i.name for i in op.inputs if i.location in ("body", "field")}
    if raw and (not op.body_type or body_inputs.intersection(inputs)):
        raise ServiceError(f"{op.id} cannot take a separate body with these inputs", code="input")
    with_body = op.body_required or bool(body_inputs.intersection(inputs))
    required = [
        i
        for i in op.inputs
        if (i.required or (with_body and i.required_in_body))
        and not (raw and i.name in body_inputs)
    ]
    for item in required:
        if item.name not in inputs:
            for key in ("const", "default"):
                if key in item.schema:
                    inputs[item.name] = item.schema[key]
                    break
    missing = [i.name for i in required if inputs.get(i.name) is None and i.location != "field"]
    missing += [i.name for i in required if i.location == "field" and i.name not in inputs]
    if raw and isinstance(body, Mapping):  # a whole body needs the fields its schema requires
        missing += [
            i.sent_as
            for i in op.inputs
            if i.location == "field" and i.required_in_body and i.sent_as not in body
        ]
    if missing:
        raise ServiceError(f"{op.id} needs {', '.join(map(repr, missing))}", code="input")
    path = op.route or op.path
    query: list[tuple[str, str]] = list(op.fixed_query)
    headers = {"User-Agent": _USER_AGENT}
    cookies: list[str] = []
    fields: dict[str, Any] = {}
    whole: Any = body
    for item in op.inputs:
        if item.name not in inputs:
            continue
        value = inputs[item.name]
        if item.location == "field":
            fields[item.sent_as] = value
        elif item.location == "body":
            whole = value
        elif value is None:  # a parameter left out
            continue
        elif item.location == "path":
            text = _path_text(item, value)
            if text in ("", ".", ".."):  # each would reach another route
                raise ServiceError(f"{op.id} cannot take {text!r} as {item.name}", code="input")
            path = path.replace("{" + item.sent_as + "}", text)
            if item.in_body and with_body and not raw:
                fields[item.sent_as] = value
        elif item.location == "query":
            query.extend(_pairs(item.sent_as, item.style, item.explode, value))
        elif item.location == "header":
            headers[item.sent_as] = _header_text(item, _joined_text(item, value, ","))
        else:
            cookies.extend(
                f"{key}={_header_text(item, text)}" for key, text in _cookie_pairs(item, value)
            )
    if cookies:
        headers["Cookie"] = "; ".join(cookies)
    data = b""
    if whole is not _NOT_GIVEN or fields or op.body_required:
        data, content_type = _body(op, fields if whole is _NOT_GIVEN else whole)
        headers["Content-Type"] = content_type
    if "{" in path:
        raise ServiceError(f"{op.id} has a path parameter it does not declare")
    base = urllib.parse.urlsplit(op.api_url)
    full_path = urllib.parse.quote(base.path.rstrip("/") + path, safe="/%!$&'()*+,;=:@-._~")
    query_text = "&".join(
        part
        for part in (base.query, urllib.parse.urlencode(query, quote_via=urllib.parse.quote))
        if part
    )
    target = full_path + ("?" + query_text if query_text else "")
    url = urllib.parse.urlunsplit((base.scheme, base.netloc, full_path, query_text, ""))
    return op.method, url, target, headers, data


def _text_of(value: Any) -> str:
    """A value as the text of a parameter: ``true`` and ``false`` for booleans, and
    JSON for an object or a list within another."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None:
        return ""
    if isinstance(value, (dict, list, tuple)):
        return _json_text(value)
    return str(value)


def _json_text(value: Any) -> str:
    try:
        return json.dumps(value, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    except (TypeError, ValueError) as error:
        raise ServiceError(f"cannot send {value!r} as JSON: {error}", code="input") from None


def _entries(value: Any) -> list[str]:
    """An object as its names and values in turn, a list as its items, else the value."""
    if isinstance(value, Mapping):
        return [_text_of(x) for pair in value.items() for x in pair]
    if isinstance(value, (list, tuple)):
        return [_text_of(v) for v in value]
    return [_text_of(value)]


def _joined_text(item: Input, value: Any, separator: str) -> str:
    """A header's or a simple path parameter's text: ``k=v,k2=v2`` for an exploded
    object, else its entries joined."""
    if item.style == "json":
        return _json_text(value)
    if isinstance(value, Mapping) and item.explode:
        return separator.join(f"{_text_of(k)}={_text_of(v)}" for k, v in value.items())
    return separator.join(_entries(value))


def _path_text(item: Input, value: Any) -> str:
    """A path parameter as its style writes it, each part percent-encoded."""

    def quoted(text: Any) -> str:
        return urllib.parse.quote(_text_of(text), safe="")

    if item.style == "json":
        return quoted(_json_text(value))
    if isinstance(value, Mapping):
        pairs = [(quoted(k), quoted(v)) for k, v in value.items()]
        parts = [f"{k}={v}" for k, v in pairs] if item.explode else [x for p in pairs for x in p]
    elif isinstance(value, (list, tuple)):
        parts = [quoted(v) for v in value]
    else:
        parts = [quoted(value)]
    if item.style == "label":
        return "." + ("." if item.explode else ",").join(parts)
    if item.style == "matrix":
        name = urllib.parse.quote(item.sent_as, safe="")
        if item.explode and isinstance(value, Mapping):
            return "".join(f";{part}" for part in parts)
        if item.explode and isinstance(value, (list, tuple)):
            return "".join(f";{name}={part}" for part in parts)
        joined = ",".join(parts)
        return f";{name}={joined}" if joined else f";{name}"
    return ",".join(parts)


def _pairs(name: str, style: str, explode: bool, value: Any) -> list[tuple[str, str]]:
    """A query parameter or a form field as name and value pairs, by its style."""
    if style == "json":
        return [(name, _json_text(value))]
    if isinstance(value, Mapping):
        if style == "deepObject":
            return _deep(name, value)
        if explode and style == "form":
            return [(str(k), _text_of(v)) for k, v in value.items() if v is not None]
    elif isinstance(value, (list, tuple)):
        if explode or style == "deepObject":
            return [(name, _text_of(v)) for v in value]
    else:
        return [(name, _text_of(value))]
    separator = {"spaceDelimited": " ", "pipeDelimited": "|"}.get(style, ",")
    return [(name, separator.join(_entries(value)))]


def _deep(name: str, value: Mapping) -> list[tuple[str, str]]:
    """An object as ``name[key]=value`` pairs, nested objects by more keys."""
    pairs: list[tuple[str, str]] = []
    for key, item in value.items():
        if item is None:
            continue
        inner = f"{name}[{key}]"
        if isinstance(item, Mapping):
            pairs.extend(_deep(inner, item))
        elif isinstance(item, (list, tuple)):
            pairs.extend((inner, _text_of(v)) for v in item)
        else:
            pairs.append((inner, _text_of(item)))
    return pairs


def _cookie_pairs(item: Input, value: Any) -> list[tuple[str, str]]:
    if item.style != "json" and item.explode:
        if isinstance(value, Mapping):
            return [(str(k), _text_of(v)) for k, v in value.items()]
        if isinstance(value, (list, tuple)):
            return [(item.sent_as, _text_of(v)) for v in value]
    return [(item.sent_as, _joined_text(item, value, ","))]


def _header_text(item: Input, text: str) -> str:
    if not text.isprintable():
        raise ServiceError(f"{item.name} cannot hold control characters", code="input")
    try:
        text.encode("latin-1")
    except UnicodeEncodeError:
        raise ServiceError(
            f"{item.name} is a header, which holds Latin-1 text only", code="input"
        ) from None
    return text


def _body(op: Operation, value: Any) -> tuple[bytes, str]:
    """A request body as its operation's kind of body is written, and its Content-Type."""
    if op.body_kind == "raw":
        item = op.body_input
        if item is not None and item.binary:
            return _file(op, item.name, value)[1], op.body_type
        if isinstance(value, (bytes, bytearray, memoryview)):
            return bytes(value), op.body_type
        return ("" if value is None else _text_of(value)).encode(), op.body_type
    if op.body_kind in ("form", "multipart"):
        if not isinstance(value, Mapping):
            raise ServiceError(f"{op.id} takes its body as an object of fields", code="input")
        if op.body_kind == "form":
            items = {i.sent_as: i for i in op.inputs if i.location == "field"}
            pairs = []
            for key, item in value.items():
                spec = items.get(key)
                if item is not None:
                    style = spec.style if spec is not None else "form"
                    explode = spec.explode if spec is not None else True
                    pairs.extend(_pairs(str(key), style, explode, item))
            data = urllib.parse.urlencode(pairs, quote_via=urllib.parse.quote)
            return data.encode(), op.body_type
        return _multipart(op, value)
    return _json_text(value).encode(), op.body_type


def _multipart(op: Operation, value: Mapping) -> tuple[bytes, str]:
    """``multipart/form-data``: a part for each field, and for each item of a list."""
    items = {i.sent_as: i for i in op.inputs if i.location == "field"}
    boundary = secrets.token_hex(16)
    chunks: list[bytes] = []
    for key, field_value in value.items():
        if field_value is None:
            continue
        spec = items.get(key)
        binary = spec is not None and spec.binary
        parts = (
            field_value
            if isinstance(field_value, list) or (isinstance(field_value, tuple) and not binary)
            else [field_value]
        )
        for part in parts:
            disposition = f'form-data; name="{_quoted(str(key))}"'
            kind = _concrete(spec.content_type) if spec is not None else ""
            if binary:
                filename, data = _file(op, str(key), part)
                disposition += f'; filename="{_quoted(filename)}"'
                kind = kind or mimetypes.guess_type(filename)[0] or "application/octet-stream"
            elif isinstance(part, (bytes, bytearray, memoryview)):
                data, kind = bytes(part), kind or "application/octet-stream"
            elif isinstance(part, (dict, list, tuple)) or (
                spec is not None and spec.style == "json"
            ):
                data, kind = _json_text(part).encode(), kind or _JSON
            else:
                data = _text_of(part).encode()
            head = f"--{boundary}\r\nContent-Disposition: {disposition}\r\n"
            if kind:
                head += f"Content-Type: {kind}\r\n"
            chunks += [head.encode() + b"\r\n", data, b"\r\n"]
    chunks.append(f"--{boundary}--\r\n".encode())
    return b"".join(chunks), f"{op.body_type}; boundary={boundary}"


def _quoted(name: str) -> str:
    """A multipart name or filename within its quotes, as browsers write one."""
    return name.replace('"', "%22").replace("\r", "%0D").replace("\n", "%0A")


def _concrete(media_types: str) -> str:
    """The first concrete media type of a list of them, or ""."""
    for kind in media_types.split(","):
        kind = kind.strip()
        if kind and "*" not in kind and _header_safe(kind):
            return kind
    return ""


def _file(op: Operation, name: str, value: Any) -> tuple[str, bytes]:
    """A file input as its filename and bytes: bytes, a binary file, base64 text, or a
    ``(filename, any of those)`` pair."""
    filename = name
    if isinstance(value, tuple) and len(value) == 2 and isinstance(value[0], str):
        filename, value = value
    elif isinstance(getattr(value, "name", None), str):
        filename = os.path.basename(value.name) or name
    if hasattr(value, "read"):
        value = value.read()
    if isinstance(value, str):
        try:
            value = base64.b64decode(value, validate=True)
        except (binascii.Error, ValueError):
            raise ServiceError(
                f"{op.id} takes {name} as a file: bytes, or base64 text", code="input"
            ) from None
    if not isinstance(value, (bytes, bytearray, memoryview)):
        raise ServiceError(f"{op.id} takes {name} as a file: bytes, or base64 text", code="input")
    return filename, bytes(value)


def _sendable_header(name: str) -> bool:
    lowered = name.lower()
    return lowered not in _RESERVED_HEADERS and not lowered.startswith("x-bittensor-")


def _is_json(media_type: str) -> bool:
    kind = media_type.split(";")[0].strip().lower()
    return kind == _JSON or kind.endswith("+json")


def _not_json(name: str) -> Any:
    raise ValueError(f"{name} is not JSON")


def _kebab(name: str) -> str:
    """``name`` as a command line writes it: ``rentBySpec`` and ``rent_by_spec`` are
    ``rent-by-spec``, ``GET /v1/models`` is ``get-v1-models``."""
    name = re.sub(r"([a-z0-9])([A-Z])", r"\1-\2", name)
    return re.sub(r"[^A-Za-z0-9]+", "-", name).strip("-").lower()


def _deref(
    spec: Mapping[str, Any], node: Any, *, schema: bool = False, strict: bool = False
) -> Any:
    """``node``, or the target of the local ``$ref`` it is, followed to the end. A
    reference to nothing is an empty object, one that says nothing, unless ``strict``,
    and one to another file is left as it is, unless ``strict``."""
    seen = set()
    while isinstance(node, dict) and isinstance(node.get("$ref"), str):
        ref = node["$ref"]
        if not ref.startswith("#/") and strict:
            raise ServiceError(f"the spec refers to {ref!r} in another file; serve it as one")
        if ref in seen or not ref.startswith("#/"):
            return node
        seen.add(ref)
        if schema and len(node) > 1 and not str(spec.get("openapi", "")).startswith("3.0."):
            # In 3.1 the sibling constraints apply together with the target schema.
            return {"allOf": [{"$ref": ref}, {k: v for k, v in node.items() if k != "$ref"}]}
        try:
            node = _pointer(spec, ref)
        except ServiceError:
            if strict:
                raise
            return {}
    return node


def _is_object(spec: Mapping[str, Any], schema: Any) -> bool:
    """Whether ``schema``, or a schema it takes all of, describes an object. Reads at
    most 256 schemas, as ``allOf`` branches can repeat through references."""
    pending, read = [schema], 0
    while pending and read < 256:
        node = _deref(spec, pending.pop())
        read += 1
        if not isinstance(node, dict):
            continue
        kind = node.get("type")
        if kind == "object" or (isinstance(kind, list) and "object" in kind):
            return True
        if "properties" in node:
            return True
        pending.extend(_list(node.get("allOf")))
    return False


def _definition_name(ref: str) -> str:
    """A tool's $defs name for a reference: a component schema's own name, else a hash."""
    prefix = "#/components/schemas/"
    name = ref[len(prefix) :]
    if ref.startswith(prefix) and re.fullmatch(r"[A-Za-z0-9._-]{1,64}", name):
        return name
    return "ref-" + hashlib.sha256(ref.encode()).hexdigest()[:12]


def _pointer(spec: Mapping[str, Any], ref: str) -> Any:
    target: Any = spec
    # A reference is a url fragment, so its pointer may be percent-encoded (RFC 6901).
    for part in urllib.parse.unquote(ref[2:]).split("/"):
        part = part.replace("~1", "/").replace("~0", "~")
        if (
            isinstance(target, list)
            and part.isascii()
            and part.isdigit()
            and int(part) < len(target)
        ):
            target = target[int(part)]
        else:
            target = target.get(part) if isinstance(target, dict) else None
    if target is None:
        raise ServiceError(f"the spec has a broken reference {ref!r}")
    return target


def _node_cost(node: Any) -> int:
    return 1 + (len(node) >> 6 if isinstance(node, str) else 0)


def _resolve(spec: Mapping[str, Any], node: Any, budget: list, *, strict: bool = False) -> Any:
    """``node`` with local ``$ref`` replaced by their targets, spending ``budget[0]`` nodes,
    and long strings as several, so nothing grows past the budget.

    A cyclic or broken ref, and every ref past the budget or depth, stays a ref.
    """

    def walk(node: Any, depth: int, seen: frozenset) -> Any:
        budget[0] -= _node_cost(node)
        if budget[0] < 0 and strict:
            raise ServiceError(
                "the input schema is too large to expand; use --help --json for its references",
                code="too_large",
            )
        if budget[0] < 0 or depth > RESOLVE_DEPTH:
            return node
        if isinstance(node, dict):
            ref = node.get("$ref")
            if isinstance(ref, str):
                if ref in seen or not ref.startswith("#/"):
                    return node
                try:
                    target = _pointer(spec, ref)
                except ServiceError:
                    return node
                resolved = walk(target, depth + 1, seen | {ref})
                if len(node) > 1 and not str(spec.get("openapi", "")).startswith("3.0."):
                    siblings = {k: v for k, v in node.items() if k != "$ref"}
                    return {"allOf": [resolved, walk(siblings, depth + 1, seen)]}
                return resolved
            return {key: walk(value, depth + 1, seen) for key, value in node.items()}
        if isinstance(node, list):
            return [walk(value, depth + 1, seen) for value in node]
        return node

    return walk(node, 0, frozenset())


def _get(node: Any, *keys: str) -> Any:
    for key in keys:
        node = node.get(key) if isinstance(node, dict) else None
    return node


def _text(value: Any) -> str:
    return value.strip() if isinstance(value, str) else ""


def _line(text: str) -> str:
    """``text`` as one printable line."""
    return " ".join("".join(c for c in text if c.isprintable() or c.isspace()).split())


def _checked_url(url: str, what: str, local: bool = False) -> str:
    """``url`` if it may be fetched: https to a public host, or, when ``local``
    (the chain is on this machine), also http and private hosts."""
    if any(ch.isspace() or not ch.isprintable() for ch in url):
        raise ServiceError(f"{what} url {url!r} is not a plain url")
    split = urllib.parse.urlsplit(url)
    host = split.hostname or ""
    if split.username is not None or split.password is not None or not host:
        raise ServiceError(f"{what} url {url!r} is not a plain url")
    if not local and _is_private(host):
        raise ServiceError(f"{what} url {url!r} is not a public host")
    if split.scheme == "https" or (local and split.scheme == "http"):
        return url
    raise ServiceError(f"{what} url {url!r} must be https")


def _is_local(url: str) -> bool:
    """Whether a url is on this machine, as a localnet's chain endpoint is."""
    host = urllib.parse.urlsplit(url).hostname or ""
    if host == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def _is_private(host: str) -> bool:
    if host == "localhost" or host.endswith(".localhost"):
        return True
    try:
        return not _is_global(host)
    except ValueError:
        return False


class _Redirects(urllib.request.HTTPRedirectHandler):
    """Follow a redirect only for an unsigned GET, and only to an allowed url.

    A signature covers the exact request target, so a signed request is never
    sent anywhere else, and a request with a body is never resent without it.
    """

    max_redirections = 5

    def __init__(self, local: bool):
        super().__init__()
        self.local = local

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if (
            req.get_method() != "GET"
            or req.data is not None
            or req.get_header(http_auth.HEADER_SIGNATURE.capitalize())
        ):
            return None
        _checked_url(newurl, "a redirect", self.local)
        new = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new is not None and (new.type, new.host) != (req.type, req.host):
            # A header input, such as an API key, is for the host it was sent to.
            new.headers = {k: v for k, v in new.headers.items() if k in ("User-agent", "Accept")}
        return new


_local = threading.local()


class _Stop:
    """Stops the requests of one awaited call when the call is cancelled: a thread
    cannot be cancelled, but its sockets can be shut."""

    def __init__(self) -> None:
        self.stopped = False
        self.deadline: Optional[_Deadline] = None

    def stop(self) -> None:
        self.stopped = True
        if self.deadline is not None:
            self.deadline.shut()


async def _threaded(work, *args):
    """``work(*args)`` in a thread, its requests stopped when the await is cancelled."""
    stop = _Stop()

    def run():
        _local.stop = stop
        try:
            return work(*args)
        finally:
            _local.stop = None

    try:
        return await asyncio.to_thread(run)
    except asyncio.CancelledError:
        stop.stop()
        raise


class _Deadline:
    """Shuts a request's sockets when its time is up: a socket's timeout bounds
    each read, and a host that trickles bytes would never trip it."""

    def __init__(self, seconds: float):
        self.sockets: list[socket.socket] = []
        self.passed = False
        self._end = time.monotonic() + seconds
        self._timer = threading.Timer(seconds, self.shut)
        self._timer.daemon = True

    def left(self) -> float:
        return 0.0 if self.passed else max(0.0, self._end - time.monotonic())

    def watch(self, sock: socket.socket) -> None:
        # A duplicate: wrapping a socket in TLS detaches it, and shutting one shuts both.
        self.sockets.append(sock.dup())
        if self.passed:
            self.shut()

    def lift(self) -> None:
        """No end for the rest: a body written as it arrives may take as long as it
        keeps arriving, each read still waiting at most the timeout."""
        self._timer.cancel()

    def shut(self) -> None:
        self.passed = True
        for sock in self.sockets:
            with contextlib.suppress(OSError):
                sock.shutdown(socket.SHUT_RDWR)

    def __enter__(self) -> _Deadline:
        _local.deadline = self
        stop = getattr(_local, "stop", None)
        if stop is not None:
            stop.deadline = self
            if stop.stopped:
                self.passed = True
        self._timer.start()
        return self

    def __exit__(self, *exc: Any) -> None:
        self._timer.cancel()
        _local.deadline = None
        for sock in self.sockets:
            sock.close()


class _Connection(http.client.HTTPConnection):
    """Watched by its request's deadline, which also bounds connecting, and refusing
    to talk to a private address, whatever name resolved to it, unless the chain is
    a localnet on this machine."""

    public = True

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._create_connection = self._connect

    def _connect(self, address, timeout=None, source_address=None, *args, **kwargs):
        deadline = getattr(_local, "deadline", None)
        timeout = timeout if isinstance(timeout, (int, float)) else None
        host, port = address
        error: Exception = OSError(f"{host} has no address")
        for family, kind, proto, _, peer in _lookup(host, port, deadline):
            if self.public and not _is_global(peer[0]):
                error = ServiceError(f"{self.host} is not a public host")
                continue
            left = deadline.left() if deadline is not None else timeout
            if left is not None and left <= 0:
                raise TimeoutError(f"connecting to {host} took too long")
            sock = socket.socket(family, kind, proto)
            if deadline is not None:
                deadline.watch(sock)
            try:
                sock.settimeout(min(left, timeout) if timeout and left is not None else left)
                if source_address:
                    sock.bind(source_address)
                sock.connect(peer)
            except OSError as failed:
                sock.close()
                error = failed
                continue
            sock.settimeout(timeout)
            return sock
        raise error


# A lookup past its deadline keeps its thread until the system resolver gives up. This
# bounds how many can pile up; it is well above the 8 lookups `discover` runs at once,
# so a few stalled hosts do not hold up the others.
_resolver_slots = threading.BoundedSemaphore(64)


def _lookup(host: str, port: Any, deadline: Optional[_Deadline]) -> list:
    """The addresses of ``host``, looked up within the deadline: the lookup itself
    cannot be stopped, so it runs on in its own thread when time is up."""
    if deadline is None:
        return socket.getaddrinfo(host, port, 0, socket.SOCK_STREAM)
    found: list = []

    def look_up() -> None:
        try:
            found.append(socket.getaddrinfo(host, port, 0, socket.SOCK_STREAM))
        except OSError as error:
            found.append(error)
        finally:
            _resolver_slots.release()

    worker = threading.Thread(target=look_up, daemon=True)
    while not _resolver_slots.acquire(timeout=min(deadline.left(), 0.05)):
        if deadline.left() <= 0:
            raise TimeoutError(f"looking up {host} took too long")
    try:
        worker.start()
    except RuntimeError:
        _resolver_slots.release()
        raise
    while worker.is_alive() and deadline.left() > 0:
        worker.join(min(deadline.left(), 0.05))
    if not found:
        raise TimeoutError(f"looking up {host} took too long")
    if isinstance(found[0], OSError):
        raise found[0]
    return found[0]


_NAT64 = ipaddress.ip_network("64:ff9b::/96")


def _is_global(address: Any) -> bool:
    """Whether an address is a public one, as is the IPv4 address an IPv6 one stands for."""
    ip = ipaddress.ip_address(str(address).split("%")[0])
    if ip.version == 6:
        if ip.ipv4_mapped is not None:
            ip = ip.ipv4_mapped
        elif ip in _NAT64:
            ip = ipaddress.ip_address(int(ip) & 0xFFFFFFFF)
    return ip.is_global and not ip.is_multicast


class _TLSConnection(_Connection, http.client.HTTPSConnection):
    pass


class _LocalConnection(_Connection):
    public = False


class _LocalTLSConnection(_LocalConnection, http.client.HTTPSConnection):
    pass


class _HTTP(urllib.request.HTTPHandler):
    def __init__(self, connection: type[http.client.HTTPConnection]):
        super().__init__()
        self._connection = connection

    def http_open(self, req):
        return self.do_open(self._connection, req)


class _HTTPS(urllib.request.HTTPSHandler):
    def __init__(self, connection: type[http.client.HTTPSConnection]):
        self._tls = ssl.create_default_context()
        super().__init__(context=self._tls)
        self._connection = connection

    def https_open(self, req):
        return self.do_open(self._connection, req, context=self._tls)


@functools.lru_cache(maxsize=2)
def _opener(local: bool) -> urllib.request.OpenerDirector:
    # Connect to the address we checked. A proxy would resolve the name again.
    return urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        _Redirects(local),
        _HTTP(_LocalConnection if local else _Connection),
        _HTTPS(_LocalTLSConnection if local else _TLSConnection),
    )


def _check_timeout(timeout: float) -> None:
    if type(timeout) not in (int, float) or not 0 < timeout < threading.TIMEOUT_MAX:
        raise ServiceError(
            f"timeout must be positive and below {threading.TIMEOUT_MAX:g} seconds", code="input"
        )


def _send(
    method: str,
    url: str,
    headers: Mapping[str, str],
    data: bytes,
    local: bool = False,
    limit: int = MAX_RESPONSE_BYTES,
    timeout: float = TIMEOUT,
    into: Optional[Callable[[Response], Optional[IO[bytes]]]] = None,
) -> Response:
    _check_timeout(timeout)
    request = urllib.request.Request(url, data=data or None, headers=dict(headers), method=method)
    with _Deadline(timeout) as deadline:
        try:
            try:
                reply = _opener(local).open(request, timeout=timeout)
            except urllib.error.HTTPError as error:
                reply = error
            with reply:
                status = reply.getcode() or 0
                fields = _headers(reply.headers)
                body = b""
                if method != "HEAD" and status not in (204, 304):
                    head = Response(status, fields, b"", reply.geturl())
                    sink = into(head) if into is not None and head.ok else None
                    if sink is None:
                        body = _read(reply, limit, url, fields)
                    else:
                        deadline.lift()
                        _stream(reply, sink, url, fields)
                response = Response(status, fields, body, reply.geturl())
            if deadline.passed:  # a body that ends with the connection may be cut off
                raise ServiceError(f"{url} did not answer within {timeout:g} s", code="timeout")
            return response
        except (
            ServiceError,
            urllib.error.URLError,
            http.client.HTTPException,
            OSError,
            ValueError,
        ) as error:
            reason = getattr(error, "reason", None) or error
            if deadline.passed or isinstance(reason, TimeoutError):
                raise ServiceError(
                    f"{url} did not answer within {timeout:g} s", code="timeout"
                ) from None
            if isinstance(error, ServiceError):
                raise
            raise ServiceError(f"could not reach {url}: {reason}", code="unreachable") from None


def _get_json(
    url: str,
    what: str,
    local: bool = False,
    cache: CacheDir = None,
    timeout: float = FETCH_TIMEOUT,
    limit: int = MAX_DOCUMENT_BYTES,
    *,
    spec_tokens: Optional[list[int]] = None,
) -> tuple[dict, str, int]:
    """The JSON object at ``url``, or the YAML one when ``yaml``, and where it was."""
    checked = _checked_url(url, what, local)
    if local:
        cache = None
    cached = _cached(cache, checked, limit)
    if cached is not None:
        try:
            response = Response(200, {"content-type": _JSON}, cached)
            return _json_object(response, url, what, spec_tokens), checked, len(cached)
        except ServiceError as error:
            if error.code == "too_large":
                raise
            # A damaged cache entry is a miss.
    accept = (
        f"{_JSON}, application/yaml;q=0.9, text/yaml;q=0.9" if spec_tokens is not None else _JSON
    )
    headers = {"User-Agent": _USER_AGENT, "Accept": accept, "Accept-Encoding": "gzip"}
    response = _gunzipped(_send("GET", checked, headers, b"", local, limit, timeout), url, limit)
    if len(response.content) > limit:
        raise ServiceError(f"{what} at {url} exceeds the remaining byte limit", code="too_large")
    document = _json_object(response, url, what, spec_tokens)
    effective = response.url or checked
    if effective == checked:
        _keep(cache, checked, response, limit)
    return document, effective, len(response.content)


def _gunzipped(response: Response, url: str, limit: int) -> Response:
    if response.headers.get("content-encoding", "").strip().lower() != "gzip":
        return response
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(response.content)) as stream:
            content = stream.read(limit + 1)
    except (OSError, EOFError, zlib.error):
        raise ServiceError(f"the response from {url} is not valid gzip") from None
    if len(content) > limit:
        raise ServiceError(f"the response from {url} is over {limit >> 20} MiB", code="too_large")
    headers = {k: v for k, v in response.headers.items() if k != "content-encoding"}
    return Response(response.status, headers, content, response.url)


def _max_age(headers: Mapping[str, str]) -> int:
    """Seconds a response may be kept, by its Cache-Control and Age, up to CACHE_MAX_SECONDS."""
    directives = urllib.request.parse_http_list(headers.get("cache-control", "").lower())
    ages = []
    for directive in directives:
        name, _, value = directive.partition("=")
        if name.strip() in ("no-store", "no-cache"):
            return 0
        if name.strip() == "max-age":
            match = re.fullmatch(r'([0-9]{1,18})|"([0-9]{1,18})"', value.strip())
            if not match:
                return 0
            ages.append(match[1] or match[2])
    age = headers.get("age", "0").strip()
    if len(ages) != 1 or not re.fullmatch(r"[0-9]{1,18}", age):
        return 0
    elapsed = int(age)
    if "date" in headers:
        try:
            stamp = parsedate_to_datetime(headers["date"])
            if stamp.tzinfo is None:
                return 0
            elapsed = max(elapsed, int(time.time() - stamp.timestamp()))
        except (OverflowError, OSError, TypeError, ValueError):
            return 0
    return min(CACHE_MAX_SECONDS, max(0, int(ages[0]) - elapsed))


def _cache_key(url: str) -> bytes:
    return hashlib.sha256(url.encode()).hexdigest().encode("ascii")


def _cache_file(cache: Union[str, Path], key: bytes) -> Path:
    return Path(cache) / f"{key.decode('ascii')}.cache"


def _sweep(cache: Union[str, Path]) -> None:
    """Remove the least recently written entries past CACHE_MAX_BYTES."""
    entries = []
    for path in Path(cache).glob("*.cache"):
        with contextlib.suppress(OSError):
            stat = path.stat()
            entries.append((stat.st_mtime, stat.st_size, path))
    total = sum(size for _, size, _ in entries)
    for _, size, path in sorted(entries):
        if total <= CACHE_MAX_BYTES:
            break
        with contextlib.suppress(OSError):
            path.unlink()
            total -= size


def _cached(cache: CacheDir, url: str, limit: int = MAX_SPEC_BYTES) -> Optional[bytes]:
    if cache is None:
        return None
    key = _cache_key(url)
    try:
        with _cache_file(cache, key).open("rb") as stream:
            identity, expires = stream.readline(96).rstrip(b"\n").split(b" ")
            now = time.time()
            if identity != key or not now < float(expires) <= now + CACHE_MAX_SECONDS:
                return None
            content = stream.read(limit + 1)
    except (OSError, ValueError):
        return None
    return content if len(content) <= limit else None


def _keep(cache: CacheDir, url: str, response: Response, limit: int = MAX_SPEC_BYTES) -> None:
    if cache is None or any(v.strip() == "*" for v in response.headers.get("vary", "").split(",")):
        return
    seconds = _max_age(response.headers)
    if not seconds or len(response.content) > limit:
        return
    key = _cache_key(url)
    path = _cache_file(cache, key)
    header = key + f" {time.time() + seconds}\n".encode("ascii")
    with contextlib.suppress(OSError):  # a cache it cannot write is only slower
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as partial:
            try:
                partial.write(header)
                partial.write(response.content)
                partial.close()
                os.replace(partial.name, path)
            finally:
                Path(partial.name).unlink(missing_ok=True)
        _sweep(path.parent)


_JSON_TOKEN = re.compile(rb'["\[{,:]')
_JSON_STRING_TOKEN = re.compile(rb'\\[\s\S]|"')


def _json_tokens(content: bytes, limit: int, url: str) -> int:
    """Count container and entry delimiters before JSON can allocate their objects."""
    if b"\0" in content[:4]:
        content = content.decode(json.detect_encoding(content)).encode()
    if not re.match(rb"(?:\xef\xbb\xbf)?\s*[\[{]", content):
        return 0
    count = cursor = 0
    while token := _JSON_TOKEN.search(content, cursor):
        cursor = token.end()
        if token[0] == b'"':
            for end in _JSON_STRING_TOKEN.finditer(content, cursor):
                cursor = end.end()
                if end[0] == b'"':
                    break
            else:
                break  # the JSON decoder reports the unfinished string
        else:
            count += 1
            if count > limit:
                raise ServiceError(
                    f"the OpenAPI specs at {url} have too many entries", code="too_large"
                )
    return count


def _json_object(
    response: Response, url: str, what: str, spec_tokens: Optional[list[int]] = None
) -> dict:
    """A response as the JSON object it is, or, when ``yaml``, the YAML one."""
    if not response.ok:
        message = response.message[:200]
        raise ServiceError(
            f"{what} at {url} answered HTTP {response.status}"
            + (f": {message}" if message else ""),
            status=response.status,
            code="http",
        )
    try:
        count = (
            _json_tokens(response.content, spec_tokens[0], url) if spec_tokens is not None else 0
        )
        value = response.json()
    except (ValueError, RecursionError):
        if spec_tokens is None:
            raise ServiceError(f"{what} at {url} is not JSON") from None
        value, count = _yaml(response.text, url, what, spec_tokens[0])
    if not isinstance(value, dict):
        raise ServiceError(f"{what} at {url} is not a JSON object")
    if spec_tokens is not None:
        spec_tokens[0] -= count
    return value


# Bound parser work and the values and text after aliases are expanded.
MAX_YAML_EVENTS = 4_500_000
MAX_YAML_NODES = 4_000_000


def _yaml(text: str, url: str, what: str, event_limit: int) -> tuple[Any, int]:
    """The YAML at ``url`` as JSON would read it: scalar keys become text, as an
    unquoted status code ``200:`` is, and values JSON has no type for are refused."""
    import yaml

    # The pure Python parser takes time quadratic in nesting depth, so a few
    # kilobytes of brackets would hold it for minutes; libyaml does not.
    if not yaml.__with_libyaml__:
        raise ServiceError(
            f"{what} at {url} is YAML, which needs PyYAML built with libyaml: reinstall "
            "PyYAML from its wheel, or ask the publisher to serve JSON"
        )
    loader = _yaml_loader()
    try:
        # Charge an alias the cost of its anchor without walking that tree again.
        # Only completed anchors can be used, so recursive values fail here.
        limit = min(MAX_YAML_EVENTS, event_limit)
        node_limit = min(MAX_YAML_NODES, event_limit)
        anchors: dict[str, int] = {}
        containers: list[tuple[Optional[str], int]] = [(None, 0)]
        for count, event in enumerate(yaml.parse(text, Loader=loader), 1):
            if count > limit:
                raise ServiceError(
                    f"{what} at {url} has more than {limit} YAML events", code="too_large"
                )
            if isinstance(event, yaml.CollectionStartEvent):
                if event.anchor is not None:
                    anchors.pop(event.anchor, None)
                containers.append((event.anchor, 1))
                continue
            if isinstance(event, yaml.CollectionEndEvent):
                anchor, cost = containers.pop()
            elif isinstance(event, yaml.ScalarEvent):
                anchor, cost = event.anchor, _node_cost(event.value)
            elif isinstance(event, yaml.AliasEvent):
                if event.anchor not in anchors:
                    if any(anchor == event.anchor for anchor, _ in containers):
                        raise ServiceError(
                            f"{what} at {url} has a recursive YAML alias and expands past "
                            f"{node_limit} nodes",
                            code="too_large",
                        )
                    raise ServiceError(f"{what} at {url} has an undefined YAML alias")
                anchor, cost = None, anchors[event.anchor]
            else:
                continue
            if anchor is not None:
                anchors[anchor] = cost
            parent, size = containers[-1]
            if size + cost > node_limit:
                raise ServiceError(
                    f"{what} at {url} expands past {node_limit} nodes", code="too_large"
                )
            containers[-1] = (parent, size + cost)
        value = yaml.load(text, Loader=loader)
    except (yaml.YAMLError, RecursionError, ValueError):
        raise ServiceError(f"{what} at {url} is neither JSON nor YAML") from None
    pending = [value]
    while pending:
        node = pending.pop()
        if isinstance(node, dict):
            if not all(isinstance(key, str) for key in node):
                items = list(node.items())
                node.clear()
                node.update((_yaml_key(key, url, what), item) for key, item in items)
            pending.extend(node.values())
        elif isinstance(node, list):
            pending.extend(node)
        elif not isinstance(node, (str, int, float, type(None))):
            raise ServiceError(f"{what} at {url} holds a {type(node).__name__}, which JSON cannot")
    return value, max(count, containers[0][1])


def _yaml_key(key: object, url: str, what: str) -> str:
    if isinstance(key, str):
        return key
    if isinstance(key, (int, float)) or key is None:
        return json.dumps(key)
    raise ServiceError(f"{what} at {url} has a {type(key).__name__} key, which JSON cannot")


@functools.cache
def _yaml_loader() -> type:
    """A safe YAML loader of the 1.2 core schema, as OpenAPI is written: a date stays
    text, and so do ``on``, ``yes`` and ``no``, which YAML 1.1 reads as booleans."""
    import yaml

    base = yaml.CSafeLoader
    kept = {"tag:yaml.org,2002:timestamp", "tag:yaml.org,2002:bool"}
    loader = type("OpenAPILoader", (base,), {})
    loader.yaml_implicit_resolvers = {
        first: [(tag, pattern) for tag, pattern in resolvers if tag not in kept]
        for first, resolvers in base.yaml_implicit_resolvers.items()
    }
    loader.add_implicit_resolver(
        "tag:yaml.org,2002:bool",
        re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"),
        list("tTfF"),
    )
    return loader


def _headers(headers) -> dict[str, str]:
    fields = {key.lower(): value for key, value in (headers.items() if headers else [])}
    for name in ("cache-control", "age", "vary", "content-length"):
        if name in fields:
            fields[name] = ",".join(headers.get_all(name))
    return fields


def _stream(reply, sink: IO[bytes], url: str, headers: Mapping[str, str]) -> None:
    """Write a body to ``sink`` as it arrives, checking it is as long as it said."""
    raw = headers.get("content-length")
    if raw is not None and ("transfer-encoding" in headers or not re.fullmatch(r"[0-9]+", raw)):
        raise ServiceError(f"the response from {url} has an invalid Content-Length")
    read = getattr(reply, "read1", reply.read)
    status = getattr(reply, "status", None)
    written = 0
    while chunk := read(1 << 16):
        try:
            rest = chunk
            while rest:
                count = sink.write(rest)
                if count is None:  # a sink that does not count takes it all
                    break
                if count <= 0:
                    raise OSError("the output took nothing")
                rest = rest[count:]
            sink.flush()
        except OSError as error:
            raise ServiceError(
                f"cannot write the response from {url}: {error}", status=status, code="output"
            ) from None
        written += len(chunk)
    if raw is not None and written != int(raw):
        raise ServiceError(
            f"the response from {url} ended early", status=status, code="unreachable"
        )


def _read(reply, limit: int, url: str, headers: Mapping[str, str]) -> bytes:
    raw = headers.get("content-length")
    expected = None
    if raw is not None:
        if "transfer-encoding" in headers or not re.fullmatch(r"[0-9]+", raw):
            raise ServiceError(f"the response from {url} has an invalid Content-Length")
        digits = raw.lstrip("0") or "0"
        if len(digits) > len(str(limit)) or int(digits) > limit:
            raise ServiceError(
                f"the response from {url} is over {limit >> 20} MiB", code="too_large"
            )
        expected = int(digits)
    content = reply.read(limit + 1)
    if len(content) > limit:
        raise ServiceError(f"the response from {url} is over {limit >> 20} MiB", code="too_large")
    if expected is not None and len(content) != expected:
        raise ServiceError(
            f"the response from {url} ended early",
            status=getattr(reply, "status", None),
            code="unreachable",
        )
    return content
