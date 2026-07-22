"""`btcli addresses`: local ss58 address book for named contacts."""

from __future__ import annotations

import asyncio
import sys
from typing import Optional

import typer

from ... import config as cfg
from ...settings import FINNEY_GENESIS_HASH
from ...vault import VaultPageError, scan_vault_address
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


def _scan_from_vault(app_ctx: AppContext) -> str:
    """Scan a Vault address QR through the webcam page and return the ss58."""
    try:
        address, genesis = asyncio.run(
            scan_vault_address(
                browser=app_ctx.extension_browser,
                open_browser=not app_ctx.output.quiet and sys.stderr.isatty(),
                on_status=app_ctx.output.message,
            )
        )
    except VaultPageError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    except KeyboardInterrupt:
        app_ctx.output.message("aborted.")
        raise typer.Exit(130)
    if genesis is not None and genesis.lower() != FINNEY_GENESIS_HASH:
        app_ctx.output.message(
            f"[dim]note: the key was exported from a network with genesis {genesis[:10]}…, "
            "not Bittensor mainnet — saving the address anyway[/dim]"
        )
    return address


@app.command("add")
@with_globals
def add_address(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Contact name to save."),
    ss58: Optional[str] = typer.Argument(
        None, help="ss58 address for that name (omit with --vault)."
    ),
    from_vault: bool = typer.Option(
        False,
        "--vault",
        help="Scan the address from a Polkadot Vault phone via the webcam "
        "(open the key in Vault so its QR is on screen).",
    ),
    note: str = typer.Option("", "--note", help="Optional note stored with the entry."),
):
    """Save a named address: `btcli addresses add triumph-a 5FHne...`.

    With `--vault`, the address is scanned from your Polkadot Vault phone
    instead of typed: `btcli addresses add my-vault --vault`.
    """
    app_ctx = ctx_of(ctx)
    if from_vault and ss58 is not None:
        app_ctx.output.error("give either an ss58 address or --vault, not both")
        raise typer.Exit(2)
    if from_vault:
        ss58 = _scan_from_vault(app_ctx)
    if ss58 is None:
        app_ctx.output.error(
            "missing address", help="pass the ss58, or --vault to scan it from your phone"
        )
        raise typer.Exit(2)
    _save(app_ctx, name, ss58, note)


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
