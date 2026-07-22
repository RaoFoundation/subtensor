"""Global options, shared across commands but scoped to what each one can use.

The root callback sets defaults (from flags/env), so ``btcli -n test <cmd>``
still works. In addition, commands carry the same options as *overrides*
(default None/False, no env) so they also show in that command's ``--help`` and
can be passed after the subcommand: ``btcli <cmd> -n test``. When a command's
override is set, it wins over the root default.

A command only advertises the options it can act on, in tiers:

- ``read`` (every command): network, wallet identity, output mode. Reads need
  the wallet too — ``*_ss58`` params default to the configured wallet's keys.
- ``unlock``: + coldkey password sources, for commands that unlock local keys
  without submitting (``wallet sign``, ``wallet unlock``, ...).
- ``extension``: + the bridge-page options, for the ``extension`` commands.
- ``tx``: everything — mutations additionally get ``--yes``, ``--dry-run``, and
  the signer-backend/extension options.

Both the generated commands (tx/query) and the hand-written ones share this one
definition, so the global surface can't drift between them.
"""

from __future__ import annotations

import functools
import inspect
from typing import Any, Callable, Literal, Optional

import typer

from .logs import setup_logging

Tier = Literal["read", "unlock", "extension", "tx"]

# Help-panel titles: globals render apart from the command's own options,
# grouped by what they configure. Typer shows one boxed panel per title.
PANEL_NETWORK_WALLET = "Global: network & wallet"
PANEL_OUTPUT = "Global: output"
PANEL_EXECUTION = "Global: execution"
PANEL_UNLOCK = "Global: coldkey unlock"
PANEL_EXTENSION = "Global: extension signing"

# (param_name, annotation, typer.Option) — override form: None/False defaults, no env.
# Relevant to every command, including reads.
_COMMON = [
    (
        "network",
        Optional[str],
        typer.Option(
            None,
            "--network",
            "-n",
            help="Network name (finney/test/local) or ws:// endpoint.",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "fallback_endpoints",
        Optional[str],
        typer.Option(
            None,
            "--fallback-endpoints",
            help="Comma-separated ws:// endpoints tried when the primary is unreachable "
            "('none' to disable).",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "archive_endpoints",
        Optional[str],
        typer.Option(
            None,
            "--archive-endpoints",
            help="Comma-separated archive ws:// endpoints for reads older than the "
            "primary retains ('none' to disable).",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "retry_forever",
        bool,
        typer.Option(
            False,
            "--retry-forever",
            help="Never give up on connection failures; keep cycling the endpoint pool.",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "wallet",
        Optional[str],
        typer.Option(
            None,
            "--wallet",
            "-w",
            help="Coldkey wallet name.",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "wallet_hotkey",
        Optional[str],
        typer.Option(
            None,
            "--wallet-hotkey",
            "-H",
            help="Hotkey name within the wallet.",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "wallet_path",
        Optional[str],
        typer.Option(
            None,
            "--wallet-path",
            help="Wallet directory.",
            rich_help_panel=PANEL_NETWORK_WALLET,
        ),
    ),
    (
        "json_output",
        bool,
        typer.Option(
            False,
            "--json",
            help="Machine-readable JSON output.",
            rich_help_panel=PANEL_OUTPUT,
        ),
    ),
    (
        "quiet",
        bool,
        typer.Option(
            False,
            "--quiet",
            "-q",
            help="Suppress informational output.",
            rich_help_panel=PANEL_OUTPUT,
        ),
    ),
    (
        "verbosity",
        int,
        typer.Option(
            0,
            "--verbose",
            "-v",
            count=True,
            help="Diagnostic logging on stderr: -v connection lifecycle, "
            "-vv full debug, -vvv raw websocket frames.",
            rich_help_panel=PANEL_OUTPUT,
        ),
    ),
]

# The mutation flow: confirmation and previews.
_TX_FLOW = [
    (
        "assume_yes",
        bool,
        typer.Option(
            False,
            "--yes",
            "-y",
            help="Skip confirmation prompts.",
            rich_help_panel=PANEL_EXECUTION,
        ),
    ),
    (
        "dry_run",
        bool,
        typer.Option(
            False,
            "--dry-run",
            help="Preview mutations without submitting.",
            rich_help_panel=PANEL_EXECUTION,
        ),
    ),
    (
        "mev_shield",
        Optional[bool],
        typer.Option(
            None,
            "--mev-shield/--no-mev-shield",
            help="Encrypt the extrinsic via the MevShield pallet so the mempool can't "
            "front-run it. Stake-trading commands shield by default; pass "
            "--no-mev-shield to submit in the clear, or persist the choice with "
            "`btcli config set mev_shield false`.",
            rich_help_panel=PANEL_EXECUTION,
        ),
    ),
]

# Password sources for unlocking encrypted local coldkeys.
_UNLOCK = [
    (
        "wallet_password_file",
        Optional[str],
        typer.Option(
            None,
            "--wallet-password-file",
            envvar="BT_WALLET_PASSWORD_FILE",
            help="File containing the coldkey password (one line).",
            rich_help_panel=PANEL_UNLOCK,
        ),
    ),
    (
        "macos_password",
        bool,
        typer.Option(
            False,
            "--macos-password",
            help="Unlock encrypted coldkeys via a native macOS password dialog.",
            rich_help_panel=PANEL_UNLOCK,
        ),
    ),
    (
        "keychain_password",
        bool,
        typer.Option(
            False,
            "--keychain-password",
            help="Unlock encrypted coldkeys from the macOS Keychain (see wallet keychain save).",
            rich_help_panel=PANEL_UNLOCK,
        ),
    ),
]

# Choosing and filtering the signing backend (mutations only).
_SIGNER = [
    (
        "signer_backend",
        Optional[str],
        typer.Option(
            None,
            "--signer",
            help="Signing backend: wallet (default), extension, ledger, or vault "
            "(Polkadot Vault via QR).",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
    (
        "ledger",
        bool,
        typer.Option(
            False,
            "--ledger",
            help="Sign on a Ledger device (Polkadot generic app); shorthand for --signer ledger.",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
    (
        "ledger_account",
        Optional[int],
        typer.Option(
            None,
            "--ledger-account",
            help="Ledger derivation account (m/44'/354'/ACCOUNT'/0'/index'). Default 0.",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
    (
        "ledger_index",
        Optional[int],
        typer.Option(
            None,
            "--ledger-index",
            help="Ledger derivation address index (m/44'/354'/account'/0'/INDEX'). Default 0.",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
    (
        "signer_address",
        Optional[str],
        typer.Option(
            None,
            "--signer-address",
            envvar="BT_SIGNER_ADDRESS",
            help="External signer account ss58 address (extension: prompts when omitted; "
            "vault: falls back to the wallet's coldkeypub).",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
    (
        "extension_source",
        Optional[str],
        typer.Option(
            None,
            "--extension-source",
            help="Filter extension accounts by source (e.g. talisman, polkadot-js).",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
]

# Reaching the extension bridge page (mutations and the `extension` commands).
_BRIDGE = [
    (
        "extension_browser",
        Optional[str],
        typer.Option(
            None,
            "--extension-browser",
            envvar="BT_EXTENSION_BROWSER",
            help="Browser for the bridge page: firefox, chrome, or an app name.",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
    (
        "extension_bridge_url",
        Optional[str],
        typer.Option(
            None,
            "--extension-bridge",
            envvar="BT_EXTENSION_BRIDGE",
            help="Extension bridge WebSocket URL.",
            rich_help_panel=PANEL_EXTENSION,
        ),
    ),
]

_TIERS: dict[str, list] = {
    "read": _COMMON,
    "unlock": _COMMON + _UNLOCK,
    "extension": _COMMON + _BRIDGE,
    "tx": _COMMON + _TX_FLOW + _UNLOCK + _SIGNER + _BRIDGE,
}


def parameters(tier: Tier = "tx") -> list[inspect.Parameter]:
    """The tier's global options as keyword-only signature parameters (for generated commands)."""
    return [
        inspect.Parameter(name, inspect.Parameter.KEYWORD_ONLY, default=option, annotation=ann)
        for name, ann, option in _TIERS[tier]
    ]


def apply(ctx: typer.Context, kwargs: dict[str, Any]) -> None:
    """Pop the global options out of ``kwargs`` and override the AppContext where set.

    Every key is popped defensively — a command only carries its tier's subset.
    """
    obj = ctx.obj
    if v := kwargs.pop("network", None):
        obj.network = v
    if v := kwargs.pop("fallback_endpoints", None):
        obj.fallback_endpoints = v
    if v := kwargs.pop("archive_endpoints", None):
        obj.archive_endpoints = v
    if kwargs.pop("retry_forever", False):
        obj.retry_forever = True
    if v := kwargs.pop("wallet", None):
        obj.wallet_name = v
        obj.wallet_given = True
    if v := kwargs.pop("wallet_hotkey", None):
        obj.hotkey_name = v
        obj.hotkey_given = True
    if v := kwargs.pop("wallet_path", None):
        obj.wallet_path = v
    if kwargs.pop("json_output", False):
        obj.output.json_mode = True
    if kwargs.pop("assume_yes", False):
        obj.assume_yes = True
    if kwargs.pop("dry_run", False):
        obj.dry_run = True
    if (v := kwargs.pop("mev_shield", None)) is not None:
        obj.mev_shield = v
    quiet_override = kwargs.pop("quiet", False)
    if quiet_override:
        obj.output.quiet = True
    verbosity_override = kwargs.pop("verbosity", 0)
    if verbosity_override:
        obj.verbosity = verbosity_override
    if quiet_override or verbosity_override:
        setup_logging(verbosity=obj.verbosity, quiet=obj.output.quiet)
    if v := kwargs.pop("wallet_password_file", None):
        obj.wallet_password_file = v
    if kwargs.pop("macos_password", False):
        obj.macos_password = True
    if kwargs.pop("keychain_password", False):
        obj.keychain_password = True
    if v := kwargs.pop("signer_backend", None):
        obj.signer_backend = v
    if kwargs.pop("ledger", False):
        obj.signer_backend = "ledger"
    if (v := kwargs.pop("ledger_account", None)) is not None:
        obj.ledger_account = v
    if (v := kwargs.pop("ledger_index", None)) is not None:
        obj.ledger_index = v
    if v := kwargs.pop("signer_address", None):
        obj.signer_address = v
    if v := kwargs.pop("extension_source", None):
        obj.extension_source = v
    if v := kwargs.pop("extension_browser", None):
        obj.extension_browser = v
    if v := kwargs.pop("extension_bridge_url", None):
        obj.extension_bridge_url = v


def _with_tier(tier: Tier) -> Callable[[Callable], Callable]:
    """Decorator factory: add the tier's global options to a hand-written command.

    The wrapped command keeps its own options; the globals are appended and merged
    into the AppContext before the command body runs.
    """
    specs = _TIERS[tier]

    def decorate(fn: Callable) -> Callable:
        original = inspect.signature(fn)

        @functools.wraps(fn)
        def wrapper(*args, **kwargs):
            ctx = kwargs.get("ctx") or (args[0] if args else None)
            apply(ctx, kwargs)
            return fn(*args, **kwargs)

        wrapper.__signature__ = original.replace(
            parameters=list(original.parameters.values()) + parameters(tier)
        )
        annotations = dict(getattr(fn, "__annotations__", {}))
        for name, ann, _ in specs:
            annotations[name] = ann
        wrapper.__annotations__ = annotations
        return wrapper

    return decorate


# Read-only commands: no confirmation, unlocking, or signing options.
with_globals = _with_tier("read")
# Commands that unlock local coldkeys without submitting (wallet sign/unlock/...).
with_unlock_globals = _with_tier("unlock")
# The `extension` command group: bridge-page options only.
with_extension_globals = _with_tier("extension")
# Mutations: the full surface, including --yes/--dry-run and the signer backend.
with_tx_globals = _with_tier("tx")
