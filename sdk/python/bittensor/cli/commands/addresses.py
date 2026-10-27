"""`btcli addr`: local ss58 address book for named contacts."""

from __future__ import annotations

import asyncio
import sys
from typing import Any, Optional

import typer

from ... import config as cfg
from ...extension import BridgeError, ensure_bridge, select_extension_account
from ...settings import FINNEY_GENESIS_HASH
from ...vault import VaultPageError, scan_vault_address
from ...wallets import is_bittensor_address
from ..context import AppContext, ctx_of
from ..globals import with_globals
from ..prompt import PromptSpec, fill_missing, interactive

app = typer.Typer(
    no_args_is_help=True,
    help="Save and reuse named ss58 addresses (for multisig signers, destinations, etc.).",
)

_CONTACT_SIGNERS = ("vault", "ledger", "extension")


def _save(
    app_ctx: AppContext,
    name: str,
    ss58: str,
    note: str,
    *,
    signer: Optional[str] = None,
) -> None:
    if not is_bittensor_address(ss58):
        app_ctx.output.error(f"invalid ss58 address {ss58!r}")
        raise typer.Exit(1)
    entry: dict[str, Any] = {"name": name, "address": ss58, "note": note}
    if not signer:
        previous = cfg.get_address_entry(name)
        if previous:
            signer = previous.get("signer")
    if signer:
        entry["signer"] = signer
    try:
        saved = cfg.add_address(entry)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("saved address", {"entry": saved, "path": str(cfg.addresses_path())})


def _parse_ss58(_app_ctx: AppContext, raw: str) -> str:
    if not is_bittensor_address(raw):
        raise ValueError(f"invalid ss58 address {raw!r}")
    return raw


def _prompt_for_ss58(app_ctx: AppContext) -> str:
    """Ask for the ss58 interactively; error out in non-interactive sessions."""
    if not interactive(app_ctx):
        app_ctx.output.error(
            "missing address",
            help="pass the ss58, --vault to scan it from your phone, or "
            "--extension to pick it from your browser extension",
        )
        raise typer.Exit(2)
    answers: dict = {}
    fill_missing(
        app_ctx,
        [
            PromptSpec(
                field="ss58",
                flag="ss58",
                help="ss58 address for that name (or re-run with --vault to scan it).",
                parse=_parse_ss58,
                positional=True,
            )
        ],
        answers,
    )
    return answers["ss58"]


def _pick_from_extension(app_ctx: AppContext) -> str:
    """Pick an account from the browser extension bridge and return its ss58."""

    async def _pick() -> str:
        url = await ensure_bridge(
            bridge_url=app_ctx.extension_bridge_url,
            open_browser=not app_ctx.output.quiet and sys.stderr.isatty(),
            browser=app_ctx._extension_browser_choice(),
            fresh=True,
            on_waiting=lambda http_url, _status: app_ctx.output.message(
                f"waiting for extension authorization in browser… ({http_url})"
            ),
        )
        selection = await select_extension_account(
            url,
            source=app_ctx.extension_source,
            interactive=sys.stdin.isatty() and not app_ctx.output.json_mode,
        )
        account = selection.account
        app_ctx.output.message(
            f"picked extension account {account.name} ({account.address}, {account.source})"
        )
        return account.address

    try:
        return asyncio.run(_pick())
    except BridgeError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    except KeyboardInterrupt:
        app_ctx.output.message("aborted.")
        raise typer.Exit(130)


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
        None,
        help="ss58 address for that name (omit with --vault/--extension, or to "
        "retag an existing contact).",
    ),
    from_vault: bool = typer.Option(
        False,
        "--vault",
        help="Scan the address from a Polkadot Vault phone via the webcam "
        "(open the key in Vault so its QR is on screen). Tags the contact "
        "as a vault signer so `--signatory NAME` infers `--signer vault`.",
    ),
    from_extension: bool = typer.Option(
        False,
        "--extension",
        help="Pick the address from your browser extension (polkadot{.js}, "
        "Talisman, ...) via the local bridge. Tags the contact as an "
        "extension signer so `--signatory NAME` infers `--signer extension`.",
    ),
    signer: Optional[str] = typer.Option(
        None,
        "--signer",
        help="How this contact signs when it is a multisig member: vault, "
        "ledger, or extension. Implied by --vault / --extension. Lets "
        "`--signatory NAME` pick the backend without extra flags.",
    ),
    note: str = typer.Option("", "--note", help="Optional note stored with the entry."),
):
    """Save a named address: `btcli addr add triumph-a 5FHne...`.

    With `--vault`, the address is scanned from your Polkadot Vault phone
    instead of typed: `btcli addr add my-vault --vault`. With
    `--extension`, it is picked from your browser extension via the local
    bridge: `btcli addr add my-ext --extension`. Both also tag the
    contact so a multisig co-sign is just `--signatory <name>` — and a bare
    `-w <multisig>` run can plan the member's approval automatically.

    To tag an existing contact without re-scanning: `btcli addr add
    VAULT --signer vault`.
    """
    app_ctx = ctx_of(ctx)
    if from_vault and from_extension:
        app_ctx.output.error("give either --vault or --extension, not both")
        raise typer.Exit(2)
    if (from_vault or from_extension) and ss58 is not None:
        app_ctx.output.error("give either an ss58 address or --vault/--extension, not both")
        raise typer.Exit(2)
    if from_vault:
        if signer and signer.strip().lower() != "vault":
            app_ctx.output.error("--vault implies --signer vault")
            raise typer.Exit(2)
        signer = "vault"
        ss58 = _scan_from_vault(app_ctx)
    if from_extension:
        if signer and signer.strip().lower() != "extension":
            app_ctx.output.error("--extension implies --signer extension")
            raise typer.Exit(2)
        signer = "extension"
        ss58 = _pick_from_extension(app_ctx)
    if signer is not None:
        signer = signer.strip().lower()
        if signer not in _CONTACT_SIGNERS:
            app_ctx.output.error(
                f"unknown --signer {signer!r}",
                help="pass vault, ledger, or extension",
            )
            raise typer.Exit(2)
    if ss58 is None:
        existing = cfg.get_address_entry(name)
        if existing and signer:
            ss58 = str(existing["address"])
            if not note:
                note = str(existing.get("note") or "")
        else:
            ss58 = _prompt_for_ss58(app_ctx)
    _save(app_ctx, name, ss58, note, signer=signer)


@app.command()
@with_globals
def save(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Contact name to save."),
    ss58: str = typer.Argument(..., help="ss58 address for that name."),
    note: str = typer.Option("", "--note", help="Optional note stored with the entry."),
):
    """Alias for `add`: `btcli addr save triumph-a 5FHne...`."""
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
