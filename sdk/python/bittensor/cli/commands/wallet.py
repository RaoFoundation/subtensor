"""`btcli wallet`: create, regenerate, and list keys."""

from __future__ import annotations

import json
import re
import urllib.parse
import urllib.request
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Optional, TypedDict

import typer

from ... import config as cfg
from ... import macos_password, wallets
from ...balance import Balance
from ...intents import (
    AnnounceColdkeySwap,
    AssociateHotkey,
    SetIdentity,
    SwapColdkeyAnnounced,
    SwapHotkey,
    Transfer,
)
from ...keyfiles import WrongPasswordError
from ...settings import BLOCKTIME, resolve_endpoint
from ...timelock import format_duration
from .. import multisig_helpers as ms_helpers
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals, with_unlock_globals
from ..helpers import (
    STAKE_LIST_TITLE,
    annotate_stake_groups_with_locks,
    chain_identity_names,
    dust_note,
    filter_stakes,
    human_balance_fields,
    list_coldkeys,
    local_address_names,
    netuid_groups,
    split_dust,
    wallet_balance_row,
    wallet_balance_rows,
    wallet_inspect_data,
    wallet_overview_rows,
)
from ..prompt import PromptSpec, confirm_wallet, fill_missing, interactive
from ..secrets import copy_secret_to_clipboard, warn_argv_secrets
from ..tx import _parse_money

app = typer.Typer(no_args_is_help=True, help="Create and manage wallets.")

# Semantic --help sections (btcli-style) instead of one long command list.
PANEL_MANAGE = "Wallet management"
PANEL_SECURITY = "Security & recovery"
PANEL_OPS = "Wallet operations"
PANEL_INFO = "Wallet information"
PANEL_IDENTITY = "Identity"

# Registered at the bottom of the module so the Security & recovery panel
# doesn't render first (panels appear in command-registration order).
keychain_app = typer.Typer(
    no_args_is_help=True, help="Store coldkey passwords in the macOS Keychain."
)

_CRYPTO_TYPE_HELP = (
    "Key scheme: ed25519 (0) or sr25519 (1, default). ss58 is the address encoding for both."
)

_SEED_HELP = (
    "32-byte hex seed (alternative to --mnemonic). Avoid passing on the command "
    "line (it leaks to shell history and the process list)."
)

_PRIVATE_KEY_HELP = (
    "64-byte hex private key as stored in a decrypted coldkey/hotkey keyfile "
    "(128 hex characters, optional 0x prefix). This is not the same as --seed: "
    "the first 32 bytes of an sr25519 private key are not a usable seed. Avoid "
    "passing on the command line (it leaks to shell history and the process list)."
)

_N_WORDS_HELP = (
    "Number of words in the generated mnemonic: 12, 15, 18, 21, or 24. "
    "More words means more entropy."
)

_NO_PASSWORD_HELP = (
    "Store the coldkey unencrypted on disk. Anyone who can read the file can "
    "spend from it; only use this for test or throwaway keys."
)

_OVERWRITE_HELP = (
    "Replace existing key files with the same name. The old key becomes "
    "unrecoverable unless you have its mnemonic or a backup."
)

_JSON_PATH_HELP = (
    "Path to a PolkadotJS encrypted JSON keystore to import (alternative to "
    "--mnemonic or --seed). Tilde paths are expanded."
)

_JSON_PASSWORD_HELP = (
    "Passphrase for the encrypted JSON keystore. Prompted securely if "
    "--json-path is set and this is omitted."
)

_SEED_RE = re.compile(r"(0x)?[0-9a-fA-F]{64}")
_PRIVATE_KEY_RE = re.compile(r"(0x)?[0-9a-fA-F]{128}")


def _resolve_key_secret(
    app_ctx: AppContext,
    kind: str,
    mnemonic: Optional[str],
    seed: Optional[str],
    private_key: Optional[str] = None,
) -> tuple[Optional[str], Optional[str], Optional[str]]:
    """Settle mnemonic / seed / private_key for a regen command.

    Exactly one source. When none are passed, prompt securely and auto-detect:
    128 hex chars → private key, 64 hex chars → seed, anything else → mnemonic.
    """
    provided = sum(bool(value) for value in (mnemonic, seed, private_key))
    if provided > 1:
        app_ctx.output.error("pass only one of `--mnemonic`, `--seed`, or `--private-key`")
        raise typer.Exit(2)
    if private_key is not None:
        if not _PRIVATE_KEY_RE.fullmatch(private_key):
            app_ctx.output.error(
                "private key must be 64 bytes of hex (128 hex characters, optional 0x prefix)"
            )
            raise typer.Exit(2)
        return None, None, private_key
    if seed is not None:
        # Validate here: the wallet lib panics (rust) on malformed hex.
        if not _SEED_RE.fullmatch(seed):
            app_ctx.output.error(
                "seed must be 32 bytes of hex (64 hex characters, optional 0x prefix); "
                "for a 64-byte keyfile privateKey use --private-key"
            )
            raise typer.Exit(2)
        return None, seed, None
    if mnemonic is not None:
        return mnemonic, None, None
    if not interactive(app_ctx):
        app_ctx.output.error(
            "missing required option: `--mnemonic`, `--seed`, or `--private-key`",
            help="pass one explicitly, or run on a terminal to be prompted",
        )
        raise typer.Exit(2)
    answer = typer.prompt(f"{kind} mnemonic, hex seed, or private key", hide_input=True).strip()
    if _PRIVATE_KEY_RE.fullmatch(answer):
        return None, None, answer
    if _SEED_RE.fullmatch(answer):
        return None, answer, None
    return answer, None, None


def _resolve_coldkey_source(
    app_ctx: AppContext,
    mnemonic: Optional[str],
    seed: Optional[str],
    private_key: Optional[str],
    json_path: Optional[str],
    json_password: Optional[str],
) -> tuple[Optional[str], Optional[str], Optional[str], Optional[tuple[str, str]]]:
    """Resolve exactly one coldkey source: mnemonic, seed, private key, or JSON."""
    provided = sum(bool(value) for value in (mnemonic, seed, private_key, json_path))
    if provided > 1:
        app_ctx.output.error(
            "pass only one of `--mnemonic`, `--seed`, `--private-key`, or `--json-path`"
        )
        raise typer.Exit(2)

    if json_path:
        expanded = str(Path(json_path).expanduser())
        keystore_path = Path(expanded)
        if not keystore_path.is_file():
            app_ctx.output.error(f"JSON file does not exist: {expanded}")
            raise typer.Exit(2)
        json_data = keystore_path.read_text()
        if not json_password:
            if not interactive(app_ctx):
                app_ctx.output.error(
                    "missing required option: `--json-password`",
                    help="pass it explicitly, or run on a terminal to be prompted",
                )
                raise typer.Exit(2)
            json_password = typer.prompt(
                "Enter the backup password for the JSON file",
                hide_input=True,
            )
        if not json_password:
            app_ctx.output.error("JSON keystore password cannot be empty")
            raise typer.Exit(2)
        return None, None, None, (json_data, json_password)

    mnemonic, seed, private_key = _resolve_key_secret(
        app_ctx, "Coldkey", mnemonic, seed, private_key
    )
    return mnemonic, seed, private_key, None


def _resolve_crypto_type(app_ctx: AppContext, value: str) -> int:
    try:
        return wallets.parse_crypto_type(value)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)


class _UnlockOptions(TypedDict):
    password_file: Optional[str]
    macos_prompt: bool
    keychain: bool


def _unlock_options(app_ctx: AppContext) -> _UnlockOptions:
    return {
        "password_file": app_ctx.wallet_password_file,
        "macos_prompt": app_ctx.macos_password,
        "keychain": app_ctx.keychain_password,
    }


def _is_wrong_password(error: BaseException) -> bool:
    if isinstance(error, WrongPasswordError):
        return True
    cause = error.__cause__
    if isinstance(cause, WrongPasswordError):
        return True
    return str(error).lower().startswith("wrong password")


def _report_unlock_error(app_ctx: AppContext, error: BaseException) -> None:
    """Print a rustc-style unlock failure and exit (no traceback)."""
    if _is_wrong_password(error):
        help_text = (
            "re-save with `btcli wallet keychain save`"
            if app_ctx.keychain_password
            else "re-run and enter the coldkey password used when the key was created"
        )
        app_ctx.output.error("wrong password", help=help_text)
    else:
        text = str(error).strip()
        if text.endswith("."):
            text = text[:-1]
        if text:
            text = text[0].lower() + text[1:]
        app_ctx.output.error(text or "could not unlock coldkey")
    raise typer.Exit(1)


@app.command(rich_help_panel=PANEL_MANAGE)
@with_globals
def create(
    ctx: typer.Context,
    n_words: int = typer.Option(12, "--n-words", help=_N_WORDS_HELP),
    no_password: bool = typer.Option(False, "--no-password", help=_NO_PASSWORD_HELP),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
    hotkey_crypto_type: str = typer.Option(
        "sr25519",
        "--hotkey-crypto-type",
        help="Key scheme for the hotkey: ed25519 or sr25519 (default).",
    ),
):
    """Create a new coldkey and hotkey.

    Writes key files under the wallet path and prints each key's mnemonic to
    the terminal — record them somewhere safe, they are the only way to
    recover the keys. Prompts for a coldkey encryption password unless
    --no-password is given; the hotkey is always stored unencrypted.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx,
        help_text="Wallet name to create.",
        must_exist=False,
        hotkey_help="Name for the new hotkey.",
        hotkey_must_exist=False,
    )
    coldkey_crypto = _resolve_crypto_type(app_ctx, crypto_type)
    hotkey_crypto = _resolve_crypto_type(app_ctx, hotkey_crypto_type)
    try:
        wallet = wallets.create(
            name=app_ctx.wallet_name,
            hotkey=app_ctx.hotkey_name,
            path=app_ctx.wallet_path,
            n_words=n_words,
            use_password=not no_password,
            overwrite=overwrite,
            coldkey_crypto_type=coldkey_crypto,
            hotkey_crypto_type=hotkey_crypto,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail(
        "created wallet",
        {
            "coldkey": app_ctx.wallet_name,
            "hotkey": app_ctx.hotkey_name,
            "coldkey_crypto_type": wallets.format_crypto_type(coldkey_crypto),
            "hotkey_crypto_type": wallets.format_crypto_type(hotkey_crypto),
            "coldkey_ss58": wallet.coldkeypub.ss58_address,
            "path": app_ctx.wallet_path,
        },
    )


@app.command("new-coldkey", rich_help_panel=PANEL_MANAGE)
@with_globals
def new_coldkey(
    ctx: typer.Context,
    n_words: int = typer.Option(12, "--n-words", help=_N_WORDS_HELP),
    no_password: bool = typer.Option(False, "--no-password", help=_NO_PASSWORD_HELP),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
):
    """Create a new coldkey in the configured wallet.

    Writes coldkey files under the wallet path and prints the mnemonic to the
    terminal — record it somewhere safe, it is the only way to recover the
    key. Prompts for an encryption password unless --no-password is given.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet to create the coldkey in.", must_exist=False)
    crypto = _resolve_crypto_type(app_ctx, crypto_type)
    try:
        wallet = wallets.new_coldkey(
            name=app_ctx.wallet_name,
            path=app_ctx.wallet_path,
            n_words=n_words,
            use_password=not no_password,
            overwrite=overwrite,
            crypto_type=crypto,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail(
        "created coldkey",
        {
            "wallet": app_ctx.wallet_name,
            "crypto_type": wallets.format_crypto_type(crypto),
            "ss58": wallet.coldkeypub.ss58_address,
        },
    )


@app.command("new-hotkey", rich_help_panel=PANEL_MANAGE)
@with_globals
def new_hotkey(
    ctx: typer.Context,
    n_words: int = typer.Option(12, "--n-words", help=_N_WORDS_HELP),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
):
    """Create a new hotkey in the configured wallet.

    Writes the hotkey file (stored unencrypted) under the wallet path and
    prints its mnemonic to the terminal — record it if you need to
    regenerate the hotkey later.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx,
        help_text="Wallet to create the hotkey in.",
        must_exist=False,
        hotkey_help="Name for the new hotkey.",
        hotkey_must_exist=False,
    )
    crypto = _resolve_crypto_type(app_ctx, crypto_type)
    wallet = wallets.new_hotkey(
        name=app_ctx.wallet_name,
        hotkey=app_ctx.hotkey_name,
        path=app_ctx.wallet_path,
        n_words=n_words,
        overwrite=overwrite,
        crypto_type=crypto,
    )
    app_ctx.output.detail(
        "created hotkey",
        {
            "wallet": app_ctx.wallet_name,
            "hotkey": app_ctx.hotkey_name,
            "crypto_type": wallets.format_crypto_type(crypto),
            "ss58": wallet.hotkey.ss58_address,
        },
    )


@app.command("regen-coldkey", rich_help_panel=PANEL_SECURITY)
@with_globals
def regen_coldkey(
    ctx: typer.Context,
    mnemonic: str = typer.Option(
        None,
        "--mnemonic",
        help="Coldkey mnemonic. Prompted for securely if omitted; avoid passing on "
        "the command line (it leaks to shell history and the process list).",
    ),
    seed: str = typer.Option(None, "--seed", help=_SEED_HELP),
    private_key: str = typer.Option(None, "--private-key", help=_PRIVATE_KEY_HELP),
    json_path: str | None = typer.Option(
        None,
        "--json-path",
        "-j",
        help=_JSON_PATH_HELP,
    ),
    json_password: str | None = typer.Option(
        None,
        "--json-password",
        help=_JSON_PASSWORD_HELP,
    ),
    no_password: bool = typer.Option(False, "--no-password", help=_NO_PASSWORD_HELP),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
):
    """Regenerate a coldkey from a mnemonic, seed, private key, or JSON keystore.

    Pass exactly one of --mnemonic, --seed, --private-key, or --json-path; if
    none are given you are prompted securely on the terminal (128-hex → private
    key, 64-hex → seed, otherwise mnemonic). Rewrites the wallet's coldkey
    files on disk and prompts for a new encryption password unless --no-password
    is given. When importing from JSON, the key type is read from the keystore;
    --crypto-type applies only to mnemonic/seed/private-key regeneration.
    """
    app_ctx: AppContext = ctx_of(ctx)
    warn_argv_secrets(
        app_ctx.output,
        {
            "--mnemonic": mnemonic,
            "--seed": seed,
            "--private-key": private_key,
            "--json-password": json_password,
        },
    )
    mnemonic, seed, private_key, json_keystore = _resolve_coldkey_source(
        app_ctx,
        mnemonic,
        seed,
        private_key,
        json_path,
        json_password,
    )
    confirm_wallet(app_ctx, help_text="Wallet to regenerate the coldkey in.", must_exist=False)
    crypto = _resolve_crypto_type(app_ctx, crypto_type)
    try:
        wallet = wallets.regen_coldkey(
            mnemonic=mnemonic,
            seed=seed,
            private_key=private_key,
            json=json_keystore,
            name=app_ctx.wallet_name,
            path=app_ctx.wallet_path,
            use_password=not no_password,
            overwrite=overwrite,
            crypto_type=crypto,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    reported_crypto = wallet.coldkey.crypto_type if json_keystore else crypto
    app_ctx.output.detail(
        "regenerated coldkey",
        {
            "coldkey": app_ctx.wallet_name,
            "crypto_type": wallets.format_crypto_type(reported_crypto),
            "ss58": wallet.coldkeypub.ss58_address,
            "path": app_ctx.wallet_path,
        },
    )


@app.command("regen-hotkey", rich_help_panel=PANEL_SECURITY)
@with_globals
def regen_hotkey(
    ctx: typer.Context,
    mnemonic: str = typer.Option(
        None,
        "--mnemonic",
        help="Hotkey mnemonic. Prompted for securely if omitted.",
    ),
    seed: str = typer.Option(None, "--seed", help=_SEED_HELP),
    private_key: str = typer.Option(None, "--private-key", help=_PRIVATE_KEY_HELP),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
):
    """Regenerate a hotkey from a mnemonic, hex seed, or private key.

    Pass exactly one of --mnemonic, --seed, or --private-key; if none are given
    you are prompted securely on the terminal (128-hex → private key, 64-hex →
    seed, otherwise mnemonic). Rewrites the hotkey file (stored unencrypted)
    under the wallet path. The crypto type must match the one the key was
    created with, or the regenerated key will have a different address.
    """
    app_ctx: AppContext = ctx_of(ctx)
    warn_argv_secrets(
        app_ctx.output,
        {"--mnemonic": mnemonic, "--seed": seed, "--private-key": private_key},
    )
    mnemonic, seed, private_key = _resolve_key_secret(
        app_ctx, "Hotkey", mnemonic, seed, private_key
    )
    confirm_wallet(
        app_ctx,
        help_text="Wallet to regenerate the hotkey in.",
        must_exist=False,
        hotkey_help="Name for the regenerated hotkey.",
        hotkey_must_exist=False,
    )
    crypto = _resolve_crypto_type(app_ctx, crypto_type)
    wallet = wallets.regen_hotkey(
        mnemonic=mnemonic,
        seed=seed,
        private_key=private_key,
        name=app_ctx.wallet_name,
        hotkey=app_ctx.hotkey_name,
        path=app_ctx.wallet_path,
        overwrite=overwrite,
        crypto_type=crypto,
    )
    app_ctx.output.detail(
        "regenerated hotkey",
        {
            "coldkey": app_ctx.wallet_name,
            "hotkey": app_ctx.hotkey_name,
            "crypto_type": wallets.format_crypto_type(crypto),
            "ss58": wallet.hotkey.ss58_address,
            "path": app_ctx.wallet_path,
        },
    )


@app.command("regen-coldkeypub", rich_help_panel=PANEL_SECURITY)
@with_globals
def regen_coldkey_pub(
    ctx: typer.Context,
    ss58: str = typer.Option(..., "--ss58", help="ss58 address of the coldkey."),
    public_key: str = typer.Option(
        ..., "--public-key", help="Hex-encoded public key matching the ss58 address."
    ),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
):
    """Regenerate coldkey public file from ss58 + public key.

    Writes only the coldkeypub file (no secret material), which is enough for
    watch-only operations like checking balances. The wallet cannot sign
    until the full coldkey is regenerated from its mnemonic or seed.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet to regenerate the coldkeypub in.", must_exist=False)
    crypto = _resolve_crypto_type(app_ctx, crypto_type)
    wallets.regen_coldkey_pub(
        ss58=ss58,
        public_key_hex=public_key,
        name=app_ctx.wallet_name,
        path=app_ctx.wallet_path,
        overwrite=overwrite,
        crypto_type=crypto,
    )
    app_ctx.output.detail(
        "regenerated coldkeypub",
        {"ss58": ss58, "crypto_type": wallets.format_crypto_type(crypto)},
    )


@app.command("regen-hotkeypub", rich_help_panel=PANEL_SECURITY)
@with_globals
def regen_hotkey_pub(
    ctx: typer.Context,
    ss58: str = typer.Option(..., "--ss58", help="ss58 address of the hotkey."),
    public_key: str = typer.Option(
        ..., "--public-key", help="Hex-encoded public key matching the ss58 address."
    ),
    overwrite: bool = typer.Option(False, "--overwrite", help=_OVERWRITE_HELP),
    crypto_type: str = typer.Option("sr25519", "--crypto-type", help=_CRYPTO_TYPE_HELP),
):
    """Regenerate hotkey public file from ss58 + public key.

    Writes only the hotkeypub file (no secret material). The hotkey cannot
    sign until the full hotkey is regenerated from its mnemonic or seed.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx,
        help_text="Wallet to regenerate the hotkeypub in.",
        must_exist=False,
        hotkey_help="Name for the regenerated hotkeypub.",
        hotkey_must_exist=False,
    )
    crypto = _resolve_crypto_type(app_ctx, crypto_type)
    wallets.regen_hotkey_pub(
        ss58=ss58,
        public_key_hex=public_key,
        name=app_ctx.wallet_name,
        hotkey=app_ctx.hotkey_name,
        path=app_ctx.wallet_path,
        overwrite=overwrite,
        crypto_type=crypto,
    )
    app_ctx.output.detail(
        "regenerated hotkeypub",
        {"ss58": ss58, "crypto_type": wallets.format_crypto_type(crypto)},
    )


@app.command(rich_help_panel=PANEL_OPS)
@with_unlock_globals
def sign(
    ctx: typer.Context,
    message: str = typer.Option(..., "--message", help="Message text to sign (utf-8)."),
    use_hotkey: bool = typer.Option(
        False, "--use-hotkey", help="Sign with the hotkey instead of the coldkey."
    ),
):
    """Sign a message with the wallet's coldkey (or hotkey).

    Signing with the coldkey requires unlocking it, so you may be prompted
    for the wallet password. The signature is printed as bare hex (no ``0x``)
    under the classic ``signed_message`` / ``signer_address`` field names so
    paste-into-verifier flows match old btcli. Checked with `btcli wallet verify`.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx, help_text="Wallet that signs the message.", require_coldkey=not use_hotkey
    )
    try:
        signed = wallets.sign_message(
            message,
            name=app_ctx.wallet_name,
            hotkey=app_ctx.hotkey_name,
            path=app_ctx.wallet_path,
            use="hotkey" if use_hotkey else "coldkey",
            **_unlock_options(app_ctx),
        )
    except (ValueError, OSError, WrongPasswordError) as error:
        _report_unlock_error(app_ctx, error)
    # Classic btcli field names + bare hex (no 0x).
    app_ctx.output.detail(
        "signed",
        {
            "signed_message": signed["signature"].removeprefix("0x"),
            "signer_address": signed["ss58"],
        },
    )


@app.command(rich_help_panel=PANEL_OPS)
@with_globals
def verify(
    ctx: typer.Context,
    message: str = typer.Option(..., "--message", help="The exact message that was signed."),
    signature: str = typer.Option(
        ..., "--signature", help="Hex signature (with or without a 0x prefix)."
    ),
    ss58: str = typer.Option(..., "--ss58", help="Address the message was signed with."),
):
    """Verify a message signature against an address."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        ok = wallets.verify_message(message, signature, ss58)
    except (ValueError, TypeError) as error:
        app_ctx.output.error(f"invalid signature or address: {error}")
        raise typer.Exit(1)
    app_ctx.output.detail(None, {"valid": ok, "ss58": ss58})
    if not ok:
        raise typer.Exit(1)


@app.command(rich_help_panel=PANEL_OPS)
@with_globals
def encrypt(
    ctx: typer.Context,
    message: str = typer.Option(..., "--message", help="Message text to encrypt (utf-8)."),
    recipient: str = typer.Option(
        ...,
        "--recipient",
        help="Recipient ss58 address. Must be an ed25519 key; sr25519 keys "
        "cannot receive encrypted messages.",
    ),
):
    """Encrypt a message for a recipient (ED25519)."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        result = wallets.encrypt_message(message, recipient)
    except ValueError as error:
        app_ctx.output.error(
            str(error),
            note="only ed25519 keys can receive encrypted messages; sr25519 public "
            "keys cannot be converted to X25519",
            help="ask the recipient for an ed25519 address, e.g. from a key created "
            "with `--crypto-type ed25519` (`btcli wallet list` shows each key's scheme)",
        )
        raise typer.Exit(1)
    app_ctx.output.detail("encrypted", result)


@app.command(rich_help_panel=PANEL_OPS)
@with_unlock_globals
def decrypt(
    ctx: typer.Context,
    ciphertext: str = typer.Option(..., "--ciphertext", help="0x-hex ciphertext."),
    use_hotkey: bool = typer.Option(
        False, "--use-hotkey", help="Decrypt with the hotkey instead of the coldkey."
    ),
    copy: bool = typer.Option(
        False,
        "--copy",
        help="Copy the decrypted message to the clipboard instead of printing it "
        "(keeps secrets out of terminal scrollback).",
    ),
):
    """Decrypt a message with the wallet key.

    Uses the coldkey by default, which requires unlocking it (you may be
    prompted for the wallet password). The decrypted plaintext is printed to
    the terminal, or copied to the clipboard with --copy.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if copy and app_ctx.output.json_mode:
        app_ctx.output.error(
            "`--copy` does not apply in --json mode",
            help="drop --copy; JSON output prints the decrypted message",
        )
        raise typer.Exit(2)
    confirm_wallet(
        app_ctx, help_text="Wallet that decrypts the message.", require_coldkey=not use_hotkey
    )
    try:
        plaintext = wallets.decrypt_message(
            ciphertext,
            name=app_ctx.wallet_name,
            hotkey=app_ctx.hotkey_name,
            path=app_ctx.wallet_path,
            use_hotkey=use_hotkey,
            **_unlock_options(app_ctx),
        )
    except Exception as error:
        app_ctx.output.error(f"decryption failed: {error}")
        raise typer.Exit(1)
    if copy and copy_secret_to_clipboard(app_ctx.output, plaintext, "decrypted message"):
        return
    app_ctx.output.detail("decrypted", {"message": plaintext})


@app.command("unlock", rich_help_panel=PANEL_SECURITY)
@with_unlock_globals
def unlock_wallet(ctx: typer.Context):
    """Unlock the configured wallet's coldkey (prompts for the password on a terminal)."""
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet to unlock.")
    wallet = app_ctx.wallet()
    if not wallet.coldkey_file.is_encrypted():
        app_ctx.output.detail(
            "unlock",
            {
                "wallet": app_ctx.wallet_name,
                "encrypted": False,
                "ss58": wallet.coldkeypub.ss58_address,
            },
        )
        return
    try:
        password = wallets.resolve_wallet_password(wallet, **_unlock_options(app_ctx))
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    if password is None and not interactive(app_ctx):
        app_ctx.output.error(
            "no password source available",
            help=(
                "pass --macos-password, --keychain-password, or --wallet-password-file, "
                "or run on a terminal to be prompted"
            ),
        )
        raise typer.Exit(1)
    prompted = password is None
    keypair = None
    for attempts_left in (2, 1, 0):
        if password is None:
            password = typer.prompt(
                f"password for wallet {app_ctx.wallet_name!r}", hide_input=True, err=True
            )
        try:
            keypair = wallets.signing_keypair(
                wallet,
                "coldkey",
                password=password,
                macos_prompt=app_ctx.macos_password,
                keychain=app_ctx.keychain_password,
            )
            break
        except (ValueError, OSError, WrongPasswordError) as error:
            if prompted and _is_wrong_password(error) and attempts_left:
                app_ctx.output.error("wrong password")
                password = None
                continue
            _report_unlock_error(app_ctx, error)
    app_ctx.output.detail(
        "unlocked",
        {
            "wallet": app_ctx.wallet_name,
            "encrypted": True,
            "ss58": keypair.ss58_address,
        },
    )


@keychain_app.command("save")
@with_globals
def keychain_save(ctx: typer.Context):
    """Save the wallet coldkey password in macOS Keychain (prompts via native dialog).

    Stores the password in your login Keychain so later commands can unlock
    the coldkey with --keychain-password instead of prompting. Anyone with
    access to your logged-in macOS session can use the stored password.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx, help_text="Wallet whose coldkey password to save.", require_coldkey=False
    )
    if not macos_password.is_macos():
        app_ctx.output.error("macOS Keychain is only available on darwin")
        raise typer.Exit(1)
    try:
        password = macos_password.prompt_password(
            title=f"Save {app_ctx.wallet_name} coldkey password",
            message=(
                f"Enter the password for wallet {app_ctx.wallet_name!r}. "
                "It will be stored in your macOS Keychain."
            ),
        )
        macos_password.keychain_save(app_ctx.wallet_name, password)
    except (ValueError, OSError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail(
        "saved keychain password",
        {
            "wallet": app_ctx.wallet_name,
            "service": macos_password.KEYCHAIN_SERVICE,
            "account": macos_password.keychain_account(app_ctx.wallet_name),
        },
    )


@keychain_app.command("show")
@with_globals
def keychain_show(ctx: typer.Context):
    """Check whether a coldkey password is stored in macOS Keychain."""
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx, help_text="Wallet whose keychain entry to check.", require_coldkey=False
    )
    if not macos_password.is_macos():
        app_ctx.output.error("macOS Keychain is only available on darwin")
        raise typer.Exit(1)
    stored = macos_password.keychain_load(app_ctx.wallet_name) is not None
    app_ctx.output.detail(
        "keychain",
        {
            "wallet": app_ctx.wallet_name,
            "service": macos_password.KEYCHAIN_SERVICE,
            "stored": stored,
        },
    )


@keychain_app.command("delete")
@with_globals
def keychain_delete(ctx: typer.Context):
    """Remove the wallet coldkey password from macOS Keychain."""
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(
        app_ctx, help_text="Wallet whose keychain entry to remove.", require_coldkey=False
    )
    if not macos_password.is_macos():
        app_ctx.output.error("macOS Keychain is only available on darwin")
        raise typer.Exit(1)
    existed = macos_password.keychain_delete(app_ctx.wallet_name)
    app_ctx.output.detail(
        "deleted keychain password",
        {"wallet": app_ctx.wallet_name, "existed": existed},
    )


@app.command("show", rich_help_panel=PANEL_INFO)
@with_globals
def show_wallet(ctx: typer.Context):
    """Show the configured wallet's public keys and crypto schemes."""
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet to show.")
    wallet = app_ctx.wallet()
    coldkey_crypto = wallet.coldkeypub.crypto_type
    hotkey_crypto = wallet.hotkey.crypto_type
    app_ctx.output.detail(
        app_ctx.wallet_name,
        {
            "coldkey_ss58": wallet.coldkeypub.ss58_address,
            "coldkey_crypto_type": wallets.format_crypto_type(coldkey_crypto),
            "hotkey": app_ctx.hotkey_name,
            "hotkey_ss58": wallet.hotkey.ss58_address,
            "hotkey_crypto_type": wallets.format_crypto_type(hotkey_crypto),
            "path": app_ctx.wallet_path,
        },
    )


@app.command("list", rich_help_panel=PANEL_INFO)
@with_globals
def list_wallets(ctx: typer.Context):
    """List wallets on disk, saved multisigs, the address book, and the proxy book."""
    app_ctx: AppContext = ctx_of(ctx)
    coldkeys = wallets.list_wallets_detailed(app_ctx.wallet_path)
    records = [
        {
            "coldkey": ck.name,
            "ss58": ck.ss58,
            "crypto_type": wallets.format_crypto_type(ck.crypto_type),
            "hotkeys": [
                {
                    "name": hk.name,
                    "ss58": hk.ss58,
                    "crypto_type": wallets.format_crypto_type(hk.crypto_type),
                }
                for hk in ck.hotkeys
            ],
        }
        for ck in coldkeys
    ]
    multisig_entries = cfg.load_multisigs()
    multisigs = (
        app_ctx.run(lambda client: ms_helpers.multisig_list_records(client, app_ctx))
        if multisig_entries
        else []
    )
    app_ctx.output.wallet_list(
        app_ctx.wallet_path,
        records,
        multisigs=multisigs,
        addresses=cfg.load_addresses(),
        proxies=cfg.load_proxies(),
    )


@app.command("balance", rich_help_panel=PANEL_INFO)
@with_globals
def wallet_balance(
    ctx: typer.Context,
    address: Optional[str] = typer.Argument(
        None,
        help="Coldkey ss58 address, a local wallet name, or an address-book / "
        "saved multisig name. Defaults to the configured wallet's coldkey.",
    ),
    all_wallets: bool = typer.Option(
        False, "--all", "-a", help="Show balances for every wallet under --wallet-path."
    ),
    sort_by: Optional[str] = typer.Option(
        None,
        "--sort",
        help="When using --all: name, free, stake-value, or total-value "
        "(default: total-value, largest first).",
    ),
    show_empty: bool = typer.Option(
        False,
        "--empty",
        help="Also show wallets with zero free balance and zero stake. "
        "JSON always includes every wallet.",
    ),
):
    """Show free TAO and stake marked to TAO at spot prices (excludes slippage/fees)."""
    app_ctx: AppContext = ctx_of(ctx)
    if all_wallets:
        coldkeys = list_coldkeys(app_ctx.wallet_path)
        if not coldkeys:
            app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
            raise typer.Exit(1)

        async def _all(client):
            return await wallet_balance_rows(client, coldkeys)

        rows_data = app_ctx.run(_all)
        key_map = {
            "name": lambda r: wallets.natural_name_key(r["wallet"]),
            "free": lambda r: r["free_tao"],
            "stake-value": lambda r: r["stake_value_tao"],
            "total-value": lambda r: r["total_value_tao"],
        }
        if sort_by is not None and sort_by not in key_map:
            app_ctx.output.error(
                f"unknown sort key {sort_by!r}",
                help="use: name, free, stake-value, or total-value",
            )
            raise typer.Exit(1)
        chosen = sort_by or "total-value"
        rows_data.sort(key=key_map[chosen], reverse=chosen != "name")

        shown = rows_data if show_empty else [r for r in rows_data if r["total_value_tao"] > 0]
        hidden = len(rows_data) - len(shown)

        def _amount(display: object, tao: float) -> str:
            return "—" if tao == 0 else str(display)

        table_rows = [
            [
                r["wallet"],
                _amount(r["free"], r["free_tao"]),
                _amount(r["stake_value"], r["stake_value_tao"]),
                _amount(r["total_value"], r["total_value_tao"]),
                r["coldkey"],
            ]
            for r in shown
        ]
        # Several wallet names can point at the same coldkey (regens, backups);
        # count each coldkey once so the total is real funds, not row math.
        by_coldkey = {r["coldkey"]: r for r in rows_data}
        grand_total = Balance(sum(int(r["total_value"].rao) for r in by_coldkey.values()))
        duplicates = len(rows_data) - len(by_coldkey)
        footer = f"[bold]total[/bold] {grand_total}  [dim](spot, excl. slippage/fees"
        if duplicates:
            footer += f"; {duplicates} duplicate-coldkey wallets counted once"
        footer += ")[/dim]"
        app_ctx.output.columns(
            "wallet balances (stake at spot value; excludes slippage/fees)",
            ["wallet", "free (TAO)", "stake value (TAO)", "total value (TAO)", "coldkey"],
            table_rows,
            rows_data,
            right_align={1, 2, 3},
            footer=footer,
        )
        if hidden:
            app_ctx.output.message(
                f"[dim]{hidden} empty wallets hidden — pass --empty to show[/dim]"
            )
        return

    resolved = app_ctx.resolve_address("coldkey_ss58", address)
    # Label the row with the name that was actually queried, not the configured
    # wallet, when a positional name (wallet, book, or multisig) was given.
    label = (
        address
        if address is not None and not wallets.is_bittensor_address(address)
        else app_ctx.wallet_name
    )
    row = app_ctx.run(lambda client: wallet_balance_row(client, label, resolved))
    app_ctx.output.detail(None, human_balance_fields(row), json_fields=row)


@app.command("overview", rich_help_panel=PANEL_INFO)
@with_globals
def wallet_overview(
    ctx: typer.Context,
    all_wallets: bool = typer.Option(False, "--all", "-a", help="Overview for every wallet."),
    netuid: Optional[int] = typer.Option(None, "--netuid", help="Filter to one subnet."),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show dust subnets and positions (spot value < τ0.001). "
        "JSON always includes every position.",
    ),
):
    """Show free TAO and per-subnet stake for a wallet (or all wallets with --all).

    Positions whose hotkey is registered on the subnet show the hotkey's UID
    there, so registrations are visible at a glance.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if all_wallets:
        targets = list_coldkeys(app_ctx.wallet_path)
        if not targets:
            app_ctx.output.error(f"no wallets found in {app_ctx.wallet_path}")
            raise typer.Exit(1)
    else:
        targets = [(app_ctx.wallet_name, app_ctx.resolve_address("coldkey_ss58", None))]
    known_names = local_address_names(app_ctx.wallet_path)

    async def _fetch(client):
        rows, valuations, lock_ctx, uids = await wallet_overview_rows(
            client, targets, netuid=netuid
        )
        unnamed = [
            s["hotkey"] for row in rows for s in row["stakes"] if s["hotkey"] not in known_names
        ]
        for locks_by_netuid, _ in lock_ctx.values():
            for lock in locks_by_netuid.values():
                if lock["hotkey"] not in known_names:
                    unnamed.append(lock["hotkey"])
        return rows, valuations, lock_ctx, uids, await chain_identity_names(client, unnamed)

    rows, valuations, lock_ctx, uids, identity_names = app_ctx.run(_fetch)
    out = app_ctx.output
    if out.json_mode:
        out.value(rows)
        return

    if not all_wallets:
        row = rows[0]
        fields = {
            "wallet": f"{row['wallet']} ({row['coldkey']})",
            "free": row["free"],
            "stake_value": f"{row['stake_value']}  (spot, excl. slippage/fees)",
        }
        if row["locked_subnets"]:
            fields["locked_value"] = (
                f"{row['locked_value']}  (conviction-locked; part of stake_value)"
            )
        out.detail(None, fields)
        out.message("")

    groups = []
    for name, ss58 in targets:
        wallet_groups = netuid_groups(
            filter_stakes(valuations[ss58].positions, netuid),
            valuations[ss58],
            known_names,
            identity_names,
            {"wallet": name} if all_wallets else None,
            uids=uids,
        )
        locks_by_netuid, availability_by_netuid = lock_ctx[ss58]
        annotate_stake_groups_with_locks(
            wallet_groups, locks_by_netuid, availability_by_netuid, known_names, identity_names
        )
        groups.extend(wallet_groups)
    shown, dust = (groups, []) if show_dust else split_dust(groups)
    total = Balance(
        sum(
            valuations[ss58].spot_value(position.stake).rao
            for _, ss58 in targets
            for position in filter_stakes(valuations[ss58].positions, netuid)
        )
    )
    records = [
        {"wallet": row["wallet"], "coldkey": row["coldkey"], **stake}
        for row in rows
        for stake in row["stakes"]
    ]
    out.stake_list(STAKE_LIST_TITLE, shown, records, total)
    if dust:
        out.message(dust_note(dust))


# The indexer behind `wallet history`. The SubQuery indexer btcli used is dead
# (404 since ~2024; btcli disabled its history command over it), so this uses
# the API that powers taomarketcap.com's coldkey transfer pages instead. It is
# unauthenticated and only indexes mainnet.
_TRANSFERS_API = "https://api.taomarketcap.com/internal/v1/transactions/transfers/"


def _fetch_transfers(owner: str, limit: int) -> list[dict[str, Any]]:
    """Page through the indexer, newest first, until ``limit`` transfers."""
    results: list[dict[str, Any]] = []
    query = urllib.parse.urlencode({"coldkey": owner, "limit": min(limit, 200)})
    url: Optional[str] = f"{_TRANSFERS_API}?{query}"
    while url and len(results) < limit:
        request = urllib.request.Request(url, headers={"Accept": "application/json"})
        with urllib.request.urlopen(request, timeout=30) as response:
            body = json.loads(response.read())
        results.extend(body.get("results", []))
        url = body.get("next")
        # The API writes its pagination links with an http:// scheme.
        if url and url.startswith("http://"):
            url = "https://" + url[len("http://") :]
    return results[:limit]


def _transfer_record(owner: str, raw: dict[str, Any]) -> dict[str, Any]:
    """Normalize one indexer row into the record shape the renderer expects."""
    sender, receiver = raw.get("from_coldkey"), raw.get("to_coldkey")
    if sender == receiver:
        direction = "self"
    elif sender == owner:
        direction = "out"
    else:
        direction = "in"
    netuid = raw.get("to_netuid") if direction == "in" else raw.get("from_netuid")
    return {
        "block_number": raw.get("block_number"),
        "extrinsic_idx": raw.get("extrinsic_idx"),
        "timestamp": raw.get("timestamp"),
        "amount_rao": raw.get("amount"),
        "netuid": netuid,
        "from": sender,
        "to": receiver,
        "direction": direction,
        "success": bool(raw.get("success")),
        "tao_price_usd": raw.get("tao_price_usd"),
    }


@app.command("history", rich_help_panel=PANEL_INFO)
@with_globals
def wallet_history(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    limit: int = typer.Option(50, "--limit", help="Max transfers to show (newest first)."),
):
    """Show recent transfers for a coldkey (TaoMarketCap indexer, mainnet only).

    Privacy note: the queried coldkey address is sent to the third-party
    TaoMarketCap indexer (api.taomarketcap.com) to look up its transfers.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        label, _ = resolve_endpoint(app_ctx.network)
    except ValueError:
        label = app_ctx.network
    if label not in ("finney", "archive"):
        app_ctx.output.error(
            f"transfer history is not indexed for network {label!r}",
            note="the indexer (taomarketcap.com) only tracks mainnet (finney)",
        )
        raise typer.Exit(1)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    try:
        raw_rows = _fetch_transfers(owner, limit)
    except (OSError, ValueError) as error:
        app_ctx.output.error(
            f"could not fetch transfer history: {error}",
            note="history comes from the taomarketcap.com indexer, which may be unavailable",
        )
        raise typer.Exit(1)

    # Teach the renderer every local name it might print: wallet coldkeys and
    # address-book contacts (counterparties render as "name (ss58)").
    for name, ss58 in list_coldkeys(app_ctx.wallet_path):
        app_ctx.output.name_address(ss58, name)
        app_ctx.output.classify_address(ss58, "coldkey")
    for entry in cfg.load_addresses():
        app_ctx.output.name_address(entry.get("address"), entry.get("name"))
        app_ctx.output.classify_address(entry.get("address"), "coldkey")

    records = [_transfer_record(owner, raw) for raw in raw_rows]
    owner_label = app_ctx.output.address_names.get(owner)
    title = f"transfer history — {f'{owner_label} ({owner})' if owner_label else owner}"
    app_ctx.output.transfer_history(title, records)


@app.command("transfer", rich_help_panel=PANEL_OPS)
@with_tx_globals
def wallet_transfer(
    ctx: typer.Context,
    dest_ss58: str = typer.Option(
        ..., address_cli_name("dest_ss58"), help=ss58_param_help("dest_ss58")
    ),
    amount_tao: str = typer.Option(
        ...,
        "--amount-tao",
        "--amount",
        help="Amount to send, in TAO. Pass `all` for the entire transferable balance.",
    ),
):
    """Transfer TAO to another coldkey.

    Signs with the configured wallet's coldkey (you may be prompted for the
    wallet password) and submits the transfer on chain. Transfers are
    irreversible once included in a block, so double-check the destination.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(amount_tao, True)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount-tao`: {error}")
        raise typer.Exit(2)
    dest = app_ctx.resolve_address("coldkey_ss58", dest_ss58)
    app_ctx.submit(Transfer(dest_ss58=dest, amount_tao=amount))


@app.command("inspect", rich_help_panel=PANEL_INFO)
@with_globals
def wallet_inspect(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    show_dust: bool = typer.Option(
        False,
        "--dust",
        help="Also show dust subnets and positions (spot value < τ0.001). "
        "JSON always includes them.",
    ),
):
    """Detailed wallet view: balance, stake, delegation, identity."""
    app_ctx: AppContext = ctx_of(ctx)
    if coldkey_ss58 is None:
        confirm_wallet(app_ctx, help_text="Wallet to inspect.")
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    known_names = local_address_names(app_ctx.wallet_path)

    async def _fetch(client):
        data, valuation = await wallet_inspect_data(client, app_ctx.wallet_name, owner)
        hotkeys = [s["hotkey"] for s in data["stakes"]] + [
            d["delegate_hotkey"] for d in data["delegations"]
        ]
        unnamed = [hk for hk in hotkeys if hk not in known_names]
        return data, valuation, await chain_identity_names(client, unnamed)

    data, valuation, identity_names = app_ctx.run(_fetch)
    out = app_ctx.output
    if out.json_mode:
        out.value(data)
        return

    out.detail(None, human_balance_fields(data["balance"]))

    out.message("")
    takes = {(d["netuid"], d["delegate_hotkey"]): d["take"] for d in data["delegations"]}
    groups = netuid_groups(valuation.positions, valuation, known_names, identity_names, takes=takes)
    shown_groups, dust_groups = (groups, []) if show_dust else split_dust(groups)
    out.stake_list(STAKE_LIST_TITLE, shown_groups, data["stakes"], valuation.stake_value)
    if dust_groups:
        out.message(dust_note(dust_groups))

    out.message("")
    if data["identity"]:
        out.detail("identity", data["identity"])
    else:
        out.message("[dim]no on-chain identity[/dim]")


_IDENTITY_HELP = {
    "name": "Public display name shown for the coldkey.",
    "url": "Website URL published with the identity.",
    "description": "Short description published with the identity.",
}


@app.command("set-identity", rich_help_panel=PANEL_IDENTITY)
@with_tx_globals
def set_identity(
    ctx: typer.Context,
    name: Optional[str] = typer.Option(None, "--name", help=_IDENTITY_HELP["name"]),
    url: Optional[str] = typer.Option(None, "--url", help=_IDENTITY_HELP["url"]),
    description: Optional[str] = typer.Option(
        None, "--description", help=_IDENTITY_HELP["description"]
    ),
):
    """Set on-chain identity for the wallet coldkey.

    The identity is public and stored on chain. Fields not passed as flags
    are prompted for interactively, keeping their current values by default;
    fields without flags (image, discord, etc.) are carried over unchanged.
    """
    app_ctx: AppContext = ctx_of(ctx)
    confirm_wallet(app_ctx, help_text="Wallet whose coldkey identity to set.")
    owner = app_ctx.wallet().coldkeypub.ss58_address
    current = app_ctx.run(lambda c: c.read("identity", coldkey_ss58=owner)) or {}

    out = app_ctx.output
    if not out.json_mode:
        if current:
            out.detail("current identity", current)
        else:
            out.message("[dim]no on-chain identity yet[/dim]")

    def current_value(field: str) -> str:
        return str(current.get(field) or "")

    fields = {"name": name, "url": url, "description": description}
    specs = [
        PromptSpec(
            field=field,
            flag=f"--{field}",
            help=_IDENTITY_HELP[field],
            parse=lambda _app_ctx, raw: raw,
            # Enter keeps the current value; the name has no fallback when
            # there is no identity yet, so it stays required.
            default=(current_value(field) or None) if field == "name" else current_value(field),
        )
        for field, value in fields.items()
        if value is None
    ]
    fill_missing(app_ctx, specs, fields)
    # Non-interactive sessions keep the current (or empty) value for omitted flags.
    for field, value in fields.items():
        if value is None:
            fields[field] = current_value(field)

    # Fields without CLI flags are carried over so re-setting the identity
    # doesn't wipe them.
    app_ctx.submit(
        SetIdentity(
            **fields,
            github_repo=current_value("github_repo"),
            image=current_value("image"),
            discord=current_value("discord"),
            additional=current_value("additional"),
        )
    )


@app.command("get-identity", rich_help_panel=PANEL_IDENTITY)
@with_globals
def get_identity(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Show on-chain identity for a coldkey."""
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    identity = app_ctx.run(lambda c: c.read("identity", coldkey_ss58=owner))
    app_ctx.output.detail("identity", identity)


@app.command("associate-hotkey", rich_help_panel=PANEL_OPS)
@with_tx_globals
def associate_hotkey(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Associate a hotkey with the wallet coldkey on chain."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    app_ctx.submit(AssociateHotkey(hotkey_ss58=hotkey))


@app.command("swap-hotkey", rich_help_panel=PANEL_SECURITY)
@with_tx_globals
def swap_hotkey(
    ctx: typer.Context,
    new_hotkey_ss58: str = typer.Option(
        ..., address_cli_name("new_hotkey_ss58"), help=ss58_param_help("new_hotkey_ss58")
    ),
    netuid: Optional[int] = typer.Option(
        None,
        "--netuid",
        help="Only swap the hotkey's registration on this subnet. "
        "Omit to swap it across all subnets.",
    ),
):
    """Swap a registered hotkey for a new one.

    Moves the old hotkey's registrations and stake to the new hotkey, signed
    by the owning coldkey. Use this to rotate a hotkey you suspect is
    compromised without touching the coldkey.
    """
    app_ctx: AppContext = ctx_of(ctx)
    new_hotkey = app_ctx.resolve_address("hotkey_ss58", new_hotkey_ss58)
    app_ctx.submit(SwapHotkey(new_hotkey_ss58=new_hotkey, netuid=netuid))


@app.command("swap-coldkey", rich_help_panel=PANEL_SECURITY)
@with_tx_globals
def swap_coldkey(
    ctx: typer.Context,
    new_coldkey_ss58: str = typer.Option(
        ..., address_cli_name("new_coldkey_ss58"), help=ss58_param_help("new_coldkey_ss58")
    ),
):
    """Execute an announced coldkey swap.

    Moves the coldkey's balance, stake, and subnet ownership to the new
    coldkey. The swap must have been announced beforehand with
    `announce-coldkey-swap` and the announcement delay elapsed. This is
    irreversible: after the swap the old coldkey no longer controls anything.
    """
    app_ctx: AppContext = ctx_of(ctx)
    new_coldkey = app_ctx.resolve_address("coldkey_ss58", new_coldkey_ss58)
    app_ctx.submit(SwapColdkeyAnnounced(new_coldkey_ss58=new_coldkey))


@app.command("swap-check", rich_help_panel=PANEL_SECURITY)
@with_globals
def swap_check(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Check pending coldkey swap announcement."""
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _status(client):
        announcement = await client.read("coldkey_swap_announcement", coldkey_ss58=owner)
        if announcement is None:
            return None, None
        return announcement, await client.block()

    announcement, current_block = app_ctx.run(_status)
    if announcement is None:
        app_ctx.output.detail("coldkey swap status", {"status": "none"})
        return

    blocks_remaining = announcement["execute_block"] - current_block
    if blocks_remaining <= 0:
        executable = "now (waiting period elapsed)"
    else:
        wait = timedelta(seconds=blocks_remaining * BLOCKTIME)
        eta = datetime.now().astimezone() + wait
        if wait >= timedelta(minutes=1):
            rounded = timedelta(minutes=round(wait.total_seconds() / 60))
        else:
            rounded = wait
        executable = f"in ~{format_duration(rounded)} ({eta:%Y-%m-%d %H:%M %Z})"

    app_ctx.output.detail(
        "coldkey swap status",
        {
            "execute_block": announcement["execute_block"],
            "current_block": current_block,
            "blocks_remaining": max(0, blocks_remaining),
            "executable": executable,
            "new_coldkey_hash": announcement["new_coldkey_hash"],
            "disputed": announcement["disputed"],
            "dispute_block": announcement["dispute_block"],
        },
    )


@app.command("announce-coldkey-swap", rich_help_panel=PANEL_SECURITY)
@with_tx_globals
def announce_coldkey_swap(
    ctx: typer.Context,
    new_coldkey_ss58: str = typer.Option(
        ..., address_cli_name("new_coldkey_ss58"), help=ss58_param_help("new_coldkey_ss58")
    ),
):
    """Announce intent to swap coldkey.

    Publishes the intended new coldkey on chain and starts the mandatory
    waiting period; the swap itself happens later via `swap-coldkey`. Check
    progress with `swap-check`.
    """
    app_ctx: AppContext = ctx_of(ctx)
    new_coldkey = app_ctx.resolve_address("coldkey_ss58", new_coldkey_ss58)
    app_ctx.submit(AnnounceColdkeySwap(new_coldkey_ss58=new_coldkey))


app.add_typer(keychain_app, name="keychain", rich_help_panel=PANEL_SECURITY)
