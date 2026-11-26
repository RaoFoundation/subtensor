"""`btcli service`: the services published on chain, what they offer, calling them, and
balances at them.

An operation's inputs are flags named after them (``gpu_type`` is ``--gpu-type``)
and typed by the spec, so no request is ever written by hand. btcli's own options
can precede the operation. Common call controls also work after it; use
``--inputs`` for a provider input sharing a control's name.
"""

from __future__ import annotations

import asyncio
import contextlib
import difflib
import json
import mimetypes
import os
import re
import shlex
import sys
import tempfile
import threading
import unicodedata
import urllib.parse
from pathlib import Path
from typing import IO, Any, Awaitable, Callable, NoReturn, Optional, TypeVar

import typer
from rich.markup import escape

from ... import config as cfg
from ... import http_auth, services, wallets
from ...keyfiles import KeyfileError
from ...result import BittensorError
from ...settings import DEFAULT_NETWORK, guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals

app = typer.Typer(
    no_args_is_help=True,
    help="Subnet services: finding them, what they offer, calling them, and your balance "
    "at them.\n\n"
    f"Guide: {guide_docs_url('services')}",
)

# Columns shown for a list of records; --json shows every field.
MAX_COLUMNS = 8
# Rows in a human preview. JSON and file output always preserve all rows.
MAX_ROWS = 50
# Records of a list shown whole, each with every field, rather than as a table.
MAX_BLOCKS = 3
# Characters of an error response shown; a validation error lists every field.
MAX_ERROR = 2000
# Operations listed as not offered; --json lists them all.
MAX_SKIPPED = 20

T = TypeVar("T")


def _netuid_option():
    return typer.Option(..., "--netuid", help="Subnet of the service.")


def _service_option():
    return typer.Option(
        None,
        address_cli_name("service_hotkey_ss58"),
        help="Hotkey that published the service, when the subnet has more than one. "
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
    records = app_ctx.run(
        lambda client: services.discover(
            client, netuid, cache=cfg.service_documents_path(), select=_list_record
        )
    )
    if app_ctx.output.json_mode:
        app_ctx.output.value(_clean(records))
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
    if service and not service.operations:
        problem = f"offers no operation ({len(service.skipped)} not offered; `show` says why)"
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
        "publishers when they are given. Only a url on this machine can be signed for.",
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
        record = _clean(
            {
                "netuid": service.netuid,
                "hotkey": service.hotkey,
                "publisher": _publisher(service.owner),
                "name": service.name,
                "description": service.description,
                "document": service.url,
                "api": service.api_url,
                "spec": service.spec_url,
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
            record["operations"] = [_clean(_brief(op)) for op in operations]
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
                "spec": service.spec_url,
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
    return _clean(
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
    "signed only when you name a wallet or hotkey (-w, -H), and --unsigned leaves it "
    "unsigned even then.\n\n"
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
        None, "--body", help="The complete JSON body (@FILE or @-)."
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
    values = _inputs(app_ctx, service, op, ctx.args, inputs)
    body_kw = {}
    if body is not None:
        raw = _loads(_read(app_ctx, service, op, body))
        if raw is _NOT_JSON:
            _input_error(app_ctx, service, op, "--body takes a JSON value")
        body_kw["body"] = raw
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
    if signing and not service.signable:
        _input_error(
            app_ctx,
            service,
            op,
            f"{service.url} was read by its url, so nothing ties hotkey {service.hotkey} to it "
            f"and {op.command} cannot be signed for it"
            + ("" if op.auth == "hotkey" else "; --unsigned sends it unsigned"),
        )
    if app_ctx.dry_run or dry_run:
        method, url, _, _, data = prepared
        preview = {
            "dry_run": True,
            "operation": op.command,
            "method": method,
            "url": url,
            "body": json.loads(data) if data else None,
            "auth": op.auth,
            "netuid": service.netuid,
            "service_hotkey": service.hotkey,
        }
        if app_ctx.output.json_mode:
            app_ctx.output.value(_clean(preview))
        else:
            app_ctx.output.detail("dry run", _clean(preview))
        return
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

    def slow(error: services.ServiceError) -> Optional[str]:
        return (
            "check the outcome before retrying; --timeout SECONDS allows a longer wait"
            if error.code == "timeout"
            else None
        )

    response = _signing(
        app_ctx,
        key,
        lambda signer: service.call(op.id, values, signer=signer, timeout=timeout, **body_kw),
        f"calling {_short(service.name, 40)}…",
        slow,
    )
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
    netuid: int = _netuid_option(),
    hotkey: Optional[str] = typer.Option(
        None, "--hotkey", help="Read this hotkey's balance (an address), not your wallet's."
    ),
    service_hotkey: Optional[str] = _service_option(),
):
    app_ctx: AppContext = ctx_of(ctx)
    hotkey_ss58 = app_ctx.resolve_address("hotkey_ss58", hotkey) if hotkey else None
    publisher = (
        app_ctx.resolve_address("service_hotkey_ss58", service_hotkey) if service_hotkey else None
    )
    service = _attempt(
        app_ctx,
        lambda client: services.load_document(
            client, netuid, publisher, cache=cfg.service_documents_path()
        ),
    )
    key = None if hotkey_ss58 else _hotkey(app_ctx, or_else="--hotkey ADDRESS")

    def explain(error: services.ServiceError) -> Optional[str]:
        if error.status == 402:
            return _pay_help(app_ctx, service)
        if error.status in (401, 403) and hotkey_ss58:
            return (
                "the service shows a balance only to its hotkey; "
                "leave out --hotkey to read your own, signed"
            )
        return None

    record = _signing(
        app_ctx,
        key,
        lambda signer: service.balance(hotkey_ss58, signer=signer),
        f"reading {_short(service.name, 40)}…",
        explain,
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
    hotkey = (
        app_ctx.resolve_address("service_hotkey_ss58", service_hotkey) if service_hotkey else None
    )
    if url is not None:
        return _offline(
            app_ctx, services.load_url(url, netuid, hotkey), f"reading {_short(url, 60)}…"
        )
    if netuid is None:
        app_ctx.output.error(
            "missing required option: `--netuid`",
            help="name the subnet, as in `btcli service show --netuid 19`; "
            "`btcli service list` shows the services",
            data={"code": "input"},
        )
        raise typer.Exit(2)
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

    async def _plain() -> T:
        return await work(None if key is None else await http_auth.prepare_signer(key, client=None))

    return _offline(app_ctx, _plain(), label, explain)


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
    file is reported as one."""
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


def _usage(app_ctx, text: str) -> NoReturn:
    app_ctx.output.error(text, data={"code": "input"})
    raise typer.Exit(2)


def _inputs(app_ctx, service, op, args, inputs) -> dict[str, Any]:
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
        schema = service.resolve(item.schema)
        types = _types(schema)
        if types == {"boolean"} and not has_value:
            value: Any = not negated
        else:
            if negated:
                _input_error(app_ctx, service, op, f"{flag} takes no value")
            if not has_value:
                inline = _next(app_ctx, service, op, rest, flag)
            value = _value(app_ctx, service, op, item, schema, inline)
        if types == {"array"} and value is not None and values.get(item.name, []) is not None:
            values.setdefault(item.name, []).extend(value)
        elif item.name in values:
            _input_error(app_ctx, service, op, f"{flag} is given twice")
        else:
            values[item.name] = value
    return {**_given(app_ctx, service, op, inputs, by_option), **values}


def _next(app_ctx, service, op, rest, flag: str) -> str:
    value = next(rest, None)
    if value is None:
        _input_error(app_ctx, service, op, f"{flag} needs a value")
    return value


def _given(app_ctx, service, op, text: Optional[str], by_option: dict) -> dict[str, Any]:
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
        values[name] = value
    return values


def _read(app_ctx, service, op, text: str) -> str:
    """JSON as written, or read from the file an @ names (@- is stdin). Only --inputs
    and --body read files: a JSON value never starts with @, while a flag's text can,
    and an agent a spec steers must not send a local file by filling in an input."""
    if text.startswith("@"):
        try:
            if text == "@-":
                return sys.stdin.buffer.read().decode("utf-8")
            return Path(text[1:]).expanduser().read_bytes().decode("utf-8")
        except (OSError, UnicodeDecodeError) as error:
            _input_error(app_ctx, service, op, f"cannot read {text}: {error}")
    return text


def _value(app_ctx, service, op, item: services.Input, schema: Any, text: str) -> Any:
    """A flag's value: text for a text input, JSON for any other type (text when it is
    untyped and not JSON). An array input takes one item, or a JSON array of them."""
    if text == "null" and _nullable(schema) and "string" not in _types(schema):
        return None
    if _types(schema) != {"array"}:
        return _typed(app_ctx, service, op, item, schema, _parse(text, schema))
    items = service.resolve(schema.get("items") or {})
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


def _not_json(name: str) -> Any:
    raise ValueError(f"{name} is not JSON")


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
    app_ctx.output.error(
        _line(text),
        help=f"`{command}` lists its inputs",
        data={"code": "input"},
    )
    raise typer.Exit(2)


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
    output.detail(
        _line(op.command),
        _fields(
            {
                "summary": op.summary,
                "description": op.description,
                "auth": _AUTH[op.auth],
                "usage": _service_command(app_ctx, service, "call", op.command) + " [INPUTS]",
                "or": _service_command(app_ctx, service, "call", op.command)
                + " --inputs '{\"NAME\": VALUE, ...}'",
            }
        ),
    )
    if not op.inputs:
        return
    typer.echo()
    reserved = {flag for param in ctx.command.params for flag in param.opts}
    rows = []
    for item in op.inputs:
        schema = service.resolve(item.schema)
        schema = schema if isinstance(schema, dict) else {}
        default = schema.get("const", schema.get("default"))
        flag = _option(item)
        if flag in reserved:
            flag = f"--inputs ({item.name})"
        elif _types(schema) == {"boolean"}:
            flag = f"{_option(item)} / --no-{_option(item)[2:]}"
        rows.append(
            [
                _line(flag),
                "" if _types(schema) == {"boolean"} else _placeholder(schema),
                "yes" if item.required and default is None else "",
                "" if default is None else _line(_cell(default)),
                _short(item.description or _line(_cell(schema.get("example") or ""))),
            ]
        )
    output.columns("inputs", ["input", "value", "required", "default", "description"], rows)


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
) -> None:
    """Show a response; under --json always as ``{status, content_type, signed, ...}``
    with its ``body`` (JSON), ``text``, or the file it went ``out`` to."""
    output = app_ctx.output
    about = {
        "status": response.status,
        "content_type": response.content_type,
        "signed": signed,
    }
    if not response.ok:
        _http_error(app_ctx, service, response, about, open_to_all=op.auth == "open")
    if out is not None and str(out) == "-":
        try:
            sys.stdout.buffer.write(response.content)
            sys.stdout.flush()
        except OSError as error:
            _recover_response(output, response, "stdout", error)
        return
    parsed = _parsed(response) if out is None else _NOT_JSON
    if staged is not None and out is not None and cleanup is not None:
        path = Path(staged.name)
        try:
            with staged:
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
    elif out is None and parsed is _NOT_JSON and response.content and not _is_text(response):
        out = _save_response(response)
    if out is not None:
        if output.json_mode:
            output.value({**about, "bytes": len(response.content), "out": str(out)})
        else:
            kind = escape(_line(response.content_type))
            output.message(
                f"wrote {len(response.content)} bytes{f' of {kind}' if kind else ''} "
                f"to {escape(str(out))}"
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
        typer.echo(_clean(response.text))


def _save_response(response: services.Response) -> Path:
    suffix = mimetypes.guess_extension(response.content_type) or ".bin"
    with tempfile.NamedTemporaryFile(prefix="btcli-service-", suffix=suffix, delete=False) as file:
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


def _render(output, title: Optional[str], value: Any) -> None:
    """A JSON response by its shape: an object as its fields, nested ones by dotted name
    and text of several lines as a block, and a list of records as a table."""
    if isinstance(value, dict):
        fields: dict[str, Any] = {}
        tables: dict[str, list] = {}
        try:
            _flatten(value, "", fields, tables)
        except _Clash:
            typer.echo(json.dumps(value, indent=2))
            return
        texts = {k: v for k, v in fields.items() if isinstance(v, _Lines)}
        fields = {k: v for k, v in fields.items() if k not in texts}
        if fields:
            output.detail(title, fields)
        for index, (key, text) in enumerate(texts.items()):
            if fields or index:
                typer.echo()
            output.text_block(key, text)
        for index, (key, rows) in enumerate(tables.items()):
            if fields or texts or index:
                typer.echo()
            _table(output, key, rows)
        if not fields and not texts and not tables:
            typer.echo("{}")
    elif _is_records(value):
        _table(output, title or "", value)
    elif isinstance(value, list):
        for item in value[:MAX_ROWS]:
            typer.echo(_line(_cell(item)))
        _more(output, len(value) - MAX_ROWS, "row")
    else:
        typer.echo(_clean(value) if isinstance(value, str) else _cell(value))


class _Clash(Exception):
    """Two fields of a response flatten to one name."""


def _flatten(value: dict, prefix: str, fields: dict, tables: dict) -> None:
    """``value``'s scalars and lists of scalars into ``fields`` and its lists of records
    into ``tables``, by dotted name."""
    for key, item in value.items():
        name = f"{prefix}{_line(str(key))}"
        if name in fields or name in tables or not name:
            raise _Clash(name)
        if isinstance(item, dict) and item:
            _flatten(item, name + ".", fields, tables)
        elif _is_records(item) and item:
            tables[name] = item
        elif isinstance(item, list):
            fields[name] = ", ".join(_line(_cell(v)) for v in item) or "[]"
        elif item is None:
            if not prefix:  # a top-level null is an answer: a JSON-RPC result, say
                fields[name] = "null"
        elif isinstance(item, str) and "\n" in item.strip():
            fields[name] = _Lines(_clean(item).strip())
        else:
            fields[name] = _line(_cell(item)) if not isinstance(item, dict) else "{}"


class _Lines(str):
    """A field's text of several lines, as an answer a model wrote, shown as a block."""


def _table(output, title: str, rows: list[dict]) -> None:
    nested = any(isinstance(v, (dict, list)) and v for row in rows for v in row.values())
    flat = [_flat_record(row, "", {}) for row in rows]
    keys = list(dict.fromkeys(k for row in flat for k in row))
    if len(rows) <= MAX_BLOCKS and (len(keys) > MAX_COLUMNS or nested):
        # A few wide records, as a create or a rent answers: every field of each.
        for index, row in enumerate(rows):
            if index:
                typer.echo()
            _render(output, None, row)
        return
    if "id" in keys:  # the field other operations take, so never cut
        keys.insert(0, keys.pop(keys.index("id")))
    shown = keys[:MAX_COLUMNS]
    output.columns(
        title,
        shown,
        [[_short(_cell(row.get(k)), 60) for k in shown] for row in flat[:MAX_ROWS]],
    )
    _more(output, len(keys) - len(shown), "field")
    _more(output, len(rows) - MAX_ROWS, "row")


def _flat_record(row: dict, prefix: str, out: dict) -> dict:
    """A table row's fields by dotted name, its nested objects' too; a list stays one cell."""
    for key, value in row.items():
        name = f"{prefix}{_line(str(key))}"
        if isinstance(value, dict) and value:
            _flat_record(value, name + ".", out)
        else:
            out.setdefault(name, value)
    return out


def _more(output, count: int, what: str) -> None:
    if count > 0:
        plural = "" if count == 1 else "s"
        output.message(f"{count} more {what}{plural}; `--json` or `--out FILE` gets them all")


def _is_records(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(row, dict) for row in value)


def _cell(value: Any) -> str:
    if value is None:
        return ""
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
    elif response.status == 402:
        help = _pay_help(app_ctx, service)
    elif response.status in (401, 403) and not about["signed"]:
        help = "sign it with your hotkey: name it with -w and -H, without --unsigned"
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
    return " ".join(_clean(str(value)).split())


def _clean(value: Any) -> Any:
    """``value`` with control and format characters removed from every string:
    the service writes them, and a terminal would act on them."""
    if isinstance(value, str):
        return "".join(ch for ch in value if ch in "\n\t" or unicodedata.category(ch)[0] != "C")
    if isinstance(value, dict):
        return {_clean(k): _clean(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_clean(v) for v in value]
    return value
