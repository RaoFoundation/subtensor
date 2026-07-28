"""Manage browser extension signing via the local bridge."""

from __future__ import annotations

import asyncio
import json

import typer

from ...extension import (
    DEFAULT_BRIDGE_HOST,
    DEFAULT_BRIDGE_PORT,
    BridgeClient,
    BridgeError,
    ensure_bridge,
    run_bridge,
    stop_bridge_daemon,
)
from ..context import AppContext, ctx_of
from ..globals import with_extension_globals, with_globals

app = typer.Typer(
    no_args_is_help=True,
    help="Browser extension signing (auto-started by --signer extension).",
)


def _bridge_ws_url(host: str, port: int) -> str:
    return f"ws://{host}:{port}/ws"


@app.command("bridge")
@with_globals
def bridge(
    ctx: typer.Context,
    host: str = typer.Option(DEFAULT_BRIDGE_HOST, "--host", help="Bridge bind address."),
    port: int = typer.Option(DEFAULT_BRIDGE_PORT, "--port", help="Bridge listen port."),
    no_open: bool = typer.Option(False, "--no-open", help="Do not open the bridge page."),
):
    """Run the bridge in the foreground (usually not needed; use --signer extension)."""
    app_ctx: AppContext = ctx_of(ctx)

    async def _serve() -> None:
        server = await run_bridge(host=host, port=port, open_browser=not no_open)
        app_ctx.output.message(f"extension bridge listening on {server.http_url}")
        app_ctx.output.message(f"websocket clients: {server.ws_url}")
        await asyncio.Event().wait()

    try:
        asyncio.run(_serve())
    except KeyboardInterrupt:
        app_ctx.output.message("bridge stopped.")


@app.command("stop")
@with_globals
def stop(ctx: typer.Context):
    """Stop the background extension bridge daemon."""
    app_ctx: AppContext = ctx_of(ctx)
    if stop_bridge_daemon():
        app_ctx.output.message("extension bridge stopped.")
    else:
        app_ctx.output.message("extension bridge is not running.")


@app.command("accounts")
@with_extension_globals
def accounts(
    ctx: typer.Context,
    host: str = typer.Option(
        DEFAULT_BRIDGE_HOST, "--host", help="Bridge bind address, used if the bridge is started."
    ),
    port: int = typer.Option(
        DEFAULT_BRIDGE_PORT, "--port", help="Bridge listen port, used if the bridge is started."
    ),
):
    """List accounts exposed by connected browser extensions.

    Starts the local bridge if it is not already running and may open the
    bridge page in your browser so the extension can connect.
    """
    app_ctx: AppContext = ctx_of(ctx)

    async def _list() -> list[dict[str, str]]:
        url = await ensure_bridge(
            bridge_url=app_ctx.extension_bridge_url or _bridge_ws_url(host, port),
            open_browser=not app_ctx.output.quiet,
            browser=app_ctx.extension_browser,
            fresh=True,
        )
        async with BridgeClient(url) as client:
            rows = await client.list_accounts()
            return [
                {
                    "name": row.name,
                    "address": row.address,
                    "source": row.source,
                    "type": row.type,
                }
                for row in rows
            ]

    try:
        rows = asyncio.run(_list())
    except BridgeError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    if app_ctx.output.json_mode:
        typer.echo(json.dumps(rows, indent=2))
        return

    app_ctx.output.table(
        "extension accounts",
        ["name", "address", "source", "type"],
        [[row["name"], row["address"], row["source"], row["type"]] for row in rows],
        rows,
    )
