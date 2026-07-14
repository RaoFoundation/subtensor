"""The `btcli query` command group — generated from the reads registry.

Every registered read becomes a subcommand whose options are derived from the
read's declared params. The read-side analogue of `tx`: one generated command
per read, so the SDK's reads and the CLI's queries can't drift.
"""

from __future__ import annotations

import inspect
from dataclasses import asdict, is_dataclass
from datetime import datetime
from typing import Any, Optional

import typer

from ..balance import Balance
from ..reads import REGISTRY, Grouped, Matrix
from . import globals as g
from .context import address_cli_name, ctx_of, ss58_param_help
from .output import Output

_TYPES = {"string": str, "integer": int, "number": float, "boolean": bool}


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

    def command(ctx: typer.Context, **kwargs: Any) -> None:
        g.apply(ctx, kwargs)
        app_ctx = ctx_of(ctx)
        for pname in array_params:  # comma-separated on the CLI
            kwargs[pname] = [part.strip() for part in str(kwargs[pname]).split(",")]
        for pname in spec.params:
            if pname.endswith("_ss58"):
                kwargs[pname] = app_ctx.resolve_address(pname, kwargs.get(pname))
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
    fetch_params = inspect.signature(spec.fetch).parameters
    for pname, ptype in spec.params.items():
        cli_name = (
            address_cli_name(pname)
            if pname.endswith("_ss58") or "_ss58" in pname
            else "--" + pname.replace("_", "-")
        )
        # Own-key params may be omitted (fall back to the configured wallet);
        # all *_ss58 params also accept local wallet/hotkey names.
        wallet_defaulted = pname in ("hotkey_ss58", "coldkey_ss58")
        # Declared meaning first, then the input-shape note (ss58 resolution,
        # comma-separated lists) — same composition as the tx commands.
        input_note = ss58_param_help(pname) if pname.endswith("_ss58") else None
        if ptype == "array":
            input_note = f"{input_note} Comma-separated." if input_note else "Comma-separated list."
        declared = spec.param_docs.get(pname)
        help_text = " ".join(part for part in (declared, input_note) if part) or None
        base_type = _TYPES.get(ptype, str)
        # A default on the fetch function (e.g. mechid=0) makes the option optional.
        fetch_default = (
            fetch_params[pname].default if pname in fetch_params else inspect.Parameter.empty
        )
        if wallet_defaulted:
            option_default: Any = None
        elif fetch_default is not inspect.Parameter.empty:
            option_default = fetch_default
        else:
            option_default = ...
        annotations[pname] = Optional[base_type] if wallet_defaulted else base_type
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
    command.__doc__ = spec.doc
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
