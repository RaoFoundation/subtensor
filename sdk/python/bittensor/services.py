"""Call the services that validators and subnet owners publish on a subnet.

A published service is a url, on chain, to its document (see the services
guide). The document names an OpenAPI 3 spec, either as an ``openapi`` service
whose ``endpoint`` is the spec or as an ``http`` service with the spec at
``spec``, and the spec's servers are where the API is. An operation the spec
secures with the ``btauth/1`` scheme (``apiKey`` headers, ``X-Bittensor-Signature``
among them) is signed by the caller's hotkey, optionally when it also allows
``{}``, and one with no security requirement is open to anyone; when a spec
declares no such scheme, every operation it does not mark ``security: []`` is
signed. Each operation is called by its ``operationId`` (``"GET /path"`` when it
has none) or that name as a command line writes it (``rent-by-spec``), with one
flat set of named inputs: its path, query and header parameters and the fields
of its JSON body, so the same names serve code, forms, command lines and agent
tools.

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
import contextlib
import dataclasses
import difflib
import functools
import http.client
import ipaddress
import json
import re
import socket
import ssl
import threading
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Mapping, Optional

from . import http_auth
from ._generated import storage
from .result import BittensorError

__all__ = [
    "Input",
    "Listing",
    "Operation",
    "Published",
    "Response",
    "Service",
    "ServiceError",
    "Skipped",
    "discover",
    "listing",
    "load",
    "load_listed",
    "load_url",
]

MAX_DOCUMENT_BYTES = 8 << 20
MAX_RESPONSE_BYTES = 64 << 20
# Seconds a request may take in all, however slowly the host answers: a call,
# and the fetch of a document, a spec or a balance.
TIMEOUT = 60.0
FETCH_TIMEOUT = 20.0
# How many schema nodes one resolve expands, at most, when it replaces $refs.
RESOLVE_BUDGET = 200_000
RESOLVE_DEPTH = 32

_METHODS = ("get", "put", "post", "delete", "patch", "head", "options")
_JSON = "application/json"
_RESERVED_HEADERS = {"host", "content-length", "content-type", "transfer-encoding", "connection"}
_USER_AGENT = "bittensor-services/1"


class ServiceError(BittensorError):
    """A service cannot be used as published, or a call does not fit its operation."""

    def __init__(self, message: str, *, status: Optional[int] = None, code: str = "unusable"):
        # The message quotes what the service sent, which a terminal must not act
        # on, and one line of it must not pass for another.
        text = "".join(c if c.isprintable() else " " if c.isspace() else "" for c in message)
        super().__init__(" ".join(text.split()))
        self.status = status  # the HTTP status the service answered, if that is the error
        self.code = code  # its kind, as the module lists them


@dataclass(frozen=True)
class Input:
    """A named input of an operation."""

    name: str
    # path, query or header: a parameter; field: a field of the JSON body object;
    # body: the whole JSON body, when it is not an object of named fields
    location: str
    required: bool
    description: str
    schema: Mapping[str, Any] = field(repr=False)
    # A query array is sent as one value joined by this, as its style says, or,
    # when "", as the parameter repeated
    joined: str = field(default="", repr=False)
    # A path parameter the JSON body has a field of the same name for, which it fills too
    in_body: bool = field(default=False, repr=False)

    @property
    def command(self) -> str:
        """The name as a command line writes it: ``gpu_type`` is ``gpu-type``."""
        return _kebab(self.name)


@dataclass(frozen=True)
class Operation:
    """An operation a service offers, called with :meth:`Service.call`."""

    id: str  # its operationId, or "GET /path" when it has none
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
    body_type: str = field(default=_JSON, repr=False)
    tags: tuple[str, ...] = ()

    @property
    def command(self) -> str:
        """The name as a command line writes it: ``rentBySpec`` is ``rent-by-spec``."""
        return _kebab(self.id)


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


@dataclass(frozen=True)
class Service:
    """A published service, as :func:`load` reads it: its document, its spec and the
    operations it offers by id; ``skipped`` says which it does not offer, and why."""

    netuid: int
    hotkey: str
    url: str
    name: str
    description: str = field(repr=False)
    spec_url: str = field(repr=False)
    api_url: str
    balance_url: str = field(repr=False)
    operations: Mapping[str, Operation] = field(repr=False)
    tags: Mapping[str, str] = field(default_factory=dict, repr=False)  # descriptions, spec order
    skipped: tuple[Skipped, ...] = field(default=(), repr=False)  # operations not offered
    document: Mapping[str, Any] = field(default_factory=dict, repr=False)
    spec: Mapping[str, Any] = field(default_factory=dict, repr=False)
    local: bool = field(default=False, repr=False)  # on a localnet on this machine: any host
    # Published by the subnet owner's hotkey, or by a validator's; None when read by url.
    owner: Optional[bool] = field(default=None, repr=False)

    def operation(self, name: str) -> Operation:
        """An operation by its id or its command name (``rent-by-spec``)."""
        op = self.operations.get(name) or next(
            (op for op in self.operations.values() if op.command == _kebab(name)), None
        )
        if op is None:
            wanted = _kebab(name)
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
        ``name`` (the command name, which :meth:`call` takes back), its
        ``description`` and ``input_schema``, the JSON Schema of its inputs."""
        op = self.operation(operation)
        budget = [RESOLVE_BUDGET]
        properties = {}
        for item in op.inputs:
            schema = _resolve(self.spec, item.schema, budget)
            schema = dict(schema) if isinstance(schema, dict) else {}
            if item.description:
                schema.setdefault("description", item.description)
            properties[item.name] = schema
        about = [op.summary] + ([op.description] if op.description != op.summary else [])
        return _plain(
            {
                "name": op.command,
                "description": "\n\n".join(text for text in about if text),
                "input_schema": {
                    "type": "object",
                    "properties": properties,
                    "required": [item.name for item in op.inputs if item.required],
                },
            }
        )

    def tools(self) -> list[dict]:
        """Every operation as :meth:`tool` defines one."""
        return [self.tool(op_id) for op_id in self.operations]

    def resolve(self, schema: Any) -> Any:
        """``schema`` with the spec's local ``$ref`` replaced by their targets, up to
        RESOLVE_BUDGET nodes; past it, and for a cyclic ref, a ref stays a ref."""
        return _resolve(self.spec, schema, [RESOLVE_BUDGET])

    async def call(
        self,
        operation: str,
        inputs: Optional[Mapping[str, Any]] = None,
        *,
        signer: Any = None,
        timeout: float = TIMEOUT,
    ) -> Response:
        """Call an operation with its inputs by name; returns the response whatever its status.

        A list repeats a query parameter. A ``hotkey`` operation needs ``signer``,
        and an ``optional`` one is signed when it is given: anything
        :func:`bittensor.http_auth.sign` accepts, prepared with
        :func:`bittensor.http_auth.prepare_signer`.
        """
        op = self.operation(operation)
        method, url, target, headers, data = _request(op, inputs or {})
        if op.auth == "hotkey" and signer is None:
            raise ServiceError(f"{op.id} is signed by a hotkey; pass a signer", code="input")

        def send() -> Response:
            # Signed as it is sent: a signature made before a wait for a thread goes stale.
            if signer is not None and op.auth != "open":
                headers.update(
                    http_auth.sign(
                        signer, method=method, path=target, body=data, receiver_ss58=self.hotkey
                    )
                )
            return _send(method, url, headers, data, self.local, MAX_RESPONSE_BYTES, timeout)

        return await _threaded(send)

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
        split = urllib.parse.urlsplit(self.balance_url)
        query = urllib.parse.urlencode(
            [*urllib.parse.parse_qsl(split.query), ("hotkey", hotkey_ss58)]
        )
        url = urllib.parse.urlunsplit(split._replace(query=query))
        headers = {"User-Agent": _USER_AGENT, "Accept": _JSON}

        def read() -> Response:
            return _send("GET", url, headers, b"", self.local, MAX_DOCUMENT_BYTES, FETCH_TIMEOUT)

        response = await _threaded(read)
        if response.status in (401, 403) and signer is not None:
            target = urllib.parse.urlunsplit(("", "", split.path or "/", query, ""))
            headers.update(
                http_auth.sign(signer, method="GET", path=target, receiver_ss58=self.hotkey)
            )
            response = await _threaded(read)
        record = _json_object(response, url, "the balance")
        if isinstance(record.get("hotkey"), str) and record["hotkey"] != hotkey_ss58:
            raise ServiceError(f"the balance at {url} is {record['hotkey']}'s, not {hotkey_ss58}'s")
        return record


@dataclass(frozen=True)
class Listing:
    """What the chain says of a subnet's services: the url each hotkey published there,
    and the subnet owner's hotkey. :func:`load_listed` loads one without the chain."""

    netuid: int
    urls: Mapping[str, str]
    owner: Optional[str] = None
    local: bool = False  # the chain is a localnet on this machine


async def listing(client, netuid: int) -> Listing:
    """Read a subnet's services and its owner's hotkey from the chain."""
    published, owner, local = await asyncio.gather(
        client.read("services", netuid=netuid),
        client.query(storage.SubtensorModule.SubnetOwnerHotkey, [netuid]),
        _on_local_chain(client),
    )
    urls = {str(hotkey): str(entry["url"]) for hotkey, entry in published.items()}
    return Listing(netuid, urls, str(owner) if owner else None, local)


async def load(client, netuid: int, hotkey_ss58: Optional[str] = None) -> Service:
    """Read the service a hotkey published on a subnet, with its document and spec.

    Without ``hotkey_ss58`` it is the subnet owner's service, or the subnet's only one.
    A validator can publish a service too, under any name, so another is used only when
    named by its hotkey.
    """
    return await load_listed(await listing(client, netuid), hotkey_ss58)


async def load_listed(listed: Listing, hotkey_ss58: Optional[str] = None) -> Service:
    """:func:`load` from a :class:`Listing` read before, without the chain."""
    netuid, urls, owner = listed.netuid, listed.urls, listed.owner
    if hotkey_ss58 is None:
        if not urls:
            raise ServiceError(f"netuid {netuid} has no services", code="no_service")
        if owner in urls:
            hotkey_ss58 = owner
        elif len(urls) > 1:
            raise ServiceError(
                f"netuid {netuid} has {len(urls)} services, none of them the subnet "
                "owner's; name one by its hotkey: " + ", ".join(sorted(urls)),
                code="ambiguous",
            )
        else:
            hotkey_ss58 = next(iter(urls))
    url = urls.get(hotkey_ss58)
    if url is None:
        raise ServiceError(
            f"{hotkey_ss58} has published no service on netuid {netuid}", code="no_service"
        )
    service = await _threaded(_fetch_service, netuid, hotkey_ss58, url, listed.local)
    return dataclasses.replace(service, owner=hotkey_ss58 == owner)


async def load_url(
    url: str, netuid: Optional[int] = None, hotkey_ss58: Optional[str] = None
) -> Service:
    """Read the service a document describes, by its url, without the chain: to
    check a document before it is published, or to use one known by its url.

    The document is held to the rules of a published one: a url on this machine
    may use http and private hosts, as on a localnet, any other only https to
    public hosts. It must list ``netuid`` and ``hotkey_ss58`` in ``publishers``,
    and without them it must list exactly one publisher, whose service it is.
    """
    return await _threaded(_fetch_service, netuid, hotkey_ss58, url, _is_local(url))


@dataclass(frozen=True)
class Published:
    """A service published on chain, as :func:`discover` finds it."""

    netuid: int
    hotkey: str
    url: str
    service: Optional[Service] = None  # None when it cannot be used as published
    problem: str = ""  # why it cannot
    owner: bool = False  # published by the subnet owner's hotkey, not a validator's


async def discover(client, netuid: Optional[int] = None) -> list[Published]:
    """Every service published on chain, or on ``netuid``, each loaded as :func:`load`
    loads one, by netuid; one that cannot be used says why."""
    local = await _on_local_chain(client)
    few = asyncio.Semaphore(8)  # at a time, so slow services do not take every thread

    async def find(netuid: int, hotkey: str, url: str) -> Published:
        owner = hotkey == owners.get(netuid)
        try:
            async with few:
                service = await _threaded(_fetch_service, netuid, hotkey, url, local)
        except Exception as error:  # one service's failure is never the listing's
            problem = str(ServiceError(str(error)))
            return Published(netuid, hotkey, url, problem=problem, owner=owner)
        return Published(
            netuid, hotkey, url, dataclasses.replace(service, owner=owner), owner=owner
        )

    if netuid is None:
        rows = await client.query_map(storage.SubtensorModule.Services)
    else:
        published = await client.read("services", netuid=netuid)
        rows = [((netuid, hotkey), entry) for hotkey, entry in published.items()]
    netuids = sorted({int(netuid) for (netuid, _), _ in rows})
    found = await asyncio.gather(
        *(client.query(storage.SubtensorModule.SubnetOwnerHotkey, [n]) for n in netuids)
    )
    owners = dict(zip(netuids, found))
    return await asyncio.gather(
        *(
            find(int(netuid), str(hotkey), info["url"])
            for (netuid, hotkey), info in sorted(rows, key=lambda row: row[0])
        )
    )


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
        return "".join(ch for ch in value if ch in "\n\t" or unicodedata.category(ch)[0] != "C")
    if isinstance(value, dict):
        return {_plain(key): _plain(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_plain(item) for item in value]
    return value


def _fetch_service(netuid: Optional[int], hotkey: Optional[str], url: str, local: bool) -> Service:
    try:
        return _service(netuid, hotkey, url, local)
    except ValueError as error:  # a url the service wrote that is not one
        raise ServiceError(f"the service at {url} has a bad url: {error}") from None


def _service(netuid: Optional[int], hotkey: Optional[str], url: str, local: bool) -> Service:
    document = _get_json(url, "the services document", local)
    netuid, hotkey = _publisher(document, url, netuid, hotkey)
    balance_url = _checked_url(_gamma_balance(document, url), "the balance", local)
    spec_url = _checked_url(_spec_url(document, url), "the OpenAPI spec", local)
    spec = _get_json(spec_url, "the OpenAPI spec", local)
    operations, skipped = _operations(spec, spec_url, local)
    return Service(
        netuid=netuid,
        hotkey=hotkey,
        url=url,
        name=_text(document.get("name")) or url,
        description=_text(document.get("description")),
        spec_url=spec_url,
        api_url=_checked_url(_server(spec_url, spec), "the API", local),
        balance_url=balance_url,
        operations=operations,
        tags={
            tag["name"]: _text(tag.get("description"))
            for tag in _list(spec.get("tags"))
            if isinstance(tag, dict) and isinstance(tag.get("name"), str)
        },
        skipped=skipped,
        document=document,
        spec=spec,
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
        and isinstance(p.get("hotkey"), str)
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


def _spec_url(document: Mapping[str, Any], url: str) -> str:
    """The url of the one OpenAPI spec the document names."""
    specs = []
    services = document.get("services")
    for service in services if isinstance(services, list) else []:
        if not isinstance(service, dict):
            continue
        if service.get("type") == "openapi" and isinstance(service.get("endpoint"), str):
            specs.append(service["endpoint"])
        elif service.get("type") == "http" and isinstance(service.get("spec"), str):
            specs.append(service["spec"])
    if len(specs) != 1:
        found = "no OpenAPI spec" if not specs else f"{len(specs)} OpenAPI specs"
        raise ServiceError(f"the document at {url} names {found}; it must name one")
    return urllib.parse.urljoin(url, specs[0])


class _Unusable(Exception):
    """Why an operation cannot be offered."""


def _operations(
    spec: Mapping[str, Any], spec_url: str, local: bool
) -> tuple[dict[str, Operation], tuple[Skipped, ...]]:
    version = spec.get("openapi")
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

    declared = any(btauth({name: []}) for name in schemes)
    root_security = spec.get("security")
    operations: dict[str, Operation] = {}
    skipped: list[Skipped] = []
    budget = [RESOLVE_BUDGET]  # schema nodes the bodies of the whole spec may take to read
    paths = spec.get("paths")
    for path, item in paths.items() if isinstance(paths, dict) else []:
        try:
            item = _deref(spec, item)
        except ServiceError as error:
            skipped.append(Skipped("", _line(str(path)), "", str(error)))
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

            skip = functools.partial(Skipped, method.upper(), _line(str(path)), given)
            try:
                if not path.startswith("/") or not _plain_path(path):
                    raise _Unusable("has a path that is not plain")
                security = op.get("security", root_security)
                if security is not None and not isinstance(security, list):
                    raise _Unusable("has a malformed security requirement")
                requirements = security or []
                anonymous = not requirements or {} in requirements
                if not declared:  # v1.3: every paid route takes btauth/1
                    auth = "open" if security == [] else "optional" if anonymous else "hotkey"
                elif any(btauth(r) for r in requirements):
                    auth = "optional" if anonymous else "hotkey"
                elif anonymous:
                    auth = "open"
                else:
                    raise _Unusable("needs credentials other than btauth/1")
                if op_id in operations:
                    raise _Unusable(f"repeats the operationId {op_id!r}")
                api_url = _checked_url(_server(spec_url, op, item, spec), "the API", local)
                inputs, body_required, body_type = _inputs(
                    spec, [*_list(item.get("parameters")), *_list(op.get("parameters"))], op, budget
                )
                in_path = {i.name for i in inputs if i.location == "path"}
                undeclared = set(re.findall(r"\{([^{}]*)\}", path)) - in_path
                if undeclared:
                    raise _Unusable(f"does not declare its path parameter {min(undeclared)!r}")
            except (_Unusable, ServiceError) as reason:
                skipped.append(skip(str(reason)))
                continue
            except ValueError as error:
                skipped.append(skip(f"has a bad url: {_line(str(error))}"))
                continue
            except Exception:  # whatever else the spec gets wrong is this operation's
                skipped.append(skip("has a malformed definition"))
                continue
            operations[op_id] = Operation(
                id=op_id,
                method=method.upper(),
                path=path,
                auth=auth,
                summary=_text(op.get("summary")),
                description=_text(op.get("description")),
                inputs=inputs,
                api_url=api_url,
                body_required=body_required,
                body_type=body_type,
                tags=tuple(tag for tag in _list(op.get("tags")) if isinstance(tag, str)),
            )
    # Names that a command line or a tool would write the same way call nothing reliably.
    commands: dict[str, list[str]] = {}
    for op_id, op in operations.items():
        commands.setdefault(op.command, []).append(op_id)
    for command, ids in commands.items():
        if len(ids) > 1:
            for op_id in ids:
                op = operations.pop(op_id)
                given = "" if op_id == f"{op.method} {op.path}" else _line(op_id)
                skipped.append(
                    Skipped(op.method, _line(op.path), given, f"shares the name {command!r}")
                )
    return operations, tuple(skipped)


def _btauth_header(scheme: Any) -> str:
    """The lowercase header name of a btauth/1 ``apiKey`` scheme, else ""."""
    if isinstance(scheme, dict) and scheme.get("type") == "apiKey" and scheme.get("in") == "header":
        name = str(scheme.get("name", "")).lower()
        return name if name.startswith("x-bittensor-") else ""
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
        value = _get(variables, match.group(1), "default")
        if not isinstance(value, str):
            raise ServiceError(
                f"{spec_url} has a server variable {match.group(1)!r} with no default"
            )
        return value

    return urllib.parse.urljoin(spec_url, re.sub(r"\{([^{}]*)\}", default, url))


def _plain_path(path: str) -> bool:
    return not any(ch in "?#" or ch.isspace() or not ch.isprintable() for ch in path)


def _list(value: Any) -> list:
    return value if isinstance(value, list) else []


def _inputs(
    spec, parameters: list, op: Mapping[str, Any], budget: list
) -> tuple[tuple[Input, ...], bool, str]:
    """An operation's inputs, one flat set of names, whether its body is required,
    and its body's media type."""
    merged: dict[tuple[str, str], Input] = {}
    for raw in parameters:
        param = _deref(spec, raw)
        if not isinstance(param, dict) or not isinstance(param.get("name"), str):
            continue
        location = str(param.get("in"))
        required = location == "path" or bool(param.get("required"))
        schema = _deref(spec, param.get("schema") or {})
        schema = schema if isinstance(schema, dict) else {}
        if location not in ("path", "query", "header") or (
            location == "header" and not _sendable_header(param["name"])
        ):
            why = f"needs {param['name']!r} in {location!r}"
        elif "content" in param or schema.get("type") == "object" or "properties" in schema:
            why = f"takes {param['name']!r} as an object in the {location}"
        else:
            why = ""
        if why:
            if required:
                raise _Unusable(why)
            continue
        merged[(param["name"], location)] = Input(
            name=param["name"],
            location=location,
            required=required,
            description=_text(param.get("description")),
            schema=schema,
            joined=_joined(param) if location == "query" else "",
        )
    inputs = list(merged.values())
    body = _deref(spec, op.get("requestBody"))
    content = body.get("content") if isinstance(body, dict) else None
    body_required = isinstance(body, dict) and bool(body.get("required"))
    body_type = _JSON
    if isinstance(content, dict) and content:
        media_type = next((kind for kind in content if _is_json(kind)), None)
        if media_type is None:
            kinds = ", ".join(sorted(_line(str(kind)) for kind in content))
            raise _Unusable(f"takes a body that is {kinds}, not JSON")
        body_type = media_type.split(";")[0].strip()
        media = content[media_type] if isinstance(content[media_type], dict) else {}
        schema = _deref(spec, media.get("schema") or {})
        fields = _object(spec, schema, budget)
        if fields and fields[0]:
            properties, required = fields
            for name, raw in properties.items():
                prop = _deref(spec, raw)
                prop = prop if isinstance(prop, dict) else {}
                if not isinstance(name, str) or prop.get("readOnly") is True:
                    continue
                if (name, "path") in merged:
                    # PUT /items/{id} with the id in the body too: one input fills both.
                    at = inputs.index(merged[(name, "path")])
                    inputs[at] = dataclasses.replace(inputs[at], in_body=True)
                    continue
                inputs.append(
                    Input(
                        name=name,
                        location="field",
                        required=body_required and name in required,
                        description=_text(prop.get("description")),
                        schema=prop,
                    )
                )
        else:
            inputs.append(
                Input(
                    name="body",
                    location="body",
                    required=body_required,
                    description=_text(body.get("description")),
                    schema=schema if isinstance(schema, dict) else {},
                )
            )
    seen: dict[str, str] = {}
    for item in inputs:
        if not item.command:
            raise _Unusable(f"has an input {item.name!r} with no letter or digit in its name")
        if item.command in seen:
            alike = seen[item.command]
            raise _Unusable(
                f"has two inputs named {alike!r}"
                if alike == item.name
                else f"has inputs {alike!r} and {item.name!r}, named alike"
            )
        seen[item.command] = item.name
    return tuple(inputs), body_required, body_type


def _joined(param: Mapping[str, Any]) -> str:
    """What a query array is joined by, as its style says, or "" when each item
    repeats the parameter."""
    style = param.get("style", "form")
    explode = param.get("explode", style == "form")
    if explode is True:
        return ""
    return {"form": ",", "spaceDelimited": " ", "pipeDelimited": "|"}.get(style, ",")


def _object(spec, schema: Any, budget: list, depth: int = 0) -> Optional[tuple[dict, set]]:
    """The fields of an object schema and which are required, with ``allOf`` merged and
    a nullable ``anyOf`` or ``oneOf`` read as its one other choice; None when it is
    not an object, is one of several shapes, or takes more than ``budget[0]`` nodes."""
    schema = _deref(spec, schema)
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


def _request(op: Operation, inputs: Mapping[str, Any]):
    if not isinstance(inputs, Mapping):
        raise ServiceError(
            f"{op.id} takes its inputs as a mapping of names to values", code="input"
        )
    known = {i.name: i for i in op.inputs}
    by_command = {i.command: i.name for i in op.inputs}
    given: dict[str, Any] = {}
    for name, value in inputs.items():
        name = name if name in known else by_command.get(_kebab(str(name)), name)
        if name not in known:
            close = difflib.get_close_matches(str(name), list(known), n=2)
            hint = f"; did you mean {', '.join(close)}?" if close else ""
            raise ServiceError(f"{op.id} has no input {name!r}{hint}", code="input")
        given[name] = value
    inputs = given
    for item in op.inputs:
        # A fixed or defaulted value need not be given: "jsonrpc": "2.0" every time.
        if item.required and item.name not in inputs:
            for key in ("const", "default"):
                if key in item.schema:
                    inputs[item.name] = item.schema[key]
                    break
    missing = [i.name for i in op.inputs if i.required and i.name not in inputs]
    if missing:
        raise ServiceError(f"{op.id} needs {', '.join(map(repr, missing))}", code="input")
    path, query, headers = op.path, [], {"User-Agent": _USER_AGENT}
    fields: dict[str, Any] = {}
    whole: Any = _NOT_GIVEN
    for item in op.inputs:
        if item.name not in inputs:
            continue
        value = inputs[item.name]
        if item.location == "path":
            text = _scalar(value)
            if text in ("", ".", ".."):  # each would reach another route
                raise ServiceError(f"{op.id} cannot take {text!r} as {item.name}", code="input")
            path = path.replace("{" + item.name + "}", urllib.parse.quote(text, safe=""))
            if item.in_body:
                fields[item.name] = value
        elif item.location == "query":
            values = value if isinstance(value, (list, tuple)) else [value]
            if item.joined and isinstance(value, (list, tuple)):
                values = [item.joined.join(_scalar(v) for v in values)]
            query.extend((item.name, _scalar(v)) for v in values)
        elif item.location == "header":
            text = _scalar(value)
            if not text.isprintable():
                raise ServiceError(f"{item.name} cannot hold control characters", code="input")
            try:
                text.encode("latin-1")
            except UnicodeEncodeError:
                raise ServiceError(
                    f"{item.name} is a header, which holds Latin-1 text only", code="input"
                ) from None
            headers[item.name] = text
        elif item.location == "field":
            fields[item.name] = value
        else:
            whole = value
    data = b""
    if whole is not _NOT_GIVEN or fields or op.body_required:
        try:
            data = json.dumps(
                fields if whole is _NOT_GIVEN else whole,
                separators=(",", ":"),
                ensure_ascii=False,
                allow_nan=False,
            ).encode()
        except (TypeError, ValueError) as error:
            raise ServiceError(
                f"{op.id} cannot send its inputs as JSON: {error}", code="input"
            ) from None
        headers["Content-Type"] = op.body_type
    if "{" in path:
        raise ServiceError(f"{op.id} has a path parameter it does not declare")
    base = urllib.parse.urlsplit(op.api_url)
    full_path = urllib.parse.quote(base.path.rstrip("/") + path, safe="/%!$&'()*+,;=:@-._~")
    query_text = "&".join(part for part in (base.query, urllib.parse.urlencode(query)) if part)
    target = full_path + ("?" + query_text if query_text else "")
    url = urllib.parse.urlunsplit((base.scheme, base.netloc, full_path, query_text, ""))
    return op.method, url, target, headers, data


_NOT_GIVEN = object()


def _sendable_header(name: str) -> bool:
    lowered = name.lower()
    return lowered not in _RESERVED_HEADERS and not lowered.startswith("x-bittensor-")


def _scalar(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None or isinstance(value, (dict, list, tuple)):
        raise ServiceError(f"cannot send {json.dumps(value)} as a parameter", code="input")
    return str(value)


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


def _deref(spec: Mapping[str, Any], node: Any) -> Any:
    """``node``, or the target of the local ``$ref`` it is, followed to the end."""
    seen = set()
    while isinstance(node, dict) and isinstance(node.get("$ref"), str):
        ref = node["$ref"]
        if ref in seen or not ref.startswith("#/"):
            return node
        seen.add(ref)
        node = _pointer(spec, ref)
    return node


def _pointer(spec: Mapping[str, Any], ref: str) -> Any:
    target: Any = spec
    for part in ref[2:].split("/"):
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


def _resolve(spec: Mapping[str, Any], node: Any, budget: list) -> Any:
    """``node`` with local ``$ref`` replaced by their targets, spending ``budget[0]`` nodes,
    and long strings as several, so nothing grows past the budget.

    A cyclic or broken ref, and every ref past the budget or depth, stays a ref.
    """

    def walk(node: Any, depth: int, seen: frozenset) -> Any:
        budget[0] -= 1 + (len(node) >> 6 if isinstance(node, str) else 0)
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
                return walk(target, depth + 1, seen | {ref})
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
    if split.username or split.password or not host:
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
        if req.get_method() != "GET" or req.get_header(http_auth.HEADER_SIGNATURE.capitalize()):
            return None
        _checked_url(newurl, "a redirect", self.local)
        new = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new is not None and new.host != req.host:
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
        # Through a proxy the peer is the proxy; the host is what it reaches.
        tunnel = getattr(self, "_tunnel_host", None)
        if self.public and tunnel:
            found = _lookup(tunnel, None, deadline)
            if not all(_is_global(info[4][0]) for info in found):
                raise ServiceError(f"{tunnel} is not a public host")
        host, port = address
        error: Exception = OSError(f"{host} has no address")
        for family, kind, proto, _, peer in _lookup(host, port, deadline):
            if self.public and not tunnel and not _is_global(peer[0]):
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

    worker = threading.Thread(target=look_up, daemon=True)
    worker.start()
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
    return ip.is_global


class _TLSConnection(_Connection, http.client.HTTPSConnection):
    pass


class _LocalConnection(_Connection):
    public = False


class _LocalTLSConnection(_LocalConnection, http.client.HTTPSConnection):
    pass


_TLS = ssl.create_default_context()


class _HTTP(urllib.request.HTTPHandler):
    def __init__(self, connection: type[http.client.HTTPConnection]):
        super().__init__()
        self._connection = connection

    def http_open(self, req):
        return self.do_open(self._connection, req)


class _HTTPS(urllib.request.HTTPSHandler):
    def __init__(self, connection: type[http.client.HTTPSConnection]):
        super().__init__(context=_TLS)
        self._connection = connection

    def https_open(self, req):
        return self.do_open(self._connection, req, context=_TLS)


_openers = {
    False: urllib.request.build_opener(
        _Redirects(False), _HTTP(_Connection), _HTTPS(_TLSConnection)
    ),
    # On a localnet on this machine a service may use it and private hosts.
    True: urllib.request.build_opener(
        _Redirects(True), _HTTP(_LocalConnection), _HTTPS(_LocalTLSConnection)
    ),
}


def _send(
    method: str,
    url: str,
    headers: Mapping[str, str],
    data: bytes,
    local: bool = False,
    limit: int = MAX_RESPONSE_BYTES,
    timeout: float = TIMEOUT,
) -> Response:
    request = urllib.request.Request(url, data=data or None, headers=dict(headers), method=method)
    with _Deadline(timeout) as deadline:
        try:
            try:
                reply = _openers[local].open(request, timeout=timeout)
            except urllib.error.HTTPError as error:
                reply = error
            with reply:
                status = reply.getcode() or 0
                response = Response(status, _headers(reply.headers), _read(reply, limit, url))
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


def _get_json(url: str, what: str, local: bool = False) -> dict:
    headers = {"User-Agent": _USER_AGENT, "Accept": _JSON}
    response = _send(
        "GET",
        _checked_url(url, what, local),
        headers,
        b"",
        local,
        MAX_DOCUMENT_BYTES,
        FETCH_TIMEOUT,
    )
    return _json_object(response, url, what)


def _json_object(response: Response, url: str, what: str) -> dict:
    if not response.ok:
        message = response.message[:200]
        raise ServiceError(
            f"{what} at {url} answered HTTP {response.status}"
            + (f": {message}" if message else ""),
            status=response.status,
            code="http",
        )
    try:
        value = response.json()
    except (ValueError, RecursionError):
        yaml = re.match(r"\s*[\w\"'-]+\s*:", response.text[:200])
        raise ServiceError(
            f"{what} at {url} is not JSON"
            + (" (it reads as YAML, which is not read)" if yaml else "")
        ) from None
    if not isinstance(value, dict):
        raise ServiceError(f"{what} at {url} is not a JSON object")
    return value


def _headers(headers) -> dict[str, str]:
    return {key.lower(): value for key, value in (headers.items() if headers else [])}


def _read(reply, limit: int, url: str) -> bytes:
    content = reply.read(limit + 1)
    if len(content) > limit:
        raise ServiceError(f"the response from {url} is over {limit >> 20} MiB", code="too_large")
    length = (getattr(reply, "headers", None) or {}).get("content-length", "")
    if content and length.isdigit() and len(content) < int(length):
        raise ServiceError(f"the response from {url} ended early", code="unreachable")
    return content
