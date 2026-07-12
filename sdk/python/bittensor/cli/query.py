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
from ..reads import REGISTRY
from . import globals as g
from .context import address_cli_name, ctx_of, ss58_param_help

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
        if app_ctx.output.json_mode:
            app_ctx.output.value(payload)
        elif isinstance(payload, list) and payload and isinstance(payload[0], dict):
            cols = list(payload[0].keys())
            rows = [[r.get(c) for c in cols] for r in payload]
            app_ctx.output.table(name, cols, rows, payload)
        elif isinstance(payload, dict):
            app_ctx.output.detail(name, payload)
        else:
            app_ctx.output.value(payload)

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
        params.append(
            inspect.Parameter(
                pname,
                inspect.Parameter.KEYWORD_ONLY,
                default=typer.Option(option_default, cli_name, help=help_text),
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
