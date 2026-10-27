"""``btcli evm key`` commands."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Optional

import typer

from ....evm import keys as evm_keys
from ....evm.keys import write_keystore_file
from ...context import ctx_of
from ...globals import with_globals, with_unlock_globals
from ...prompt import interactive
from ...secrets import copy_secret_to_clipboard, warn_argv_secrets
from ._shared import (
    EVM_KEY_HELP,
    _key_fields,
    _key_info,
    _key_json,
    _key_ref,
    _password,
    _unlock,
    key_app,
)

_PRIVATE_KEY_RE = re.compile(r"(0x)?[0-9a-fA-F]{64}")


@key_app.command("new")
@with_unlock_globals
def key_new(
    ctx: typer.Context,
    name: str = typer.Option("default", "--name", help="Name for the new key."),
    overwrite: bool = typer.Option(False, "--overwrite", help="Replace an existing key."),
):
    """Create a new EVM key (random secp256k1), stored encrypted in the wallet.

    The keystore file is standard Ethereum keystore V3 — importable into
    MetaMask, geth, or ethers by copying the file. The key is deliberately
    not derived from the coldkey mnemonic, so its compromise surface never
    chains back to the coldkey; use `import --mnemonic` if you want BIP-44
    derivation from a seed you manage yourself.
    """
    app_ctx = ctx_of(ctx)
    try:
        info = evm_keys.create_evm_key(
            name,
            app_ctx.wallet_name,
            app_ctx.wallet_path,
            password=_password(app_ctx),
            overwrite=overwrite,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("created EVM key", _key_fields(info), json_fields=_key_json(info))
    app_ctx.output.message(
        "fund it with `btcli evm fund` — TAO sent to the ss58 mirror appears "
        "as this account's balance in any Ethereum tool"
    )


@key_app.command("import")
@with_unlock_globals
def key_import(
    ctx: typer.Context,
    name: str = typer.Option("default", "--name", help="Name to store the key under."),
    private_key: Optional[str] = typer.Option(
        None,
        "--private-key",
        help="Raw 0x-hex private key. Prompted for securely if no source is given; "
        "avoid passing on the command line (it leaks to shell history and the "
        "process list).",
    ),
    keystore: Optional[str] = typer.Option(
        None, "--keystore", help="Path to a keystore V3 JSON file (e.g. a MetaMask export)."
    ),
    keystore_password: Optional[str] = typer.Option(
        None, "--keystore-password", help="Password of the imported keystore file."
    ),
    mnemonic: Optional[str] = typer.Option(
        None,
        "--mnemonic",
        help=f"BIP-39 mnemonic; derives {evm_keys.ETH_DERIVATION_PATH} (MetaMask account 0).",
    ),
    derivation_path: str = typer.Option(
        evm_keys.ETH_DERIVATION_PATH, "--derivation-path", help="BIP-44 path with --mnemonic."
    ),
    overwrite: bool = typer.Option(False, "--overwrite", help="Replace an existing key."),
):
    """Import an EVM key from a private key, keystore file, or mnemonic.

    Pass one of --private-key, --keystore, or --mnemonic; if none are given
    you are prompted securely on the terminal (a 64-hex-char answer is taken
    as a private key, anything else as a mnemonic).
    """
    app_ctx = ctx_of(ctx)
    warn_argv_secrets(
        app_ctx.output,
        {
            "--private-key": private_key,
            "--mnemonic": mnemonic,
            "--keystore-password": keystore_password,
        },
    )
    # An empty flag value (the old "prompt me" convention) counts as omitted.
    private_key = private_key or None
    mnemonic = mnemonic or None
    if not private_key and not keystore and not mnemonic:
        if not interactive(app_ctx):
            app_ctx.output.error(
                "missing key source: `--private-key`, `--keystore`, or `--mnemonic`",
                help="pass one explicitly, or run on a terminal to be prompted",
            )
            raise typer.Exit(2)
        answer = typer.prompt("EVM private key or mnemonic", hide_input=True).strip()
        if _PRIVATE_KEY_RE.fullmatch(answer):
            private_key = answer
        else:
            mnemonic = answer
    keystore_json = None
    if keystore is not None:
        try:
            keystore_json = Path(keystore).expanduser().read_text()
        except OSError as error:
            app_ctx.output.error(f"cannot read keystore file {keystore!r}: {error}")
            raise typer.Exit(1)
    try:
        info = evm_keys.import_evm_key(
            name,
            app_ctx.wallet_name,
            app_ctx.wallet_path,
            private_key=private_key,
            keystore_json=keystore_json,
            keystore_password=keystore_password,
            mnemonic=mnemonic,
            derivation_path=derivation_path,
            password=_password(app_ctx),
            overwrite=overwrite,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("imported EVM key", _key_fields(info), json_fields=_key_json(info))


@key_app.command("export")
@with_globals
def key_export(
    ctx: typer.Context,
    key: Optional[str] = typer.Argument(None, help=EVM_KEY_HELP),
    out: Optional[str] = typer.Option(
        None, "--out", help="Write the keystore JSON to this file instead of stdout."
    ),
    private_key: bool = typer.Option(
        False,
        "--private-key",
        help="Decrypt the raw 0x-hex private key (for ETH_PRIVATE_KEY in "
        "Hardhat/Foundry). Copied to the clipboard on a terminal; printed when "
        "piped, in --json mode, or with --show. Prefer the encrypted keystore "
        "where the tool supports it.",
    ),
    show: bool = typer.Option(
        False,
        "--show",
        help="With --private-key: print the raw key to the terminal instead of "
        "copying it to the clipboard.",
    ),
):
    """Export a key's keystore V3 JSON (still encrypted) for MetaMask/geth/ethers.

    With `--private-key`, decrypts the raw key instead. On a terminal it goes
    to the clipboard (pass --show to print); piped output still prints, so
    `export ETH_PRIVATE_KEY=$(btcli evm key export --private-key)` keeps
    working.
    """
    app_ctx = ctx_of(ctx)
    info = _key_info(app_ctx, key)
    if private_key:
        if out:
            app_ctx.output.error(
                "--private-key prints to stdout only",
                help="a raw key on disk defeats the keystore; drop --out",
            )
            raise typer.Exit(2)
        account = _unlock(app_ctx, key)
        raw = "0x" + account.key.hex().removeprefix("0x")
        # Piped stdout and JSON mode are data flows (agents, `$(...)`); a real
        # terminal defaults to the clipboard so the key stays out of scrollback.
        to_terminal = show or app_ctx.output.json_mode or not sys.stdout.isatty()
        if not to_terminal and copy_secret_to_clipboard(
            app_ctx.output, raw, f"private key for {info.name} ({info.address})"
        ):
            return
        app_ctx.output.message(
            f"raw private key for {info.name} ({info.address}) — anyone with this "
            "controls the account; it never expires and cannot be revoked"
        )
        app_ctx.output.value(raw)
        return
    keystore = evm_keys.export_evm_key(info.name, _key_ref(app_ctx, key)[0], app_ctx.wallet_path)
    if out:
        write_keystore_file(Path(out).expanduser(), keystore)
        app_ctx.output.detail(
            "exported EVM key", {"name": info.name, "address": info.address, "file": out}
        )
    else:
        app_ctx.output.value(keystore)


@key_app.command("list")
@with_globals
def key_list(ctx: typer.Context):
    """List the wallet's EVM keys: name, address, and ss58 mirror."""
    app_ctx = ctx_of(ctx)
    keys = evm_keys.list_evm_keys(app_ctx.wallet_name, app_ctx.wallet_path)
    app_ctx.output.table(
        f"EVM keys in wallet {app_ctx.wallet_name}",
        ["name", "address", "ss58 mirror"],
        [[k.name, k.address, k.ss58_mirror] for k in keys],
        records=[
            {"name": k.name, "address": k.address, "ss58_mirror": k.ss58_mirror} for k in keys
        ],
    )


@key_app.command("show")
@with_globals
def key_show(
    ctx: typer.Context,
    key: Optional[str] = typer.Argument(None, help=EVM_KEY_HELP),
):
    """Show one EVM key's address, ss58 mirror, and keystore path."""
    app_ctx = ctx_of(ctx)
    info = _key_info(app_ctx, key)
    app_ctx.output.detail("EVM key", _key_fields(info), json_fields=_key_json(info))
