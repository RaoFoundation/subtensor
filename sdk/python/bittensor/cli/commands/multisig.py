"""`btcli multisig`: named multisig signer sets and pending operations.

Saved signer sets are reused by `btcli call --multisig NAME` and
`btcli multisig pending`. Entries live in the local multisig book
(see bittensor/config.py).
"""

from __future__ import annotations

from typing import Optional

import typer

from ... import config as cfg
from ... import storage, wallets
from .. import multisig_helpers as ms_helpers
from .. import upgrade_helpers as uh
from ..context import AppContext, ctx_of
from ..globals import with_globals
from ..prompt import confirm_wallet
from .upgrade import load_pending_upgrades, render_upgrade_records

app = typer.Typer(
    no_args_is_help=True,
    help="Save multisig signer sets and track pending multisig operations.",
)


@app.command(
    "add",
    epilog=(
        "Example: btcli multisig add finney-sudo --threshold 2 "
        "--signatories suro,triumph-a,triumph-b"
    ),
)
@with_globals
def multisig_add(
    ctx: typer.Context,
    name: Optional[str] = typer.Argument(
        None, help="Name to save the multisig under (defaults to the -w wallet name)."
    ),
    threshold: int = typer.Option(
        ...,
        "--threshold",
        min=1,
        help="Approvals required before a multisig call executes (the M in M-of-N).",
    ),
    signatories: Optional[str] = typer.Option(
        None,
        "--signatories",
        help="Full signer set: ss58, address-book names, or wallet names.",
    ),
    signatory: Optional[list[str]] = typer.Option(
        None, "--signatory", help="One signatory ref; repeat for each member."
    ),
    note: str = typer.Option("", "--note", help="Free-form note stored with the entry."),
    overwrite: bool = typer.Option(
        False, "--overwrite", help="Replace an existing entry with the same name."
    ),
):
    """Save a named multisig signer set for `pending` and `call --multisig`.

    The entry is stored in the local multisig book; the on-chain multisig
    address is derived from the resolved signer set and threshold and shown
    in the result, alongside whether it matches the chain's sudo key.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if name is None:
        confirm_wallet(
            app_ctx, help_text="Name to save the multisig under (-w name).", must_exist=False
        )
        name = app_ctx.wallet_name
    refs: list[str] = []
    if signatories:
        refs.extend(part.strip() for part in signatories.split(",") if part.strip())
    if signatory:
        refs.extend(signatory)
    refs = list(dict.fromkeys(refs))
    if not refs:
        app_ctx.output.error(
            "no signatories provided",
            help="pass `--signatories` or one or more `--signatory`",
        )
        raise typer.Exit(1)
    resolved = [app_ctx.resolve_address("coldkey_ss58", ref) for ref in refs]
    resolved = list(dict.fromkeys(resolved))
    if threshold > len(resolved):
        app_ctx.output.error(f"threshold {threshold} exceeds {len(resolved)} signatories")
        raise typer.Exit(1)
    if cfg.get_multisig(name) and not overwrite:
        app_ctx.output.error(
            f"multisig {name!r} already exists",
            help="pass `--overwrite` to replace it",
        )
        raise typer.Exit(1)
    try:
        entry = cfg.add_multisig(
            {"name": name, "threshold": threshold, "signatories": refs, "note": note}
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    async def derive(client):
        ms = await client.multisig(resolved, threshold)
        sudo_key = await client.query(storage.Sudo.Key)
        return ms.address, sudo_key

    address, sudo_key = app_ctx.run(derive)
    coldkey_exists = False
    try:
        wallets.open_wallet(name=name, path=app_ctx.wallet_path)
        coldkey_exists = True
    except Exception:
        pass
    app_ctx.output.detail(
        "saved multisig",
        {
            "entry": entry,
            "path": str(cfg.multisigs_path()),
            "multisig_address": address,
            "chain_sudo_key": sudo_key,
            "matches_sudo": address == sudo_key,
            "coldkey_wallet_also_exists": coldkey_exists,
        },
    )


@app.command("list")
@with_globals
def multisig_list(ctx: typer.Context):
    """List saved multisig signer sets."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.detail(
        "multisig book",
        {"path": str(cfg.multisigs_path()), "multisigs": cfg.load_multisigs()},
    )


@app.command("show")
@with_globals
def multisig_show(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Saved multisig name."),
):
    """Derive the on-chain multisig address for a saved signer set."""
    app_ctx: AppContext = ctx_of(ctx)
    entry = cfg.get_multisig(name)
    if entry is None:
        app_ctx.output.error(f"multisig {name!r} not found")
        raise typer.Exit(1)

    signatories = [app_ctx.resolve_address("coldkey_ss58", ref) for ref in entry["signatories"]]
    signatories = list(dict.fromkeys(signatories))

    async def derive(client):
        ms = await client.multisig(signatories, entry["threshold"])
        sudo_key = await client.query(storage.Sudo.Key)
        return ms.address, sudo_key

    address, sudo_key = app_ctx.run(derive)
    app_ctx.output.detail(
        name,
        {
            "threshold": entry["threshold"],
            "signatories": signatories,
            "multisig_address": address,
            "chain_sudo_key": sudo_key,
            "matches_sudo": address == sudo_key,
        },
    )


@app.command("remove")
@with_globals
def multisig_remove(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Saved multisig name."),
):
    """Remove a saved multisig signer set."""
    app_ctx: AppContext = ctx_of(ctx)
    existed = cfg.remove_multisig(name)
    app_ctx.output.detail("removed multisig", {"name": name, "existed": existed})


@app.command(
    "pending",
    epilog=(
        "Example: btcli multisig pending --multisig finney-sudo "
        "--call-hash 0x1234... --call-data 0xabcd..."
    ),
)
@with_globals
def multisig_pending(
    ctx: typer.Context,
    multisig: Optional[str] = typer.Option(
        None,
        "--multisig",
        help="Named multisig (same name as -w); defaults to -w when saved.",
    ),
    multisig_threshold: Optional[int] = typer.Option(
        None,
        "--multisig-threshold",
        help="Approvals needed before the multisig call executes.",
    ),
    signatories: Optional[str] = typer.Option(
        None,
        "--signatories",
        help="Full signer set: ss58, address-book names, or wallet names (include yourself).",
    ),
    other_signatories: Optional[str] = typer.Option(
        None,
        "--other-signatories",
        help="Other signers only (book names or ss58); your -w wallet coldkey is added.",
    ),
    signer: str = typer.Option(
        "coldkey", "--signer", help="Which wallet key is in the signer set: 'coldkey' or 'hotkey'."
    ),
    call_hash: Optional[str] = typer.Option(
        None,
        "--call-hash",
        help="Show one pending operation by call hash.",
    ),
    call_data: Optional[str] = typer.Option(
        None,
        "--call-data",
        help="Scale-encoded call hex. Use when call details are not in local cache.",
    ),
):
    """List pending multisig operations with approval status and co-signer commands."""
    app_ctx: AppContext = ctx_of(ctx)
    if multisig is None and not (signatories or other_signatories):
        confirm_wallet(app_ctx, help_text="Multisig wallet name (-w name).", must_exist=False)
    if signer not in ("coldkey", "hotkey"):
        app_ctx.output.error("signer must be 'coldkey' or 'hotkey'")
        raise typer.Exit(1)
    try:
        threshold, signatories_resolved, preset, signatory_refs = ms_helpers.resolve_multisig(
            app_ctx,
            multisig_name=multisig,
            threshold=multisig_threshold,
            signatories=signatories,
            other_signatories=other_signatories,
            signer=signer,
            wallet_default=app_ctx.wallet_name,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    if threshold is None:
        saved = [str(entry["name"]) for entry in cfg.load_multisigs() if entry.get("name")]
        if saved:
            note = (
                "pending ops are looked up by multisig address, which is derived from a "
                f"threshold and signer set; no saved multisig is named {app_ctx.wallet_name!r}"
            )
            help_text = (
                f"pick a saved multisig ({', '.join(sorted(saved))}) with `--multisig NAME` "
                "or `-w NAME`, or pass `--multisig-threshold N --signatories a,b,c` inline"
            )
        else:
            note = (
                "pending ops are looked up by multisig address, which is derived from a "
                "threshold and signer set; no multisigs are saved on this machine yet"
            )
            help_text = (
                f"save one with `btcli multisig add {app_ctx.wallet_name} "
                "--threshold N --signatories a,b,c`, or pass "
                "`--multisig-threshold N --signatories a,b,c` inline"
            )
        app_ctx.output.error(
            f"no multisig configured for wallet {app_ctx.wallet_name!r}",
            note=note,
            help=help_text,
        )
        raise typer.Exit(1)

    label = preset or f"{threshold}-of-{len(signatories_resolved)}"

    async def _load(client):
        ms = await client.multisig(signatories_resolved, threshold)
        records = await ms_helpers.list_pending_with_commands(
            client,
            app_ctx,
            ms=ms,
            threshold=threshold,
            signatories=signatories_resolved,
            signatory_refs=signatory_refs,
            preset=preset,
            call_hash_filter=call_hash,
            call_data=call_data,
        )
        # When this multisig IS the chain's sudo key, pending runtime-upgrade
        # proposals (held one layer out, on the CI deployment multisig) are
        # part of what its signers are waiting to act on — surface them here.
        upgrades: list = []
        sudo_key = await client.query(storage.Sudo.Key)
        if str(sudo_key) == ms.address and not call_hash:
            upgrades = await load_pending_upgrades(client, app_ctx, uh.DEFAULT_UPGRADE_REPO)
        return records, upgrades

    records, upgrades = app_ctx.run(_load)
    if app_ctx.output.json_mode and upgrades:
        app_ctx.output.value(
            {
                "pending_multisigs": records,
                "count": len(records),
                "pending_upgrades": upgrades,
            }
        )
        return
    app_ctx.output.pending_multisigs(records, title=f"pending multisig — {label}")
    if upgrades:
        render_upgrade_records(app_ctx, upgrades)
