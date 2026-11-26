"""`btcli service`: the services published on chain, what they offer, calling them, and
balances at them.

An operation's inputs are flags named after them (``gpu_type`` is ``--gpu-type``)
and typed by the spec, so no request is ever written by hand. btcli's own options
come before the operation's name and the operation's inputs after it, as with
``docker run``. The ones a call is most often given (``--json``, ``-w``, ``-H``,
``--out``, ``--timeout``, ``--netuid``) are taken after it too, unless the operation
has an input of that name.
"""

from __future__ import annotations

import asyncio
import contextlib
import difflib
import json
import mimetypes
import re
import sys
import tempfile
import unicodedata
import urllib.parse
from pathlib import Path
from typing import Any, Awaitable, Callable, NoReturn, Optional, TypeVar

import typer

from ... import http_auth, services
from ...keyfiles import KeyfileError
from ...result import BittensorError
from ...settings import guide_docs_url
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
# Records of a list shown in full, field by field, when a table would cut them.
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
    published = app_ctx.run(lambda client: services.discover(client, netuid))
    problems = [_problem(item) for item in published]
    records = [
        {
            "netuid": item.netuid,
            "hotkey": item.hotkey,
            "publisher": _publisher(item.owner),
            "url": item.url,
            "usable": not problem,
            "name": item.service.name if item.service else None,
            "description": item.service.description if item.service else None,
            "operations": _auth_counts(item.service) if item.service else None,
            "problem": problem or None,
        }
        for item, problem in zip(published, problems)
    ]
    usable = [item for item, problem in zip(published, problems) if not problem]
    broken = [(item, problem) for item, problem in zip(published, problems) if problem]
    every = every or netuid is not None
    with _rendering(app_ctx):
        app_ctx.output.columns(
            "services",
            ["netuid", "service", "operations", "open", "about"],
            _clean(
                [
                    [
                        item.netuid,
                        _short(item.service.name, 32) + ("" if item.owner else " (validator)"),
                        len(item.service.operations),
                        _auth_counts(item.service)["open"] or "",
                        _first_sentence(item.service.description),
                    ]
                    for item in usable
                    if item.service
                ]
            ),
            _clean(records),
            right_align={0, 2, 3},
            footer=None if broken and every else _list_footer(len(broken)),
        )
        if broken and every and not app_ctx.output.json_mode:
            app_ctx.output.message("")
            app_ctx.output.columns(
                "cannot be used",
                ["netuid", "published at", "why"],
                _clean(
                    [
                        [item.netuid, _host(item.url), _why(item, problem)]
                        for item, problem in broken
                    ]
                ),
                right_align={0},
                footer="the publisher has to fix these; show one with "
                "`btcli service show --netuid N`",
            )


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


def _why(item: services.Published, problem: str) -> str:
    """A problem for the list's narrow column: no url, and no hotkey the row already names."""
    problem = re.sub(
        r"https?://\S+ is not an OpenAPI 3 spec \((.+)\)", r"its spec is \1, not OpenAPI 3", problem
    )
    problem = re.sub(r"https?://\S+", "its spec", problem)
    problem = problem.replace(f"netuid {item.netuid} hotkey {item.hotkey}", "the publishing hotkey")
    return _short(problem, 70)


def _host(url: str) -> str:
    return urllib.parse.urlsplit(url).netloc or _short(url, 40)


def _problem(item: services.Published) -> str:
    """Why a published service cannot be used, without its url, which the row shows."""
    if item.service is None:
        return item.problem.replace(f" at {item.url}", "")
    if not item.service.operations:
        return f"offers no operation ({len(item.service.skipped)} not offered; `show` says why)"
    return ""


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
        "publishers when they are given.",
    )


@app.command(
    "show",
    help="Show a service and the operations it offers.\n\n"
    "A `hotkey` operation is signed by your hotkey and paid from your balance at the "
    "service. An `optional` one is free unsigned and paid when signed. An `open` one "
    "needs no key. `btcli service call --netuid N OPERATION --help` describes one.\n\n"
    "With --json, each operation lists the names of its inputs; --schemas adds their "
    "JSON Schema.",
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
        operations = list(service.operations.values())
        tools = service.tools() if schemas else []
        _metadata(
            output,
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
                "operations": [
                    _definition(op, tools[index]) if schemas else _brief(op)
                    for index, op in enumerate(operations)
                ],
                "skipped": [
                    {"method": s.method, "path": s.path, "id": s.id, "reason": s.reason}
                    for s in service.skipped
                ],
            },
        )
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
        "required": [item.name for item in op.inputs if item.required],
        "optional": [item.name for item in op.inputs if not item.required],
    }


def _definition(op: services.Operation, tool: dict) -> dict:
    """An operation in full: what it is, and the JSON Schema of its inputs."""
    return {
        "operation": op.command,
        "id": op.id,
        "auth": op.auth,
        "method": op.method,
        "path": op.path,
        "server": op.api_url,
        "tags": list(op.tags),
        "summary": op.summary,
        "description": op.description,
        "input_schema": tool["input_schema"],
    }


@app.command(
    "call",
    help="Call an operation of a service.\n\n"
    "btcli service call --netuid 19 json-rpc-request --method system_health\n\n"
    "btcli --json service call --netuid 19 json-rpc-request --inputs "
    '\'{"method": "system_health"}\'\n\n'
    "Inputs follow the operation as flags (`max_tokens` is `--max-tokens`), or all at "
    "once as a JSON object with --inputs. `OPERATION --help` lists them, and without "
    "OPERATION the service's operations are listed. A value starting with @ is read from "
    "that file (@- from stdin, @@ starts a value with @).\n\n"
    "A `hotkey` operation is signed by the wallet's hotkey and paid from its balance at "
    "the service. An `optional` one is signed when the wallet has the hotkey, unless "
    "--unsigned.\n\n"
    "With --json the answer is `{status, content_type, signed, body}` (`text` when the "
    "body is not JSON, `out` when it went to a file), and an error is JSON on stderr with "
    "a `code`. It exits 2 when the call does not fit the operation, and 1 for any other "
    "failure, an HTTP error among them.",
    context_settings={
        "allow_interspersed_args": False,
        "allow_extra_args": True,
        "ignore_unknown_options": True,
    },
)
@with_globals
def call(
    ctx: typer.Context,
    operation: Optional[str] = typer.Argument(None, metavar="[OPERATION [INPUTS]...]"),
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
    unsigned: bool = typer.Option(
        False, "--unsigned", help="Use the free tier: do not sign an `optional` operation."
    ),
    timeout: float = typer.Option(
        services.TIMEOUT, "--timeout", help="Seconds the call may take.", min=0.1
    ),
    url: Optional[str] = _url_option(),
):
    app_ctx: AppContext = ctx_of(ctx)
    args = list(ctx.args)
    if netuid is None and url is None:
        netuid = _netuid_after(args)
    service = _service(app_ctx, netuid, service_hotkey, url)
    if operation is None:
        with _rendering(app_ctx):
            _show(app_ctx, service)
        app_ctx.output.message(
            f"call one with `btcli service call --netuid {service.netuid} OPERATION`; "
            "`OPERATION --help` lists its inputs"
        )
        return
    try:
        op = service.operation(operation)
    except services.ServiceError as error:
        _fail(
            app_ctx,
            error,
            f"list its operations with `btcli service show --netuid {service.netuid}`",
        )
    own: dict[str, Any] = {"inputs": inputs, "out": out, "unsigned": unsigned, "timeout": timeout}
    values = _inputs(app_ctx, service, op, args, own)
    if own.get("wallet") is not None:
        app_ctx.wallet_name, app_ctx.wallet_given = own["wallet"], True
    if own.get("hotkey") is not None:
        app_ctx.hotkey_name, app_ctx.hotkey_given = own["hotkey"], True
    if own.get("wallet_path") is not None:
        app_ctx.wallet_path = own["wallet_path"]
    if values is None:
        with _rendering(app_ctx):
            _help(app_ctx, service, op)
        return
    out = own["out"]
    if out is not None and str(out) != "-" and (out.is_dir() or not out.parent.is_dir()):
        _input_error(app_ctx, service, op, f"cannot write to {out}: name a file in a directory")
    if op.auth == "hotkey" and own["unsigned"]:
        app_ctx.output.error(
            f"{op.command} is always signed; it cannot be sent unsigned",
            help="leave out --unsigned and name your wallet and hotkey with -w and -H",
            data={"code": "input"},
        )
        raise typer.Exit(2)
    key = None
    if op.auth == "hotkey":
        key = _hotkey(app_ctx, required=True)
    elif op.auth == "optional" and not own["unsigned"]:
        # A wallet or hotkey named on purpose is meant to sign; only the default may be absent.
        key = _hotkey(app_ctx, required=app_ctx.wallet_given or app_ctx.hotkey_given)

    async def _signed(client) -> services.Response:
        assert key is not None
        signer = await http_auth.prepare_signer(key, client=client)
        return await service.call(op.id, values, signer=signer, timeout=own["timeout"])

    def slow(error: services.ServiceError) -> Optional[str]:
        return "give it longer with --timeout SECONDS" if error.code == "timeout" else None

    if key is not None:
        response = _attempt(app_ctx, _signed, slow)
    else:  # nothing to sign, so nothing to ask the chain
        response = _offline(
            app_ctx,
            service.call(op.id, values, timeout=own["timeout"]),
            f"calling {_short(service.name, 40)}…",
            slow,
        )
    if op.auth == "optional":
        app_ctx.output.message(
            f"signed by hotkey {key.ss58_address}; --unsigned sends it unsigned"
            if key
            else "sent unsigned"
            + (
                ""
                if own["unsigned"]
                else f": wallet {app_ctx.wallet_name!r} has no hotkey to sign with"
            )
        )
    with _rendering(app_ctx):
        _show_response(
            app_ctx, service, op, response, out, signed=key.ss58_address if key else None
        )


def _netuid_after(args: list[str]) -> Optional[int]:
    """``--netuid N`` given after the operation, taken out of its inputs."""
    for index, token in enumerate(args):
        flag, has_value, value = token.partition("=")
        if flag != "--netuid":
            continue
        value = value if has_value else args[index + 1] if index + 1 < len(args) else ""
        if value.isdigit():
            del args[index : index + (1 if has_value else 2)]
            return int(value)
    return None


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
    service = _service(app_ctx, netuid, service_hotkey)
    key = None if hotkey_ss58 else _hotkey(app_ctx, required=True, or_else="--hotkey ADDRESS")

    async def _signed(client) -> dict:
        assert key is not None
        signer = await http_auth.prepare_signer(key, client=client)
        return await service.balance(signer=signer)

    def explain(error: services.ServiceError) -> Optional[str]:
        if error.status == 402:
            return _pay_help(app_ctx, service)
        if error.status in (401, 403) and hotkey_ss58:
            return (
                "the service shows a balance only to its hotkey; "
                "leave out --hotkey to read your own, signed"
            )
        return None

    if key is not None:
        record = _attempt(app_ctx, _signed, explain)
    else:
        record = _offline(
            app_ctx,
            service.balance(hotkey_ss58),
            f"reading {_short(service.name, 40)}…",
            explain,
        )
    with _rendering(app_ctx):
        if app_ctx.output.json_mode:
            _json(app_ctx.output, record)
        else:
            app_ctx.output.detail(
                f"balance at {_line(service.name)} on netuid {netuid}", _flat(record)
            )


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
    return _attempt(app_ctx, lambda client: services.load(client, netuid, hotkey))


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


def _hotkey(app_ctx: AppContext, *, required: bool, or_else: str = ""):
    """The wallet's hotkey, loaded before the chain is reached, so a missing or bad
    key file is reported as one; for an optional signature, None without one."""
    try:
        wallet = app_ctx.signer("hotkey")
        if not required and not app_ctx.wallet().hotkey_file.exists_on_device():
            return None
        signer = http_auth.resolve_signer(wallet, role="hotkey")
        return signer if signer.ss58_address else None  # reading it reads the key file
    except (OSError, ValueError, KeyfileError, BittensorError) as error:
        if not required:
            return None
        app_ctx.output.error(
            f"cannot use hotkey {app_ctx.hotkey_name!r} of wallet {app_ctx.wallet_name!r}: {error}",
            help="name the wallet and hotkey with -w and -H"
            + (f", or use {or_else}" if or_else else ""),
            data={"code": "wallet"},
        )
        raise typer.Exit(1) from None


# Inputs: flags in, typed values out.


# btcli's options that may also follow the operation when it has no input of the name.
_OWN = {
    "--json": None,
    "--unsigned": None,
    "--inputs": "inputs",
    "--out": "out",
    "--timeout": "timeout",
    "--wallet": "wallet",
    "-w": "wallet",
    "--wallet-hotkey": "hotkey",
    "-H": "hotkey",
    "--wallet-path": "wallet_path",
}


def _inputs(
    app_ctx: AppContext,
    service: services.Service,
    op: services.Operation,
    args: list[str],
    own: dict[str, Any],
) -> Optional[dict[str, Any]]:
    """The inputs that ``--inputs`` and the flags give, or None for ``--help``; btcli's
    own options among the flags go to ``own``."""
    by_option = {_option(item): item for item in op.inputs}
    for item in op.inputs:  # the spec's own spelling works too: --gpuType, --gpu_type
        by_option.setdefault("--" + item.name, item)
    values: dict[str, Any] = {}
    wants_help = False
    rest = iter(args)
    for token in rest:
        flag, has_value, inline = token.partition("=")
        item = by_option.get(flag)
        negated = False
        if item is None and flag.startswith("--no-"):
            item = by_option.get("--" + flag[len("--no-") :])
            negated = item is not None
        if item is None and token in ("--help", "-h"):
            wants_help = True
            continue
        if item is None and token == "--":
            continue
        if item is None and flag in _OWN:
            name = _OWN[flag]
            if name is None and flag == "--json":
                app_ctx.output.json_mode = True
            elif name is None:
                own["unsigned"] = True
            else:
                own[name] = inline if has_value else _next(app_ctx, service, op, rest, flag)
            continue
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
        if types == {"array"}:
            values.setdefault(item.name, []).extend(value)
        elif item.name in values:
            _input_error(app_ctx, service, op, f"{flag} is given twice")
        else:
            values[item.name] = value
    if wants_help:
        return None
    try:
        own["timeout"] = float(own["timeout"])
    except ValueError:
        _input_error(app_ctx, service, op, "--timeout takes seconds")
    if own["out"] is not None:
        own["out"] = Path(own["out"])
    values = {**_given(app_ctx, service, op, own["inputs"], by_option), **values}
    for item in op.inputs:
        schema = service.resolve(item.schema)
        if item.required and item.name not in values and isinstance(schema, dict):
            # A fixed or defaulted value need not be typed: `--jsonrpc 2.0` every time.
            for key in ("const", "default"):
                if key in schema:
                    values[item.name] = schema[key]
                    break
    missing = [_option(item) for item in op.inputs if item.required and item.name not in values]
    if missing:
        _input_error(app_ctx, service, op, f"{op.command} needs {', '.join(missing)}")
    return values


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
        item = by_option.get("--" + str(key))
        if item is None:
            close = difflib.get_close_matches(str(key), [i.name for i in op.inputs], n=2)
            _input_error(
                app_ctx,
                service,
                op,
                f"{op.command} has no input {_short(str(key), 60)!r}"
                + (f"; did you mean {', '.join(close)}?" if close else ""),
            )
        values[item.name] = value
    return values


def _read(app_ctx, service, op, text: str) -> str:
    """A value as written, or read from the file an @ names (@- is stdin, @@ an @)."""
    if text.startswith("@@"):
        return text[1:]
    if text == "@-":
        return sys.stdin.read().rstrip("\n")
    if text.startswith("@"):
        try:
            return Path(text[1:]).expanduser().read_text(encoding="utf-8").rstrip("\n")
        except (OSError, UnicodeDecodeError) as error:
            _input_error(app_ctx, service, op, f"cannot read {text}: {error}")
    return text


def _value(app_ctx, service, op, item: services.Input, schema: Any, text: str) -> Any:
    """A flag's value: text for a text input, JSON for any other type (text when it is
    untyped and not JSON), @FILE from a file. An array input takes one item, or a JSON
    array of them."""
    text = _read(app_ctx, service, op, text)
    if text == "null" and _nullable(schema) and "string" not in _types(schema):
        return None
    if _types(schema) != {"array"}:
        return _typed(app_ctx, service, op, item, schema, _parse(text, schema), text)
    items = service.resolve(schema.get("items") or {})
    whole = _loads(text)
    values = whole if isinstance(whole, list) else [_parse(text, items)]
    return [_typed(app_ctx, service, op, item, items, value, text) for value in values]


def _nullable(schema: Any) -> bool:
    if not isinstance(schema, dict):
        return False
    kind = schema.get("type")
    return (
        schema.get("nullable") is True
        or kind == "null"
        or (isinstance(kind, list) and "null" in kind)
        or any(_nullable(sub) for key in ("anyOf", "oneOf") for sub in schema.get(key) or [])
    )


_NOT_JSON = object()


def _parse(text: str, schema: Any) -> Any:
    types = _types(schema)
    if "string" in types:
        return text
    value = _loads(text)
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


def _typed(app_ctx, service, op, item: services.Input, schema: Any, value: Any, text: str):
    types = _types(schema)
    kind = _json_type(value)
    if types and not (kind in types or (kind == "integer" and "number" in types)):
        _input_error(
            app_ctx,
            service,
            op,
            f"{_option(item)} takes {_placeholder(schema)}, not {_short(text, 60)!r}",
        )
    enum = _enum(schema)
    if enum and value not in enum:
        _input_error(app_ctx, service, op, f"{_option(item)} takes {_placeholder(schema)}")
    return value


def _input_error(app_ctx, service, op, text: str) -> NoReturn:
    app_ctx.output.error(
        _line(text),
        help=f"`btcli service call --netuid {service.netuid} {op.command} --help` lists its "
        "inputs; btcli options like --network go before the operation",
        data={"code": "input"},
    )
    raise typer.Exit(2)


def _types(schema: Any) -> set[str]:
    """The JSON types a schema allows, without null."""
    if not isinstance(schema, dict):
        return set()
    kind = schema.get("type")
    types = {kind} if isinstance(kind, str) else {t for t in kind or [] if isinstance(t, str)}
    for key in ("anyOf", "oneOf", "allOf"):
        for sub in schema.get(key) or []:
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
        for sub in schema.get(key) or []:
            values.extend(_enum(sub))
    return [v for v in values if v is not None]


def _placeholder(schema: Any) -> str:
    """How an input's value is written: its choices, TEXT, or JSON of its type."""
    types = _types(schema)
    if types == {"array"} and isinstance(schema, dict):
        return _placeholder(schema.get("items") or {}) + "…"
    if _enum(schema):
        return "|".join(_line(_cell(v)) for v in _enum(schema))
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
    plain = _line(re.sub(r"[*_`#>]+|\[([^\]]*)\]\([^)]*\)", r"\1", description))
    sentence = re.split(r"(?<=[.!?])\s", plain, maxsplit=1)[0].rstrip(".")
    return _line(tag) + (f": {sentence}" if sentence else "")


_AUTH = {
    "hotkey": "signed by your hotkey and paid from its balance at the service",
    "optional": "free unsigned (--unsigned), paid when signed by your wallet's hotkey",
    "open": "open to anyone",
}


def _help(app_ctx: AppContext, service: services.Service, op: services.Operation) -> None:
    output = app_ctx.output
    if output.json_mode:
        _metadata(output, _definition(op, service.tool(op.id)))
        return
    output.detail(
        _line(op.command),
        _fields(
            {
                "summary": op.summary,
                "description": op.description,
                "auth": _AUTH[op.auth],
                "usage": f"btcli service call --netuid {service.netuid} {op.command} [INPUTS]",
                "or": f"btcli service call --netuid {service.netuid} {op.command} "
                "--inputs '{\"NAME\": VALUE, ...}'",
            }
        ),
    )
    if not op.inputs:
        return
    typer.echo()
    rows = []
    for item in op.inputs:
        schema = service.resolve(item.schema)
        schema = schema if isinstance(schema, dict) else {}
        default = schema.get("const", schema.get("default"))
        flag = _option(item)
        if _types(schema) == {"boolean"}:
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


def _show_response(
    app_ctx: AppContext,
    service: services.Service,
    op: services.Operation,
    response: services.Response,
    out: Optional[Path],
    *,
    signed: Optional[str],
) -> None:
    """Show a response; under --json always as ``{status, content_type, signed, ...}``
    with its ``body`` (JSON), ``text``, or the file it went ``out`` to."""
    output = app_ctx.output
    about = {
        "status": response.status,
        "content_type": response.content_type,
        "signed": signed,  # the hotkey that signed it, so paid for it, if any
    }
    if not response.ok:
        _http_error(app_ctx, service, response, about, open_to_all=op.auth == "open")
    if out is not None and str(out) == "-":
        sys.stdout.buffer.write(response.content)
        sys.stdout.flush()
        return
    parsed = _parsed(response)
    if out is None and parsed is _NOT_JSON and response.content and not _is_text(response):
        # Never dropped: the call may have been paid for.
        suffix = mimetypes.guess_extension(response.content_type) or ".bin"
        out = Path(tempfile.mkstemp(prefix="btcli-service-", suffix=suffix)[1])
    if out is not None:
        out.write_bytes(response.content)
        if output.json_mode:
            _json(output, {**about, "bytes": len(response.content), "out": str(out)})
        else:
            kind = _line(response.content_type)
            output.message(
                f"wrote {len(response.content)} bytes{f' of {kind}' if kind else ''} to {out}"
            )
    elif parsed is not _NOT_JSON:
        if output.json_mode:
            _json(output, {**about, "body": parsed})
        else:
            _render(output, op, _clean(parsed))
    elif not response.content:
        if output.json_mode:
            _json(output, about)
        else:
            output.message(f"done (HTTP {response.status})")
    elif output.json_mode:
        _json(output, {**about, "text": response.text})
    else:  # `--out -` writes it as it is
        typer.echo(_clean(response.text))


def _is_text(response: services.Response) -> bool:
    kind = response.content_type
    if kind:
        return kind.startswith("text/") or kind.endswith(("json", "xml", "javascript"))
    try:
        response.content.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return True


def _render(output, op: services.Operation, value: Any) -> None:
    """A JSON response by its shape: an object as its fields, nested ones as dotted
    names, and its lists of records as tables; a list of records as a table."""
    if isinstance(value, dict):
        fields: dict[str, Any] = {}
        tables: dict[str, list] = {}
        try:
            _flatten(value, "", fields, tables)
        except _Clash:
            typer.echo(json.dumps(value, indent=2, ensure_ascii=False))
            return
        if fields:
            output.detail(None, fields)
        for index, (key, rows) in enumerate(tables.items()):
            if fields or index:
                typer.echo()
            _table(output, op, key, rows)
        if not fields and not tables:
            typer.echo("{}")
    elif _is_records(value):
        _table(output, op, _line(op.summary) or op.command, value)
    elif isinstance(value, list):
        for item in value:
            typer.echo(item if isinstance(item, str) else _cell(item))
    else:
        typer.echo(value if isinstance(value, str) else _cell(value))


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
        else:
            fields[name] = _line(_cell(item)) if not isinstance(item, dict) else "{}"


def _table(output, op: services.Operation, title: str, rows: list[dict]) -> None:
    keys: list[str] = []
    for row in rows:
        keys.extend(k for k, v in row.items() if k not in keys and not isinstance(v, (dict, list)))
    nested = {k for row in rows for k, v in row.items() if isinstance(v, (dict, list))}
    if len(rows) <= MAX_BLOCKS and (len(keys) > MAX_COLUMNS or nested):
        # A few wide records, as a create or a rent answers: every field of each.
        for index, row in enumerate(rows):
            if index:
                typer.echo()
            _render(output, op, row)
        return
    if "id" in keys:  # the field other operations take, so never cut
        keys.insert(0, keys.pop(keys.index("id")))
    shown = keys[:MAX_COLUMNS]
    output.columns(
        title,
        [_line(k) for k in shown],
        [[_short(_cell(row.get(k)), 60) for k in shown] for row in rows],
    )
    _more(output, len(keys) - len(shown) + len(nested - set(keys)), "field")


def _more(output, count: int, what: str) -> None:
    if count > 0:
        plural = "" if count == 1 else "s"
        output.message(f"{count} more {what}{plural}; `--json` or `--out FILE` gets them all")


def _cell(value: Any) -> str:
    if value is None:
        return ""
    return value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)


def _is_records(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(row, dict) for row in value)


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
    elif response.status in (401, 403):
        help = (
            f"check your hotkey's balance with `btcli service balance --netuid {service.netuid}"
            f"{_wallet_flags(app_ctx)}`"
        )
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


def _pay_command(app_ctx: AppContext, service: services.Service) -> str:
    return (
        f"btcli gamma spend --netuid {service.netuid} --service-hotkey {service.hotkey} "
        f"--amount-gamma AMOUNT{_wallet_flags(app_ctx)}"
    )


def _pay_help(app_ctx: AppContext, service: services.Service) -> str:
    return f"pay the service with `{_pay_command(app_ctx, service)}`"


def _wallet_flags(app_ctx: AppContext) -> str:
    """The wallet flags a command needs to use this wallet; none for btcli's defaults."""
    flags = ""
    if app_ctx.wallet_name and app_ctx.wallet_given:
        flags += f" -w {app_ctx.wallet_name}"
    if app_ctx.hotkey_name and app_ctx.hotkey_given:
        flags += f" -H {app_ctx.hotkey_name}"
    return flags


@contextlib.contextmanager
def _rendering(app_ctx: AppContext):
    """Report what the service sent that cannot be shown, instead of a traceback."""
    try:
        yield
    except (services.ServiceError, ValueError, RecursionError, OSError) as error:
        app_ctx.output.error(_line(str(error)))
        raise typer.Exit(1) from None


def _parsed(response: services.Response) -> Any:
    if not response.is_json:
        return _NOT_JSON
    try:
        return response.json()
    except (ValueError, RecursionError):
        return _NOT_JSON


def _json(output, record: Any) -> None:
    output.value(_clean(record) if sys.stdout.isatty() else record)


def _metadata(output, record: Any) -> None:
    """What a service says about itself, cleaned for agents too, piped or not."""
    output.value(_clean(record))


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


def _flat(record: dict) -> dict:
    """A record's fields by dotted name, each on one line."""
    fields: dict[str, Any] = {}
    try:
        _flatten(_clean(record), "", fields, {})
    except _Clash:
        return {"record": _line(json.dumps(record, ensure_ascii=False))}
    return fields


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
