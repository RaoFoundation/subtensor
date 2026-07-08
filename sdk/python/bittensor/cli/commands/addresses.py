"""`btcli addresses`: local ss58 address book for named contacts."""

from __future__ import annotations

import typer

from ... import config as cfg
from ...wallets import is_bittensor_address
from ..context import AppContext, ctx_of
from ..globals import with_globals

app = typer.Typer(
    no_args_is_help=True,
    help="Save and reuse named ss58 addresses (for multisig signers, destinations, etc.).",
)


def _save(app_ctx: AppContext, name: str, ss58: str, note: str) -> None:
    if not is_bittensor_address(ss58):
        app_ctx.output.error(f"invalid ss58 address {ss58!r}")
        raise typer.Exit(1)
    try:
        entry = cfg.add_address({"name": name, "address": ss58, "note": note})
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("saved address", {"entry": entry, "path": str(cfg.addresses_path())})


@app.command("add")
@with_globals
def add_address(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Contact name to save."),
    ss58: str = typer.Argument(..., help="ss58 address for that name."),
    note: str = typer.Option("", "--note", help="Optional note stored with the entry."),
):
    """Save a named address: `btcli addresses add triumph-a 5FHne...`."""
    _save(ctx_of(ctx), name, ss58, note)


@app.command()
@with_globals
def save(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Contact name to save."),
    ss58: str = typer.Argument(..., help="ss58 address for that name."),
    note: str = typer.Option("", "--note", help="Optional note stored with the entry."),
):
    """Alias for `add`: `btcli addresses save triumph-a 5FHne...`."""
    _save(ctx_of(ctx), name, ss58, note)


@app.command("list")
@with_globals
def list_addresses(ctx: typer.Context):
    """List saved address-book entries."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.address_list(cfg.addresses_path(), cfg.load_addresses())


@app.command("show")
@with_globals
def show_address(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Saved contact name."),
):
    """Show one saved address."""
    app_ctx: AppContext = ctx_of(ctx)
    entry = next((e for e in cfg.load_addresses() if e.get("name") == name), None)
    if entry is None:
        app_ctx.output.error(f"address {name!r} not found")
        raise typer.Exit(1)
    app_ctx.output.detail(name, entry)


@app.command("remove")
@with_globals
def remove_address(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Saved contact name."),
):
    """Remove a saved address."""
    app_ctx: AppContext = ctx_of(ctx)
    existed = cfg.remove_address(name)
    app_ctx.output.detail("removed address", {"name": name, "existed": existed})
