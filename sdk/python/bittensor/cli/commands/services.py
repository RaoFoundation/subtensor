"""`btcli service`: the services published on chain, what they offer, calling them, and
balances at them.

An operation's inputs are flags named after them (``gpu_type`` is ``--gpu-type``)
and typed by the spec, so no request is ever written by hand. btcli's own options
can precede the operation. Common call controls also work after it; use
``--inputs`` for a provider input sharing a control's name.
"""

from __future__ import annotations

import asyncio
import codecs
import contextlib
import dataclasses
import difflib
import io
import json
import mimetypes
import os
import re
import shlex
import sys
import tempfile
import threading
import urllib.parse
from pathlib import Path
from typing import IO, Any, Awaitable, Callable, NoReturn, Optional, TypeVar

import typer
from rich.markup import escape

from ... import config as cfg
from ... import http_auth, services, wallets
from ...keyfiles import KeyfileError
from ...result import BittensorError
from ...services import _not_json, _plain
from ...settings import DEFAULT_NETWORK, guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals
from ..output import Output

app = typer.Typer(
    no_args_is_help=True,
    help="Subnet services: finding them, what they offer, calling them, and your balance "
    "at them.\n\n"
    f"Guide: {guide_docs_url('services')}",
)

# Wider records are easier to read as YAML.
MAX_COLUMNS = 8
# Characters of an error response shown; a validation error lists every field.
MAX_ERROR = 2000
# Operations listed as not offered; --json lists them all.
MAX_SKIPPED = 20

T = TypeVar("T")


def _service_option():
    return typer.Option(
        None,
        address_cli_name("service_hotkey_ss58"),
        help="A validator's hotkey, to use the service it published, not the subnet owner's. "
        + ss58_param_help("service_hotkey_ss58"),
    )


@app.command(
    "list",
    help="List the services published on chain that btcli can use.\n\n"
    "--all adds the ones it cannot, and why; with --json every service is listed, with "
    "`usable` and `problem`.",
)
@with_globals
def list_services(
    ctx: typer.Context,
    netuid: Optional[int] = typer.Option(None, "--netuid", help="Only this subnet's services."),
    every: bool = typer.Option(False, "--all", help="Also list the services btcli cannot use."),
):
    app_ctx: AppContext = ctx_of(ctx)
    with app_ctx.output.activity("reading the services…") as progress:
        read: list[services.Published] = []

        def record(item: services.Published) -> dict:
            read.append(item)
            progress(f"read {len(read)} services…")
            return _list_record(item)

        records = app_ctx.run(
            lambda client: services.discover(
                client, netuid, cache=cfg.service_documents_path(), select=record
            )
        )
    if app_ctx.output.json_mode:
        app_ctx.output.value(_plain(records))
        return
    usable = [row for row in records if row["usable"]]
    broken = [row for row in records if not row["usable"]]
    every = every or netuid is not None
    out = app_ctx.output
    footer = _list_footer(len(broken))
    with _rendering(app_ctx):
        out.columns(
            "services",
            ["netuid", "service", "operations", "open", "about"],
            [
                [
                    row["netuid"],
                    _short(row["name"], 32)
                    + ("" if row["publisher"] == "owner" else " (validator)"),
                    sum(row["operations"].values()),
                    row["operations"]["open"] or "",
                    _first_sentence(row["description"]),
                ]
                for row in usable
            ],
            right_align={0, 2, 3},
            footer=None if broken and every else footer,
        )
        if broken and not usable and not every:
            out.message(footer)
        if broken and every:
            out.columns(
                "cannot be used",
                ["netuid", "published at", "why"],
                [[row["netuid"], _host(row["url"]), _why(row)] for row in broken],
                right_align={0},
            )
        for row in usable + (broken if every else []):
            if row["publisher"] == "validator":
                name = escape(_line(row["name"] or row["url"]))
                out.message(f"{name}: --netuid {row['netuid']} --service-hotkey {row['hotkey']}")


def _list_record(item: services.Published) -> dict:
    service = item.service
    problem = item.problem.replace(f" at {item.url}", "")
    return {
        "netuid": item.netuid,
        "hotkey": item.hotkey,
        "publisher": _publisher(item.owner),
        "url": item.url,
        "usable": not problem,
        "name": service.name if service else None,
        "description": service.description if service else None,
        "operations": _auth_counts(service) if service else None,
        "problem": problem or None,
    }


def _publisher(owner: Optional[bool]) -> Optional[str]:
    return {True: "owner", False: "validator"}.get(owner)


def _list_footer(broken: int) -> str:
    hint = "show one with `btcli service show --netuid N`"
    if not broken:
        return hint
    many = "s" if broken > 1 else ""
    return f"{hint}\n{broken} more service{many} cannot be used yet; `--all` lists why"


def _first_sentence(text: Optional[str]) -> str:
    sentence = _line(text or "").split(". ")[0].rstrip(".")
    return _short(sentence, 60)


def _why(row: dict) -> str:
    problem = row["problem"].replace(
        f"netuid {row['netuid']} hotkey {row['hotkey']}", "the publishing hotkey"
    )
    return _short(re.sub(r"https?://\S+", "its spec", problem), 70)


def _host(url: str) -> str:
    try:
        return _line(urllib.parse.urlsplit(url).netloc) or _short(url, 40)
    except ValueError:
        return _short(url, 40)


def _auth_counts(service: services.Service) -> dict[str, int]:
    """How many operations a service offers, by who can call them."""
    counts = {"open": 0, "optional": 0, "hotkey": 0}
    for op in service.operations.values():
        counts[op.auth] += 1
    return counts


def _url_option():
    return typer.Option(
        None,
        "--url",
        help="Read the services document at this url, not the chain's, to try a service "
        "before publishing it. It must list --netuid and --service-hotkey in its "
        "publishers when they are given. Signing requires a local document and endpoint.",
    )


@app.command(
    "show",
    help="Show a service and the operations it offers.\n\n"
    "A `hotkey` operation requires a signature. An `optional` operation accepts one. "
    "An `open` operation needs no key. "
    "`btcli service call --netuid N OPERATION --help` describes one.\n\n"
    "With --json, each operation lists its inputs by the names --inputs takes; --schemas "
    "adds their JSON Schema.",
)
@with_globals
def show(
    ctx: typer.Context,
    netuid: Optional[int] = typer.Option(None, "--netuid", help="Subnet of the service."),
    service_hotkey: Optional[str] = _service_option(),
    url: Optional[str] = _url_option(),
    schemas: bool = typer.Option(
        False, "--schemas", help="With --json, include every operation's input schema."
    ),
):
    app_ctx: AppContext = ctx_of(ctx)
    service = _service(app_ctx, netuid, service_hotkey, url)
    with _rendering(app_ctx):
        _show(app_ctx, service, schemas)


def _show(app_ctx: AppContext, service: services.Service, schemas: bool = False) -> None:
    output = app_ctx.output
    links = {key: _link(service.document.get(key)) for key in ("docs", "pricing")}
    if output.json_mode:
        record = _plain(
            {
                "netuid": service.netuid,
                "hotkey": service.hotkey,
                "publisher": _publisher(service.owner),
                "name": service.name,
                "description": service.description,
                "document": service.url,
                "api": service.api_url,
                "specs": list(service.spec_urls),
                "balance": service.balance_url,
                **links,
                "pay": _pay_command(app_ctx, service),
                "tags": dict(service.tags),
                "skipped": [
                    {"method": s.method, "path": s.path, "id": s.id, "reason": s.reason}
                    for s in service.skipped
                ],
            }
        )
        operations = service.operations.values()
        if schemas:
            record["operations"] = [
                _definition(op, tool) for op, tool in zip(operations, service.tools(), strict=True)
            ]
        else:
            record["operations"] = [_plain(_brief(op)) for op in operations]
        output.value(record)
        return
    counts = _auth_counts(service)
    output.detail(
        f"{_line(service.name)} on netuid {service.netuid}",
        _fields(
            {
                "description": service.description,
                "hotkey": service.hotkey,
                "publisher": {
                    True: "the subnet owner",
                    False: "a validator, not the subnet owner: check it is the service you mean",
                }.get(service.owner),
                "document": service.url,
                "api": service.api_url,
                "spec": ", ".join(service.spec_urls),
                "balance": service.balance_url,
                **links,
                "operations": f"{len(service.operations)}: "
                + ", ".join(f"{count} {auth}" for auth, count in counts.items() if count),
                "pay": _pay_command(app_ctx, service),
            }
        ),
    )
    for title, ops in _groups(service):
        typer.echo()
        output.columns(
            title,
            ["operation", "auth", "summary"],
            [[_line(op.command), op.auth, _short(op.summary)] for op in ops],
        )
    if service.skipped:
        typer.echo()
        _show_skipped(output, service.skipped)


def _show_skipped(output, skipped: tuple[services.Skipped, ...]) -> None:
    """The operations not offered: each one, or, when they are many, each reason."""
    if len(skipped) <= MAX_SKIPPED:
        rows = [
            [_line(f"{s.method} {s.path}" + (f" ({s.id})" if s.id else "")), _short(s.reason, 100)]
            for s in skipped
        ]
        output.columns("not offered", ["operation", "why"], rows)
        return
    reasons: dict[str, list[services.Skipped]] = {}
    for skip in skipped:
        reasons.setdefault(skip.reason, []).append(skip)
    output.columns(
        "not offered",
        ["operations", "why", "such as"],
        [
            [len(group), _short(reason, 80), _short(f"{group[0].method} {group[0].path}", 50)]
            for reason, group in reasons.items()
        ],
        right_align={0},
    )
    output.message("`--json` lists each of them")


def _brief(op: services.Operation) -> dict:
    """An operation as a line of an index: what it is, and the names of its inputs."""
    return {
        "operation": op.command,
        "auth": op.auth,
        "method": op.method,
        "summary": op.summary,
        "required": [item.name for item in op.tool_inputs if item.must_supply],
        "optional": [item.name for item in op.tool_inputs if not item.must_supply],
    }


def _definition(op: services.Operation, tool: dict) -> dict:
    """An operation in full: what it is, and the JSON Schema of its inputs."""
    return _plain(
        {
            **_brief(op),
            "id": op.id,
            "path": op.path,
            "server": op.api_url,
            "tags": list(op.tags),
            "description": op.description,
        }
    ) | {"input_schema": tool["input_schema"]}


@app.command(
    "call",
    help="Call an operation of a service.\n\n"
    "btcli service call --netuid 19 json-rpc-request --method system_health\n\n"
    "btcli --json service call --netuid 19 json-rpc-request --inputs "
    '\'{"method": "system_health"}\'\n\n'
    "Inputs follow the operation as flags (`max_tokens` is `--max-tokens`), or all at "
    "once as a JSON object with --inputs. `OPERATION --help` lists them, and without "
    "OPERATION the service's operations are listed. --inputs and --body read a file "
    "named with @ (@- reads stdin).\n\n"
    "A service may charge your balance for a signed call. An optional operation is "
    "signed only when you name a wallet or hotkey (-w, -H, or BT_WALLET, BT_WALLET_HOTKEY), "
    "and --unsigned leaves it unsigned even then.\n\n"
    "With --json the answer is `{status, content_type, signed, body}` (`text` when the "
    "body is not JSON, `out` when it went to a file), and an error is JSON on stderr with "
    "a `code`. It exits 2 when the call does not fit the operation, and 1 for any other "
    "failure, an HTTP error among them.",
    context_settings={
        "help_option_names": [],
        "allow_extra_args": True,
        "ignore_unknown_options": True,
    },
)
@with_globals
def call(
    ctx: typer.Context,
    operation: Optional[str] = typer.Argument(None, metavar="[OPERATION [INPUTS]...]"),
    help_requested: bool = typer.Option(
        False, "--help", "-h", help="Show help for this command or the named operation."
    ),
    netuid: Optional[int] = typer.Option(None, "--netuid", help="Subnet of the service."),
    inputs: Optional[str] = typer.Option(
        None,
        "--inputs",
        metavar="JSON",
        help="The inputs as one JSON object, by name (@FILE, @- for stdin); flags add to it.",
    ),
    out: Optional[Path] = typer.Option(
        None,
        "--out",
        help="Write the response body to this file (- for stdout). A binary response "
        "without it goes to a temporary file.",
    ),
    service_hotkey: Optional[str] = _service_option(),
    body: Optional[str] = typer.Option(
        None,
        "--body",
        help="The whole body: JSON, or the text or bytes of a body that is not JSON "
        "(@FILE, @- for stdin).",
    ),
    files: Optional[list[str]] = typer.Option(
        None,
        "--file",
        metavar="NAME=PATH",
        help="Send a file as the file input NAME (PATH - for stdin); repeat it for more.",
    ),
    dry_run: bool = typer.Option(False, "--dry-run", help="Validate and preview without sending."),
    unsigned: bool = typer.Option(
        False, "--unsigned", help="Call an optional operation without signing."
    ),
    timeout: float = typer.Option(
        services.TIMEOUT, "--timeout", help="Seconds the call may take.", min=0.1
    ),
    url: Optional[str] = _url_option(),
):
    app_ctx: AppContext = ctx_of(ctx)
    # Click accepts the next option as a missing value, e.g. --out --dry-run.
    # A safety switch consumed this way must never turn into a live call.
    scope = ctx
    while scope:
        if any(
            str(value) in {"--dry-run", "--help", "-h", "--unsigned"}
            for value in scope.params.values()
        ):
            _usage(app_ctx, "a client option is missing its value")
        scope = scope.parent
    if help_requested and operation is None:
        typer.echo(ctx.get_help())
        return
    service = _service(app_ctx, netuid, service_hotkey, url)
    if operation is None:
        with _rendering(app_ctx):
            _show(app_ctx, service)
        app_ctx.output.message(
            f"call one with `{_service_command(app_ctx, service, 'call', 'OPERATION')}`"
        )
        return
    try:
        op = service.operation(operation)
    except services.ServiceError as error:
        _fail(
            app_ctx,
            error,
            f"list its operations with `{_service_command(app_ctx, service, 'show')}`",
        )
    if help_requested:
        with _rendering(app_ctx):
            _help(app_ctx, service, op, ctx)
        return
    if not 0 < timeout < threading.TIMEOUT_MAX:
        _usage(app_ctx, f"--timeout must be positive and below {threading.TIMEOUT_MAX:g} seconds")
    if str(out) == "-" and app_ctx.output.json_mode:
        _usage(app_ctx, "--out - writes raw bytes; choose it or --json")
    reserved = {flag for param in ctx.command.params for flag in param.opts}
    for token in ctx.args:
        if token.partition("=")[0] in reserved:
            _usage(app_ctx, f"pass {token!r} before -- or use --inputs for provider fields")
    resolve = op._resolver()
    with _rendering(app_ctx):
        values = _inputs(app_ctx, service, op, ctx.args, inputs, resolve)
    body_kw = {}
    if body is not None:
        if op.body_kind == "raw":
            body_kw["body"] = _read_bytes(app_ctx, service, op, body)
        else:
            raw = _loads(_read(app_ctx, service, op, body))
            if raw is _NOT_JSON:
                _input_error(app_ctx, service, op, "--body takes a JSON value")
            body_kw["body"] = raw
    for item, file in _files(app_ctx, service, op, files or []):
        name = item.name
        if item is op.body_input:
            if "body" in body_kw:
                _input_error(app_ctx, service, op, "the body is given twice")
            body_kw["body"] = file
        else:
            with _rendering(app_ctx):
                multiple = _types(resolve(item.schema)) == {"array"}
            if multiple:
                values.setdefault(name, []).append(file)
            elif name in values:
                _input_error(app_ctx, service, op, f"{name!r} is given twice")
            else:
                values[name] = file
    try:
        prepared = op.prepare(values, **body_kw)
    except services.ServiceError as error:
        _input_error(app_ctx, service, op, str(error))
    if op.auth == "hotkey" and unsigned:
        _input_error(app_ctx, service, op, f"{op.command} is always signed; leave out --unsigned")
    # Signing reveals the hotkey and lets the service charge it, so only a hotkey named
    # on purpose signs an optional operation.
    signing = op.auth == "hotkey" or (
        op.auth == "optional" and not unsigned and (app_ctx.wallet_given or app_ctx.hotkey_given)
    )
    if app_ctx.dry_run or dry_run:
        method, url, _, headers, data = prepared
        preview = {
            "dry_run": True,
            "operation": op.command,
            "method": method,
            "url": url,
            "headers": sorted(set(headers) - {"User-Agent", "Content-Type"}),
            "content_type": headers.get("Content-Type"),
            "body": _preview(op, data),
            "auth": op.auth,
            "signs": signing,
            "netuid": service.netuid,
            "service_hotkey": service.hotkey,
        }
        app_ctx.output.detail("dry run", _plain(preview))
        return
    if signing:
        try:
            service._check_signable(prepared[1])
        except services.ServiceError as error:
            hint = "; --unsigned sends it unsigned" if op.auth == "optional" else ""
            _input_error(app_ctx, service, op, f"{error}{hint}")
    staged = None
    cleanup = None
    if out is not None and str(out) != "-":
        if os.path.lexists(out) and (out.is_symlink() or not out.is_file()):
            _input_error(app_ctx, service, op, f"cannot write to {out}: name a regular file")
        try:
            staged, cleanup = ctx.with_resource(_stage(out))
        except OSError as error:
            _input_error(app_ctx, service, op, f"cannot write to {out}: {error}")
    key = _hotkey(app_ctx) if signing else None

    def explain(error: services.ServiceError) -> Optional[str]:
        if error.code == "too_large":
            return "`--out FILE` writes a response of any size as it arrives"
        if error.code == "timeout":
            return "check the outcome before retrying; --timeout SECONDS allows a longer wait"
        if error.status is not None:  # the service answered, so it may have done the work
            return "check the outcome before retrying"
        return None

    sink: Optional[_Sink] = None

    def into(head: services.Response) -> Optional[IO[bytes]]:
        """Where a successful body goes as it arrives: the file it was asked for, the
        terminal for a stream of events, a file for one that is not text, or None to
        read it whole."""
        nonlocal sink
        temporary = None
        if out is not None:
            target: IO[bytes] = sys.stdout.buffer if str(out) == "-" else staged
        elif _is_stream(head):
            target = _Kept() if app_ctx.output.json_mode else _Echo()
        elif not _is_text(head) or _too_large(head):
            target = ctx.with_resource(_temporary(_suffix(head)))
            temporary = Path(target.name)
        else:
            return None
        sink = _Sink(target, temporary)
        return sink

    try:
        response = _signing(
            app_ctx,
            key,
            lambda signer: service._send_prepared(
                op, prepared, signer=signer, timeout=timeout, into=into
            ),
            f"calling {_short(service.name, 40)}…",
            explain,
        )
    except BaseException:
        if sink is not None:
            sink.discard()
        raise
    if sink is not None and isinstance(sink.target, _Kept):
        response = dataclasses.replace(response, content=sink.target.getvalue())
        sink = None
    if op.auth == "optional":
        app_ctx.output.message(
            f"signed by hotkey {key.ss58_address}; --unsigned sends it unsigned"
            if key
            else "sent unsigned"
            + ("" if unsigned or not service.signable else "; -w and -H sign it with your hotkey")
        )
    with _rendering(app_ctx):
        _show_response(
            app_ctx,
            service,
            op,
            response,
            out,
            staged,
            cleanup,
            signed=key.ss58_address if key else None,
            sink=sink,
        )


@app.command(
    "balance",
    help="Show what a hotkey can still spend at a service.\n\n"
    "Reads your wallet's hotkey (-w, -H), signing when the service asks for it. --hotkey "
    "reads any other hotkey without a wallet, where the service shows balances to anyone.",
)
@with_globals
def balance(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet of the service."),
    hotkey: Optional[str] = typer.Option(
        None, "--hotkey", help="Read this hotkey's balance (an address), not your wallet's."
    ),
    service_hotkey: Optional[str] = _service_option(),
):
    app_ctx: AppContext = ctx_of(ctx)
    hotkey_ss58 = _address(app_ctx, "hotkey_ss58", hotkey) if hotkey else None
    publisher = _address(app_ctx, "service_hotkey_ss58", service_hotkey) if service_hotkey else None
    service = _attempt(
        app_ctx,
        lambda client: services.load_document(
            client, netuid, publisher, cache=cfg.service_documents_path()
        ),
    )
    key = None if hotkey_ss58 else _hotkey(app_ctx, or_else="--hotkey ADDRESS")
    address = hotkey_ss58 or key.ss58_address

    def explain(error: services.ServiceError) -> Optional[str]:
        if error.status == 402 and not hotkey_ss58:  # a spend pays for the hotkey that signs it
            return _pay_help(app_ctx, service)
        if error.status in (401, 403) and hotkey_ss58:
            return (
                "the service shows a balance only to its hotkey; "
                "leave out --hotkey to read your own, signed"
            )
        return None

    label = f"reading {_short(service.name, 40)}…"

    async def unsigned() -> Optional[dict]:
        """The balance read openly; None when the service asks for a signature."""
        try:
            return await service.balance(address)
        except services.ServiceError as error:
            if key is not None and error.status in (401, 403):
                return None
            raise

    record = _offline(app_ctx, unsigned(), label, explain)
    if record is None:
        record = _signing(
            app_ctx, key, lambda signer: service.balance(address, signer=signer), label, explain
        )
    with _rendering(app_ctx):
        if app_ctx.output.json_mode:
            app_ctx.output.value(record)
        else:
            _render(app_ctx.output, f"balance at {_line(service.name)} on netuid {netuid}", record)


Explain = Callable[[services.ServiceError], Optional[str]]

_HELP = {
    "no_service": "`btcli service list` shows the subnets that have one",
    "ambiguous": "name one with --service-hotkey; `btcli service list` shows them",
    "unusable": "its publisher has to fix this; `btcli service list` shows the services that work",
}


def _fail(
    app_ctx: AppContext, error: services.ServiceError, help: Optional[str] = None
) -> NoReturn:
    """Report a ServiceError by its code and exit: 2 when the command does not fit,
    as for any usage error, else 1."""
    data: dict[str, Any] = {"code": error.code}
    if error.status is not None:
        data["status"] = error.status
    app_ctx.output.error(str(error), help=help or _HELP.get(error.code), data=data)
    raise typer.Exit(2 if error.code == "input" else 1)


def _service(
    app_ctx: AppContext,
    netuid: Optional[int],
    service_hotkey: Optional[str],
    url: Optional[str] = None,
) -> services.Service:
    hotkey = _address(app_ctx, "service_hotkey_ss58", service_hotkey) if service_hotkey else None
    if url is not None:
        return _offline(
            app_ctx, services.load_url(url, netuid, hotkey), f"reading {_short(url, 60)}…"
        )
    if netuid is None:
        _usage(
            app_ctx,
            "missing required option: `--netuid`",
            help="name the subnet, as in `btcli service show --netuid 19`; "
            "`btcli service list` shows the services",
        )
    return _attempt(
        app_ctx,
        lambda client: services.load(client, netuid, hotkey, cache=cfg.service_documents_path()),
    )


def _signing(
    app_ctx: AppContext,
    key,
    work: Callable[[Any], Awaitable[T]],
    label: str,
    explain: Optional[Explain] = None,
) -> T:
    """``work(signer)``, with the chain only for a hashed hotkey, whose signing
    generation is read from it; an ordinary one, or none, signs without it."""
    if key is not None and key.crypto_type in http_auth.CLASSICAL_HASHED_CRYPTO_TYPES:

        async def _prepared(client) -> T:
            return await work(await http_auth.prepare_signer(key, client=client))

        return _attempt(app_ctx, _prepared, explain)

    async def _without_chain() -> T:
        return await work(None if key is None else await http_auth.prepare_signer(key, client=None))

    return _offline(app_ctx, _without_chain(), label, explain)


def _offline(
    app_ctx: AppContext, work: Awaitable[T], label: str, explain: Optional[Explain] = None
) -> T:
    """``work`` without the chain, its ServiceError reported."""
    try:
        with app_ctx.output.activity(label):
            return asyncio.run(work)  # ty: ignore[invalid-argument-type]
    except KeyboardInterrupt:
        app_ctx.output.message("aborted.")
        raise typer.Exit(130) from None
    except services.ServiceError as error:
        _fail(app_ctx, error, explain(error) if explain else None)
    except ValueError as error:  # a hotkey that cannot sign, as a post-quantum one
        app_ctx.output.error(str(error), data={"code": "wallet"})
        raise typer.Exit(1) from None


def _attempt(
    app_ctx: AppContext, work: Callable[[Any], Awaitable[T]], explain: Optional[Explain] = None
) -> T:
    """``app_ctx.run(work)``, its ServiceError reported after the chain is closed."""

    async def _work(client) -> Any:
        try:
            return await work(client)
        except services.ServiceError as error:
            return error

    result = app_ctx.run(_work)
    if isinstance(result, services.ServiceError):
        _fail(app_ctx, result, explain(result) if explain else None)
    return result


def _hotkey(app_ctx: AppContext, *, or_else: str = ""):
    """The wallet's hotkey, loaded before the request is sent, so a missing or bad key
    file is reported as one. Ledger, Vault and the extension sign transactions, not
    requests, so naming one stops the call rather than signing with the wallet."""
    if app_ctx.uses_external_signer():
        app_ctx.output.error(
            f"--signer {app_ctx.signer_backend} cannot sign a service request",
            help="a hotkey in your wallet signs it: name it with -w and -H, without --signer",
            data={"code": "wallet"},
        )
        raise typer.Exit(1)
    try:
        signer = http_auth.resolve_signer(app_ctx.signer("hotkey"), role="hotkey")
        if not signer.ss58_address:  # reading it reads the key file
            raise ValueError("the hotkey has no address")
    except (OSError, ValueError, KeyfileError, BittensorError) as error:
        app_ctx.output.error(
            f"cannot use hotkey {app_ctx.hotkey_name!r} of wallet {app_ctx.wallet_name!r}: {error}",
            help="name the wallet and hotkey with -w and -H"
            + (f", or use {or_else}" if or_else else ""),
            data={"code": "wallet"},
        )
        raise typer.Exit(1) from None
    return signer


# Inputs: flags in, typed values out.


def _usage(app_ctx: AppContext, text: str, *, help: Optional[str] = None) -> NoReturn:
    app_ctx.output.error(text, help=help, data={"code": "input"})
    raise typer.Exit(2)


def _address(app_ctx: AppContext, param: str, value: str) -> str:
    """The address a flag names, one it cannot resolve an input error."""
    try:
        return app_ctx.resolve_address_ref(param, value).address
    except Exception as error:
        _usage(app_ctx, f"cannot resolve {address_cli_name(param)} {value!r}: {error}")


def _inputs(app_ctx, service, op, args, inputs, resolve) -> dict[str, Any]:
    """Decode the operation's flags and combine them with named JSON inputs."""
    by_option = {_option(item): item for item in op.inputs}
    for item in op.inputs:  # the spec's own spelling works too: --gpuType, --gpu_type
        by_option.setdefault("--" + item.name, item)
    values: dict[str, Any] = {}
    rest = iter(args)
    for token in rest:
        flag, has_value, inline = token.partition("=")
        item = by_option.get(flag)
        negated = False
        if item is None and flag.startswith("--no-"):
            item = by_option.get("--" + flag[len("--no-") :])
            negated = item is not None
        if item is None:
            close = difflib.get_close_matches(flag, [_option(i) for i in op.inputs], n=2)
            _input_error(
                app_ctx,
                service,
                op,
                f"{op.command} has no input {_short(token, 60)!r}"
                + (f"; did you mean {', '.join(close)}?" if close else ""),
            )
        if item.binary:
            _input_error(app_ctx, service, op, f"{flag} takes a file: --file {item.name}=PATH")
        schema = resolve(item.schema)
        types = _types(schema)
        if types == {"boolean"} and not has_value:
            value: Any = not negated
        else:
            if negated:
                _input_error(app_ctx, service, op, f"{flag} takes no value")
            if not has_value:
                inline = _next(app_ctx, service, op, rest, flag, by_option)
            value = _value(app_ctx, service, op, item, schema, inline, resolve)
        if types == {"array"} and value is not None and values.get(item.name, []) is not None:
            values.setdefault(item.name, []).extend(value)
        elif item.name in values:
            _input_error(app_ctx, service, op, f"{flag} is given twice")
        else:
            values[item.name] = value
    return {**_given(app_ctx, service, op, inputs, by_option, resolve), **values}


def _next(app_ctx, service, op, rest, flag: str, by_option: dict) -> str:
    value = next(rest, None)
    if value is None:
        _input_error(app_ctx, service, op, f"{flag} needs a value")
    name = value.partition("=")[0]
    if name in by_option or (name.startswith("--no-") and "--" + name[5:] in by_option):
        _input_error(app_ctx, service, op, f"{flag} needs a value ({flag}={value} if it is one)")
    return value


def _given(app_ctx, service, op, text: Optional[str], by_option: dict, resolve) -> dict[str, Any]:
    """The inputs ``--inputs`` gives: a JSON object of them by name."""
    if text is None:
        return {}
    record = _loads(_read(app_ctx, service, op, text))
    if not isinstance(record, dict):
        _input_error(app_ctx, service, op, "--inputs takes a JSON object of the inputs by name")
    values = {}
    for key, value in record.items():
        item = by_option.get("--" + key)
        name = item.name if item is not None else key
        if name in values:
            _input_error(app_ctx, service, op, f"{name!r} is given twice")
        schema = resolve(item.schema) if item is not None and not item.binary else None
        if schema is not None and (value is not None or not _nullable(schema)):
            if _types(schema) == {"array"} and isinstance(value, list):
                items = resolve(schema.get("items") or {})
                for each in value:
                    _typed(app_ctx, service, op, item, items, each)
            else:
                _typed(app_ctx, service, op, item, schema, value)
        values[name] = value
    return values


def _read(app_ctx, service, op, text: str) -> str:
    """JSON as written, or read from the file an @ names (@- is stdin). Only --inputs,
    --body and --file read files: a JSON value never starts with @, while a flag's text
    can, and an agent a spec steers must not send a local file by filling in an input."""
    try:
        return _read_bytes(app_ctx, service, op, text).decode("utf-8")
    except UnicodeDecodeError as error:
        _input_error(app_ctx, service, op, f"cannot read {text}: {error}")


def _read_bytes(app_ctx, service, op, text: str) -> bytes:
    """Text as written, or the bytes of the file an @ names (@- is stdin)."""
    if not text.startswith("@"):
        return text.encode()
    return _file_bytes(app_ctx, service, op, text[1:], text)


def _file_bytes(app_ctx, service, op, path: str, given: str) -> bytes:
    try:
        if path == "-":
            return sys.stdin.buffer.read()
        return Path(path).expanduser().read_bytes()
    except OSError as error:
        _input_error(app_ctx, service, op, f"cannot read {given}: {error}")


def _files(app_ctx, service, op, given: list[str]) -> list[tuple[services.Input, Any]]:
    """The file inputs ``--file NAME=PATH`` names, each as its input and value:
    a file input takes its filename and bytes, a body that is not JSON its bytes."""
    takers = [i for i in op.inputs if i.binary]
    if op.body_input is not None and op.body_kind == "raw":
        takers.append(op.body_input)
    by_name = {key: item for item in takers for key in (item.sent_as, item.command, item.name)}
    found = []
    for text in given:
        name, _, path = text.partition("=")
        item = by_name.get(name)
        if not path:
            _input_error(app_ctx, service, op, f"--file takes NAME=PATH, not {_short(text, 60)!r}")
        if item is None:
            names = ", ".join(dict.fromkeys(i.name for i in takers))
            _input_error(
                app_ctx,
                service,
                op,
                f"{op.command} has no file input {_short(name, 60)!r}"
                + (f"; its file inputs are {names}" if names else ""),
            )
        data = _file_bytes(app_ctx, service, op, path, text)
        if item is op.body_input:
            found.append((item, data))
        else:
            found.append((item, (item.name if path == "-" else Path(path).name, data)))
    return found


def _preview(op: services.Operation, data: bytes) -> Any:
    """A request body as a dry run shows it: JSON, or the text of a form or of a text
    body, else its size."""
    if not data:
        return None
    if op.body_kind == "json":
        with contextlib.suppress(ValueError):
            return json.loads(data)
    binary = op.body_kind == "multipart" or (op.body_input is not None and op.body_input.binary)
    if not binary:
        with contextlib.suppress(UnicodeDecodeError):
            return data.decode("utf-8")
    return f"{len(data)} bytes"


def _value(app_ctx, service, op, item: services.Input, schema: Any, text: str, resolve) -> Any:
    """A flag's value: text for a text input, JSON for any other type (text when it is
    untyped and not JSON). An array input takes one item, or a JSON array of them."""
    if text == "null" and _nullable(schema) and "string" not in _types(schema):
        return None
    if _types(schema) != {"array"}:
        return _typed(app_ctx, service, op, item, schema, _parse(text, schema))
    items = resolve(schema.get("items") or {})
    whole = _loads(text)
    values = whole if isinstance(whole, list) else [_parse(text, items)]
    return [_typed(app_ctx, service, op, item, items, value) for value in values]


def _branches(schema, key):
    value = schema.get(key)
    return value if isinstance(value, list) else []


def _nullable(schema: Any) -> bool:
    if not isinstance(schema, dict):
        return False
    kind = schema.get("type")
    return (
        schema.get("nullable") is True
        or kind == "null"
        or (isinstance(kind, list) and "null" in kind)
        or any(_nullable(sub) for key in ("anyOf", "oneOf") for sub in _branches(schema, key))
    )


_NOT_JSON = object()


def _parse(text: str, schema: Any) -> Any:
    types = _types(schema)
    if types == {"string"}:
        return text
    value = _loads(text)
    if "string" in types:
        kind = _json_type(value)
        if kind != "string" and value is not _NOT_JSON and kind in types:
            return value
        return text
    if not types:  # untyped: JSON when it is, else text
        return text if value is _NOT_JSON else value
    return None if value is _NOT_JSON else value


def _loads(text: str) -> Any:
    """``text`` as JSON, or _NOT_JSON; NaN and Infinity are not JSON."""
    try:
        return json.loads(text, parse_constant=_not_json)
    except (ValueError, RecursionError):
        return _NOT_JSON


def _typed(app_ctx, service, op, item: services.Input, schema: Any, value: Any):
    types = _types(schema)
    kind = _json_type(value)
    if types and not (kind in types or (kind == "integer" and "number" in types)):
        _input_error(
            app_ctx,
            service,
            op,
            f"{_option(item)} takes {_placeholder(schema)}",
        )
    enum = _enum(schema)
    if enum and not any(type(value) is type(choice) and value == choice for choice in enum):
        _input_error(app_ctx, service, op, f"{_option(item)} takes {_placeholder(schema)}")
    return value


def _input_error(app_ctx, service, op, text: str) -> NoReturn:
    command = _service_command(app_ctx, service, "call", op.command, "--help")
    _usage(app_ctx, _line(text), help=f"`{command}` lists its inputs")


def _types(schema: Any) -> set[str]:
    """The JSON types a schema allows, without null."""
    if not isinstance(schema, dict):
        return set()
    kind = schema.get("type")
    kinds = kind if isinstance(kind, list) else [kind]
    types = {t for t in kinds if isinstance(t, str)}
    for key in ("anyOf", "oneOf", "allOf"):
        for sub in _branches(schema, key):
            types |= _types(sub)
    if not types and isinstance(schema.get("enum"), list):
        types = {_json_type(v) for v in schema["enum"]}
    if not types and "const" in schema:
        types = {_json_type(schema["const"])}
    if not types and isinstance(schema.get("properties"), dict):
        types = {"object"}
    types.discard("null")
    return types


def _json_type(value: Any) -> str:
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, list):
        return "array"
    return "string" if isinstance(value, str) else "null" if value is None else "object"


def _enum(schema: Any) -> list:
    if not isinstance(schema, dict):
        return []
    values = list(schema["enum"]) if isinstance(schema.get("enum"), list) else []
    if "const" in schema:
        values.append(schema["const"])
    for key in ("anyOf", "oneOf"):
        branches = _branches(schema, key)
        if branches:
            choices = [_enum(sub) for sub in branches]
            if not all(choices):
                return values
            values.extend(value for branch in choices for value in branch)
    return [v for v in values if v is not None]


def _placeholder(schema: Any) -> str:
    """How an input's value is written: its choices, TEXT, or JSON of its type."""
    types = _types(schema)
    if types == {"array"} and isinstance(schema, dict):
        return _placeholder(schema.get("items") or {}) + "…"
    choices = _enum(schema)
    if choices:
        return "|".join(_line(_cell(v)) for v in choices)
    if "string" in types:
        return "TEXT"
    return "|".join(sorted(t.upper() for t in types)) or "JSON"


# What is shown.


def _groups(service: services.Service) -> list[tuple[str, list[services.Operation]]]:
    """Operations by their first tag, in the spec's tag order, as Swagger UI lists them."""
    groups: dict[str, list[services.Operation]] = {tag: [] for tag in service.tags}
    for op in service.operations.values():
        groups.setdefault(op.tags[0] if op.tags else "", []).append(op)
    if list(groups) == [""] or not any(groups.values()):
        return [("operations", groups.get("", []))]
    return [
        (_short(_tag_title(tag, service.tags.get(tag, "")), 72), ops)
        for tag, ops in groups.items()
        if ops
    ]


def _tag_title(tag: str, description: str) -> str:
    """A tag and the first sentence of its description, as plain text."""
    if not tag:
        return "other"
    # The title shows a short first sentence; cutting first keeps the match linear.
    plain = _line(re.sub(r"[*_`#>]+|\[([^\]]*)\]\([^)]*\)", r"\1", description[:500]))
    sentence = re.split(r"(?<=[.!?])\s", plain, maxsplit=1)[0].rstrip(".")
    return _line(tag) + (f": {sentence}" if sentence else "")


_AUTH = {
    "hotkey": "requires your hotkey signature",
    "optional": "accepts a hotkey signature (-w, -H) or an unsigned call",
    "open": "open to anyone",
}


def _help(app_ctx: AppContext, service: services.Service, op: services.Operation, ctx) -> None:
    output = app_ctx.output
    if output.json_mode:
        output.value(_definition(op, service.tool(op.id)))
        return
    reserved = {flag for param in ctx.command.params for flag in param.opts}
    rows = []
    needed = []  # the required inputs as flags, for the usage line
    resolve = op._resolver()
    for item in op.inputs:
        schema = resolve(item.schema)
        schema = schema if isinstance(schema, dict) else {}
        default = schema.get("const", schema.get("default"))
        flag = _option(item)
        takes_file = item.binary or (item.location == "body" and op.body_kind == "raw")
        if takes_file:
            flag = f"--file {item.name}=PATH"
        elif flag in reserved:
            flag = f"--inputs ({item.name})"
        elif _types(schema) == {"boolean"}:
            flag = f"{_option(item)} / --no-{_option(item)[2:]}"
        value = (
            "FILE" if takes_file else "" if _types(schema) == {"boolean"} else _placeholder(schema)
        )
        if item.must_supply and not flag.startswith("--inputs"):
            needed.append(flag if takes_file or not value else f"{flag} {value}")
        rows.append(
            [
                _line(flag),
                value,
                "yes" if item.required and default is None else "",
                "" if default is None else _line(_cell(default)),
                _short(item.description or _line(_cell(schema.get("example") or ""))),
            ]
        )
    command = _service_command(app_ctx, service, "call", op.command)
    output.detail(
        _line(op.command),
        _fields(
            {
                "summary": op.summary,
                "description": op.description,
                "auth": _AUTH[op.auth],
                "usage": " ".join([command, *needed, "[INPUTS]"]),
                "or": f"{command} --inputs '{_example(op)}'",
            }
        ),
    )
    if rows:
        typer.echo()
        output.columns("inputs", ["input", "value", "required", "default", "description"], rows)


def _example(op: services.Operation) -> str:
    """The required inputs as ``--inputs`` and a tool take them, a body's required
    fields inside it."""
    fields = [i.sent_as for i in op.inputs if i.location == "field" and i.required_in_body]
    parts = []
    for item in op.tool_inputs:
        if item.must_supply and item is op.body_input and fields:
            parts.append(f'"{item.name}": {{' + ", ".join(f'"{f}": …' for f in fields) + "}")
        elif item.must_supply:
            parts.append(f'"{item.name}": …')
    return "{" + ", ".join(parts) + "}" if parts else '{"NAME": VALUE, ...}'


@contextlib.contextmanager
def _stage(out: Path):
    """Reserve a sibling file before the call. Keep it only if the final rename fails."""
    with (
        contextlib.ExitStack() as cleanup,
        tempfile.NamedTemporaryFile(dir=out.parent, prefix=".btcli-", delete=False) as staged,
    ):
        cleanup.callback(Path(staged.name).unlink, missing_ok=True)
        yield staged.file, cleanup


def _show_response(
    app_ctx: AppContext,
    service: services.Service,
    op: services.Operation,
    response: services.Response,
    out: Optional[Path],
    staged: Optional[IO[bytes]],
    cleanup: Optional[contextlib.ExitStack],
    *,
    signed: Optional[str],
    sink: Optional[_Sink] = None,
) -> None:
    """Show a response; under --json always as ``{status, content_type, signed, ...}``
    with its ``body`` (JSON), ``text``, or the file it went ``out`` to. ``sink`` is
    where its body already went as it arrived."""
    output = app_ctx.output
    about = {
        "status": response.status,
        "content_type": response.content_type,
        "signed": signed,
    }
    if not response.ok:
        _http_error(app_ctx, service, response, about, open_to_all=op.auth == "open")
    if sink is not None and isinstance(sink.target, _Echo):
        typer.echo()
        return
    if out is not None and str(out) == "-":
        if sink is None:
            try:
                sys.stdout.buffer.write(response.content)
                sys.stdout.flush()
            except OSError as error:
                _recover_response(output, response, "stdout", error)
        return
    size = sink.written if sink is not None else len(response.content)
    parsed = _parsed(response) if out is None and sink is None else _NOT_JSON
    if staged is not None and out is not None and cleanup is not None:
        path = Path(staged.name)
        try:
            with staged:
                if sink is None:
                    staged.write(response.content)
        except OSError as error:
            _recover_response(output, response, str(out), error)
        try:
            path.replace(out)
        except OSError as error:
            cleanup.pop_all()  # Keep the complete response for recovery.
            output.error(
                f"cannot replace {out}: {error}",
                data={"code": "output", "out": str(path)},
                help=f"the response was saved to {path}",
            )
            raise typer.Exit(1) from None
    elif sink is not None and sink.temporary is not None:
        sink.close()
        out = sink.temporary
    elif out is None and parsed is _NOT_JSON and response.content and not _is_text(response):
        out = _save_response(response)
    if out is not None:
        output.detail(
            "response",
            {"out": str(out), "bytes": size, "content type": _line(response.content_type)},
            {**about, "bytes": size, "out": str(out)},
        )
    elif parsed is not _NOT_JSON:
        if output.json_mode:
            output.value({**about, "body": parsed})
        else:
            _render(output, _line(op.summary) or op.command, parsed)
    elif not response.content:
        if output.json_mode:
            output.value(about)
        else:
            output.message(f"done (HTTP {response.status})")
    elif output.json_mode:
        output.value({**about, "text": response.text})
    else:  # `--out -` writes it as it is
        typer.echo(_plain(response.text))


# Responses that are a stream of events, shown as they arrive.
_STREAMS = frozenset(
    [
        "text/event-stream",
        "application/x-ndjson",
        "application/ndjson",
        "application/jsonl",
        "application/x-jsonlines",
        "application/stream+json",
    ]
)


def _is_stream(response: services.Response) -> bool:
    return response.content_type in _STREAMS


def _too_large(response: services.Response) -> bool:
    """Whether a body says it is larger than a response held in memory may be."""
    length = response.headers.get("content-length", "")
    return length.isdigit() and int(length) > services.MAX_RESPONSE_BYTES


def _suffix(response: services.Response) -> str:
    return mimetypes.guess_extension(response.content_type) or ".bin"


class _Sink:
    """Where a body goes as it arrives, counting what it wrote."""

    def __init__(self, target: IO[bytes], temporary: Optional[Path] = None):
        self.target = target
        self.temporary = temporary  # the file btcli made for it, if it made one
        self.written = 0

    def write(self, data: bytes) -> int:
        self.target.write(data)
        self.written += len(data)
        return len(data)

    def flush(self) -> None:
        with contextlib.suppress(OSError):
            self.target.flush()

    def close(self) -> None:
        if self.temporary is not None:
            self.target.close()

    def discard(self) -> None:
        if self.temporary is not None:
            self.target.close()
            self.temporary.unlink(missing_ok=True)


class _Kept(io.BytesIO):
    """A stream of events kept whole for --json, as large as a response may be."""

    def write(self, data) -> int:
        if self.tell() + len(data) > services.MAX_RESPONSE_BYTES:
            raise services.ServiceError(
                f"the response is over {services.MAX_RESPONSE_BYTES >> 20} MiB", code="too_large"
            )
        return super().write(data)


class _Echo:
    """A stream of events printed as it arrives, without the controls a terminal acts on."""

    def __init__(self) -> None:
        self._text = codecs.getincrementaldecoder("utf-8")("replace")

    def write(self, data: bytes) -> int:
        sys.stdout.write(_plain(self._text.decode(data)))
        return len(data)

    def flush(self) -> None:
        sys.stdout.flush()


def _temporary(suffix: str):
    return tempfile.NamedTemporaryFile(prefix="btcli-service-", suffix=suffix, delete=False)


def _save_response(response: services.Response) -> Path:
    with _temporary(_suffix(response)) as file:
        path = Path(file.name)
        try:
            file.write(response.content)
            file.flush()
        except OSError:
            file.close()
            path.unlink(missing_ok=True)
            raise
    return path


def _recover_response(output, response: services.Response, target: str, error: OSError) -> NoReturn:
    backup = _save_response(response)
    output.error(
        f"cannot write to {target}: {error}",
        data={"code": "output", "out": str(backup)},
        help=f"the response was saved to {backup}",
    )
    raise typer.Exit(1) from None


def _is_text(response: services.Response) -> bool:
    kind = response.content_type
    if kind:
        return kind.startswith("text/") or kind.endswith(("json", "xml", "javascript"))
    try:
        text = response.content.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return all(ch.isprintable() or ch in "\t\r\n" for ch in text)


def _render(output: Output, title: str, value: Any) -> None:
    """Show flat records with the standard CLI views and structured values as YAML."""
    if isinstance(value, dict) and value and _flat_record(value):
        output.detail(title, {key: _cell(item) for key, item in value.items()})
        return
    if isinstance(value, list):
        if not value:
            output.columns(title, [], [])
            return
        first = value[0]
        if (
            isinstance(first, dict)
            and 0 < len(first) <= MAX_COLUMNS
            and all(
                isinstance(row, dict) and row.keys() == first.keys() and _flat_record(row)
                for row in value
            )
        ):
            columns = [str(key) for key in first]
            output.columns(title, columns, [[_cell(row[key]) for key in columns] for row in value])
            return
    typer.echo(_plain(value) if isinstance(value, str) else _yaml(value))


def _yaml(value: Any) -> str:
    """Nested data as YAML, which shows text of several lines, as an answer a model
    wrote, as lines and not escaped."""
    import yaml

    dumper = type("Dumper", (yaml.SafeDumper,), {})
    dumper.add_representer(
        str,
        lambda dumper, text: dumper.represent_scalar(
            "tag:yaml.org,2002:str", text, style="|" if "\n" in text else None
        ),
    )
    text = yaml.dump(
        _plain(value), Dumper=dumper, allow_unicode=True, sort_keys=False, width=1 << 30
    )
    return text.rstrip("\n")


def _flat_record(record: dict[str, Any]) -> bool:
    return all(
        key.strip()
        and key.isprintable()
        and not isinstance(value, (dict, list))
        and (not isinstance(value, str) or value.isprintable())
        for key, value in record.items()
    )


def _cell(value: Any) -> str:
    return value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)


def _http_error(
    app_ctx: AppContext,
    service: services.Service,
    response: services.Response,
    about: dict,
    *,
    open_to_all: bool,
) -> NoReturn:
    """Report an HTTP error; no key or payment changes one from an open operation."""
    text = (
        response.message
        if _is_text(response)
        else f"{len(response.content)} bytes of {response.content_type or 'binary'}"
    )
    note = _line(text[:MAX_ERROR] + ("…" if len(text) > MAX_ERROR else "")).replace("`", "'")
    help = None
    if open_to_all:
        pass
    elif response.status in (401, 402, 403) and not about["signed"]:
        help = "sign it with your hotkey: name it with -w and -H, without --unsigned"
    elif response.status == 402:
        help = _pay_help(app_ctx, service)
    elif response.status in (401, 403) and service.owner is not None:
        help = f"check your hotkey's balance with `{_service_command(app_ctx, service, 'balance')}`"
    body = _parsed(response)
    if body is _NOT_JSON:  # a binary body has no place in a message
        body = response.text if _is_text(response) else None
    app_ctx.output.error(
        f"HTTP {response.status} from {_line(service.name)}",
        note=f"the service says: {note}" if note else None,
        help=help,
        data={"code": "http", **about, "body": body},
    )
    raise typer.Exit(1)


def _command(app_ctx, *args):
    words = ["btcli"]
    if app_ctx.network != (os.getenv("BT_NETWORK") or cfg.get("network", DEFAULT_NETWORK)):
        words.extend(["--network", app_ctx.network])
    if app_ctx.wallet_path != (
        os.getenv("BT_WALLET_PATH") or cfg.get("wallet_path", wallets.DEFAULT_WALLET_PATH)
    ):
        words.extend(["--wallet-path", str(app_ctx.wallet_path)])
    if app_ctx.wallet_given:
        words.extend(["-w", app_ctx.wallet_name])
    if app_ctx.hotkey_given:
        words.extend(["-H", app_ctx.hotkey_name])
    return shlex.join([*words, *args])


def _service_command(app_ctx, service, verb, *args):
    selectors = ["--netuid", str(service.netuid)]
    if service.owner is not True:
        selectors.extend(["--service-hotkey", service.hotkey])
    if service.owner is None:
        selectors.extend(["--url", service.url])
    return _command(app_ctx, "service", verb, *selectors, *args)


def _pay_command(app_ctx, service) -> Optional[str]:
    if service.owner is None:
        return None
    return _command(
        app_ctx,
        "gamma",
        "spend",
        "--netuid",
        str(service.netuid),
        "--service-hotkey",
        service.hotkey,
        "--amount-gamma",
        "AMOUNT",
    )


def _pay_help(app_ctx, service) -> Optional[str]:
    command = _pay_command(app_ctx, service)
    return f"pay the service with `{command}`" if command else None


@contextlib.contextmanager
def _rendering(app_ctx: AppContext):
    """Report what the service sent that cannot be shown, instead of a traceback."""
    try:
        yield
    except services.ServiceError as error:
        _fail(app_ctx, error)
    except (ValueError, RecursionError, OSError) as error:
        app_ctx.output.error(_line(str(error)), data={"code": "output"})
        raise typer.Exit(1) from None


def _parsed(response: services.Response) -> Any:
    if not response.is_json:
        return _NOT_JSON
    try:
        return response.json()
    except (ValueError, RecursionError):
        return _NOT_JSON


def _option(item: services.Input) -> str:
    return "--" + item.command


def _link(value: Any) -> Optional[str]:
    """A url the document gives, such as its docs or pricing, if it is one."""
    return value if isinstance(value, str) and value.startswith("https://") else None


def _short(text: str, width: int = 80) -> str:
    """``text`` on one line, cut to ``width``: table cells never wrap."""
    line = _line(text)
    return line if len(line) <= width else line[: width - 1] + "…"


def _fields(fields: dict[str, Any]) -> dict[str, str]:
    """``fields`` on one line each, without the empty ones."""
    return {key: _line(value) for key, value in fields.items() if value}


def _line(value: Any) -> str:
    """``value`` as one line of text a terminal shows as it is: the service writes it,
    and a newline would pass for another row, a control character for a command."""
    return " ".join(_plain(str(value)).split())
