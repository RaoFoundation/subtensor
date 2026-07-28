"""`btcli config`: persistent defaults for the global options.

Stored values seed the global options when neither a flag nor an env var is
given (see bittensor/config.py for the precedence rules).
"""

from __future__ import annotations

from typing import Optional

import typer

from ... import config as cfg
from ..context import AppContext, ctx_of
from ..globals import with_globals

app = typer.Typer(no_args_is_help=True, help="Read and write persistent CLI config.")


@app.command("set")
@with_globals
def set_value(
    ctx: typer.Context,
    key: str = typer.Argument(..., help=f"One of: {', '.join(sorted(cfg.SETTABLE))}."),
    value: str = typer.Argument(..., help="Value to store."),
):
    """Set a persistent default, e.g. `btcli config set network test`."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        stored = cfg.set_value(key, value)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("set", {"key": key, "value": stored, "path": str(cfg.config_path())})


@app.command("get")
@with_globals
def get_value(
    ctx: typer.Context,
    key: Optional[str] = typer.Argument(None, help="Key to read; omit to show the whole config."),
):
    """Show one stored value, or the entire config."""
    app_ctx: AppContext = ctx_of(ctx)
    data = cfg.load()
    if key is None:
        app_ctx.output.detail(f"config ({cfg.config_path()})", data or {"(empty)": ""})
        return
    if key not in cfg.SETTABLE:
        app_ctx.output.error(
            f"unknown config key {key!r}",
            help=f"settable keys: {', '.join(sorted(cfg.SETTABLE))}",
        )
        raise typer.Exit(1)
    app_ctx.output.value(data.get(key))


@app.command("unset")
@with_globals
def unset_value(
    ctx: typer.Context,
    key: str = typer.Argument(..., help="Key to remove (reverts to env/default)."),
):
    """Remove a stored value."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        existed = cfg.unset_value(key)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("unset", {"key": key, "existed": existed})


@app.command("clear")
@with_globals
def clear_config(ctx: typer.Context):
    """Clear all stored config values.

    Removes every key from the config file. Subsequent commands fall back to
    environment variables and built-in defaults.
    """
    app_ctx: AppContext = ctx_of(ctx)
    data = cfg.load()
    for key in list(data):
        cfg.unset_value(key)
    app_ctx.output.detail("cleared config", {"path": str(cfg.config_path())})


@app.command("path")
@with_globals
def show_path(ctx: typer.Context):
    """Print the config file path."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.value(str(cfg.config_path()))
