"""``btcli evm key`` commands."""

from __future__ import annotations

from pathlib import Path
from typing import Optional

import typer

from ....evm import keys as evm_keys
from ....evm.keys import write_keystore_file
from ...context import ctx_of
from ...globals import with_globals, with_unlock_globals
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
        None, "--private-key", help="Raw 0x-hex private key (prompted for if flag given empty)."
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
    """Import an EVM key from a private key, keystore file, or mnemonic."""
    app_ctx = ctx_of(ctx)
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
        help="Decrypt and print the raw 0x-hex private key (for ETH_PRIVATE_KEY "
        "in Hardhat/Foundry). Prefer the encrypted keystore where the tool "
        "supports it.",
    ),
):
    """Export a key's keystore V3 JSON (still encrypted) for MetaMask/geth/ethers.

    With `--private-key`, decrypts and prints the raw key instead — the shape
    JS toolchains want in an environment variable:
    `export ETH_PRIVATE_KEY=$(btcli evm key export --private-key)`.
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
        app_ctx.output.message(
            f"raw private key for {info.name} ({info.address}) — anyone with this "
            "controls the account; it never expires and cannot be revoked"
        )
        app_ctx.output.value("0x" + account.key.hex().removeprefix("0x"))
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
