"""The `btcli query` command group — generated from the reads registry.

Every registered read becomes a subcommand whose options are derived from the
read's declared params. The read-side analogue of `tx`: one generated command
per read, so the SDK's reads and the CLI's queries can't drift.
"""

from __future__ import annotations

import functools
import inspect
from dataclasses import asdict, is_dataclass
from datetime import datetime
from typing import Any, Optional

import typer

from ..balance import Balance
from ..reads import REGISTRY, Grouped, Matrix
from ..settings import query_docs_url
from . import globals as g
from .context import AppContext, address_cli_name, ctx_of, ss58_param_help
from .output import Output
from .prompt import PromptSpec, fill_missing
from .stake_picker import required_address_spec

_TYPES = {"string": str, "integer": int, "number": float, "boolean": bool}
_TRUE = {"y", "yes", "true", "1"}
_FALSE = {"n", "no", "false", "0"}
_OWN_KEY = ("hotkey_ss58", "coldkey_ss58")


def _cli_name(pname: str) -> str:
    if pname.endswith("_ss58") or "_ss58" in pname:
        return address_cli_name(pname)
    return "--" + pname.replace("_", "-")


def _fetch_default(pname: str, fetch_params: Any) -> Any:
    if pname not in fetch_params:
        return inspect.Parameter.empty
    return fetch_params[pname].default


def _is_required(pname: str, fetch_params) -> bool:
    """True when omitting the param would fail the read.

    Own-key params fall back to the wallet (or keep None for owner lookup).
    A fetch default of ``None`` is not a usable value — those still fail
    (hyperparameter reads declare ``netuid=None`` on the shared fetch).
    A real default (``mechid=0``, ``lite=True``) is optional.
    """
    if pname in _OWN_KEY:
        return False
    default = _fetch_default(pname, fetch_params)
    return default is inspect.Parameter.empty or default is None


def _option_help(spec, pname: str, ptype: str, fetch_params) -> Optional[str]:
    fetch_default = _fetch_default(pname, fetch_params)
    has_fetch_default = fetch_default is not inspect.Parameter.empty
    if pname.endswith("_ss58"):
        input_note = ss58_param_help(pname)
        if has_fetch_default and pname in _OWN_KEY:
            input_note = input_note.split(" Defaults to your wallet")[0]
    else:
        input_note = None
    if ptype == "array":
        input_note = f"{input_note} Comma-separated." if input_note else "Comma-separated list."
    declared = spec.param_docs.get(pname)
    return " ".join(part for part in (declared, input_note) if part) or None


def _query_placeholder(ptype: str) -> Optional[str]:
    return {
        "integer": "integer",
        "number": "number",
        "boolean": "y/n",
        "array": "comma-separated",
    }.get(ptype)


def _parse_query_prompt(ptype: str, _app_ctx: AppContext, raw: str) -> Any:
    if ptype == "integer":
        try:
            return int(raw)
        except ValueError:
            raise ValueError(f"expected an integer, got {raw!r}")
    if ptype == "number":
        try:
            return float(raw)
        except ValueError:
            raise ValueError(f"expected a number, got {raw!r}")
    if ptype == "boolean":
        lowered = raw.lower()
        if lowered in _TRUE:
            return True
        if lowered in _FALSE:
            return False
        raise ValueError(f"expected y or n, got {raw!r}")
    return raw


def _jsonable(obj: Any) -> Any:
    """Normalize a read result to JSON-friendly primitives."""
    if isinstance(obj, Balance):
        return str(obj)
    if isinstance(obj, datetime):
        return obj.isoformat(timespec="seconds")
    if is_dataclass(obj) and not isinstance(obj, type):
        return {k: _jsonable(v) for k, v in asdict(obj).items()}
    if isinstance(obj, dict):
        return {k: _jsonable(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [_jsonable(v) for v in obj]
    return obj


def _records_table(output: Output, name: str, records: list[dict]) -> None:
    """Render a list of records as a table, hiding opaque columns.

    Columns whose values are themselves dicts/lists (e.g. the neurons read's
    ``raw`` blob) are unreadable in a table cell, so they are dropped from the
    human view — ``--json`` carries the full records.
    """
    if not records:
        output.detail(name, {})  # title + the dim `none` convention
        return
    cols = [c for c, v in records[0].items() if not isinstance(v, (dict, list))]
    cols = cols or list(records[0].keys())
    rows = [[r.get(c) for c in cols] for r in records]
    output.table(name, cols, rows, records)


def _make_command(name: str, spec):
    array_params = [p for p, t in spec.params.items() if t == "array"]
    fetch_params = inspect.signature(spec.fetch).parameters

    def command(ctx: typer.Context, **kwargs: Any) -> None:
        g.apply(ctx, kwargs)
        app_ctx = ctx_of(ctx)
        missing: list[PromptSpec] = []
        for pname, ptype in spec.params.items():
            if not _is_required(pname, fetch_params) or kwargs.get(pname) is not None:
                continue
            richer = required_address_spec(pname)
            if richer is not None:
                missing.append(richer)
                continue
            missing.append(
                PromptSpec(
                    field=pname,
                    flag=_cli_name(pname),
                    help=_option_help(spec, pname, ptype, fetch_params),
                    parse=functools.partial(_parse_query_prompt, ptype),
                    placeholder=_query_placeholder(ptype),
                )
            )
        if missing:
            fill_missing(app_ctx, missing, kwargs)
        for pname in array_params:  # comma-separated on the CLI
            kwargs[pname] = [part.strip() for part in str(kwargs[pname]).split(",")]
        for pname in spec.params:
            if not pname.endswith("_ss58"):
                continue
            raw = kwargs.get(pname)
            # Reads that declare an explicit default (e.g. coldkey_ss58=None →
            # look up the hotkey owner) must keep None; only force the wallet
            # fallback when the param is required and merely omitted on the CLI.
            if (
                raw is None
                and pname in ("hotkey_ss58", "coldkey_ss58")
                and pname in fetch_params
                and fetch_params[pname].default is not inspect.Parameter.empty
            ):
                continue
            kwargs[pname] = app_ctx.resolve_address(pname, raw)
        result = app_ctx.run(lambda client: client.read(name, **kwargs))
        payload = _jsonable(result)
        output = app_ctx.output
        if output.json_mode:
            output.value(payload)
        elif payload is None or (isinstance(payload, (list, dict)) and not payload):
            output.detail(name, {})  # title + the dim `none` convention
        elif isinstance(spec.render, Grouped):
            key = spec.render.key
            _records_table(
                output,
                name,
                [{key: group, **item} for group, items in payload.items() for item in items or []],
            )
        elif isinstance(spec.render, Matrix):
            hint = spec.render
            _records_table(
                output,
                name,
                [
                    {hint.row: row, hint.col: col, hint.value: value}
                    for row, cells in payload.items()
                    for col, value in cells.items()
                ],
            )
        elif isinstance(payload, list) and isinstance(payload[0], dict):
            _records_table(output, name, payload)
        elif isinstance(payload, dict):
            output.detail(name, payload)
        else:
            output.value(payload)

    params = [
        inspect.Parameter("ctx", inspect.Parameter.POSITIONAL_OR_KEYWORD, annotation=typer.Context)
    ]
    annotations: dict[str, Any] = {"ctx": typer.Context}
    for pname, ptype in spec.params.items():
        cli_name = _cli_name(pname)
        # Own-key params may be omitted (fall back to the configured wallet);
        # all *_ss58 params also accept local wallet/hotkey names.
        wallet_defaulted = pname in _OWN_KEY
        base_type = _TYPES.get(ptype, str)
        # A default on the fetch function (e.g. mechid=0) makes the option optional.
        fetch_default = _fetch_default(pname, fetch_params)
        has_fetch_default = fetch_default is not inspect.Parameter.empty
        help_text = _option_help(spec, pname, ptype, fetch_params)
        # Optional coldkey_ss58=None (owner lookup) is not a wallet default —
        # keep the option optional without implying the signing wallet coldkey.
        # Required params are Optional at the Click layer so a missing one
        # reaches the command body, which prompts (same as generated tx).
        if wallet_defaulted:
            option_default: Any = fetch_default if has_fetch_default else None
        elif not _is_required(pname, fetch_params):
            option_default = fetch_default
        else:
            option_default = None
            help_text = f"{help_text} (required)" if help_text else "(required)"
        annotations[pname] = Optional[base_type]
        # Bool options get a negated twin (--flag/--no-flag) so default-True
        # fields (e.g. lite) can be turned off — same pattern as `tx.py`.
        flag_name = f"{cli_name}/--no-{cli_name[2:]}" if ptype == "boolean" else cli_name
        params.append(
            inspect.Parameter(
                pname,
                inspect.Parameter.KEYWORD_ONLY,
                default=typer.Option(option_default, flag_name, help=help_text),
                annotation=annotations[pname],
            )
        )
    for p in g.parameters("read"):
        annotations[p.name] = p.annotation
        params.append(p)
    command.__signature__ = inspect.Signature(params)
    command.__annotations__ = annotations
    # The docs page also lists the storage items / runtime APIs the read hits,
    # with source links into the chain code.
    command.__doc__ = f"{spec.doc}\n\nDocs: {query_docs_url(spec.name)}"
    return command


def build_query_app() -> typer.Typer:
    """Assemble the `query` group with one generated subcommand per registered read,
    grouped in --help by each read's declared category."""
    app = typer.Typer(no_args_is_help=True, help="Query chain state (generated from reads).")
    for name, spec in sorted(REGISTRY.items(), key=lambda item: (item[1].category, item[0])):
        app.command(name.replace("_", "-"), rich_help_panel=spec.category)(
            _make_command(name, spec)
        )
    return app
