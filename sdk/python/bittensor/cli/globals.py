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
import re
from typing import Any, Callable, Iterable, Literal, Optional

import typer

from ..intents.proxy import ProxyTypeChoice
from .logs import setup_logging
from .multisig_helpers import SIGNATORY_BACKENDS

Tier = Literal["read", "unlock", "extension", "tx"]

# Separators inside one --signatory value (same grammar as --signatories).
_SIGNATORY_SEP = re.compile(r"[,\s]+")

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
            "--no-prompt",  # the v9 btcli spelling, kept as an alias
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
    (
        "proxy_for",
        Optional[str],
        typer.Option(
            None,
            "--proxy-for",
            help="Dispatch as this account (ss58, proxy-book/address-book name, or "
            "local wallet name) via Proxy.proxy; your wallet key signs as its "
            "registered proxy. Pass `self` to bypass a configured default.",
            rich_help_panel=PANEL_EXECUTION,
        ),
    ),
    (
        "force_proxy_type",
        Optional[ProxyTypeChoice],
        typer.Option(
            None,
            "--force-proxy-type",
            help="Require this exact proxy type to be used (with --proxy-for).",
            rich_help_panel=PANEL_EXECUTION,
        ),
    ),
    (
        "signatory_wallet",
        Optional[list[str]],
        typer.Option(
            None,
            "--signatory",
            help="When -w names a saved multisig: which member signs this approval "
            "(local wallet, address-book name, or ss58). Give several — repeated "
            "flags or one quoted comma/space list ('LOCALX VAULT') — to collect "
            "that many approvals in one run, in order. NAME=vault forces the "
            "backend for one member.",
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
            help="External signer account (ss58 or address-book name). When -w is a "
            "multisig, --signatory already names the member — this flag is not "
            "needed. Extension: prompts when omitted. Vault: falls back to the "
            "wallet's coldkeypub.",
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


def apply(
    ctx: typer.Context,
    kwargs: dict[str, Any],
    *,
    skip: Optional[Iterable[str]] = None,
) -> None:
    """Pop the global options out of ``kwargs`` and override the AppContext where set.

    Every key is popped defensively — a command only carries its tier's subset.
    ``skip`` leaves those names in ``kwargs`` so a command that already owns
    the flag (e.g. ``ExecuteProxyAnnounced.force_proxy_type``) keeps it.
    """
    reserved = set(skip or ())
    obj = ctx.obj

    def _pop(name, default=None):
        if name in reserved:
            return default
        return kwargs.pop(name, default)

    if v := _pop("network"):
        obj.network = v
    if v := _pop("fallback_endpoints"):
        obj.fallback_endpoints = v
    if v := _pop("archive_endpoints"):
        obj.archive_endpoints = v
    if _pop("retry_forever", False):
        obj.retry_forever = True
    if v := _pop("wallet"):
        obj.wallet_name = v
        obj.wallet_given = True
    if v := _pop("wallet_hotkey"):
        obj.hotkey_name = v
        obj.hotkey_given = True
    if v := _pop("wallet_path"):
        obj.wallet_path = v
    if _pop("json_output", False):
        obj.output.json_mode = True
    if _pop("assume_yes", False):
        obj.assume_yes = True
    if _pop("dry_run", False):
        obj.dry_run = True
    if (v := _pop("mev_shield")) is not None:
        obj.mev_shield = v
    if (v := _pop("proxy_for")) is not None:
        obj.proxy_for = v
    if (v := _pop("force_proxy_type")) is not None:
        obj.force_proxy_type = str(getattr(v, "value", v))
    if v := _pop("signatory_wallet"):
        # Repeated flags and comma/space-separated lists both work; ss58
        # addresses and book names never contain the separators.
        refs = [part for item in v for part in _SIGNATORY_SEP.split(item.strip()) if part]
        obj.signatory_wallets = refs
        obj.signatory_wallet = refs[0] if refs else None
        if len(refs) == 1:
            # A single NAME=backend value never reaches the rounds planner
            # (which parses the suffix itself), so split it here: the name
            # becomes the signatory, the suffix the signing backend.
            name, sep, forced = refs[0].partition("=")
            forced = forced.strip().lower()
            if sep and forced not in SIGNATORY_BACKENDS:
                raise typer.BadParameter(
                    f"unknown backend {forced!r} in --signatory {refs[0]!r}; "
                    "use wallet, vault, ledger, or extension",
                    param_hint="--signatory",
                )
            if sep:
                obj.signatory_wallet = name
                obj.signatory_wallets = [name]
                if forced != "wallet":
                    obj.signer_backend = forced
    quiet_override = _pop("quiet", False)
    if quiet_override:
        obj.output.quiet = True
    verbosity_override = _pop("verbosity", 0)
    if verbosity_override:
        obj.verbosity = verbosity_override
    if quiet_override or verbosity_override:
        setup_logging(verbosity=obj.verbosity, quiet=obj.output.quiet)
    if v := _pop("wallet_password_file"):
        obj.wallet_password_file = v
    if _pop("macos_password", False):
        obj.macos_password = True
    if _pop("keychain_password", False):
        obj.keychain_password = True
    if v := _pop("signer_backend"):
        obj.signer_backend = v
    if _pop("ledger", False):
        obj.signer_backend = "ledger"
    if (v := _pop("ledger_account")) is not None:
        obj.ledger_account = v
    if (v := _pop("ledger_index")) is not None:
        obj.ledger_index = v
    if v := _pop("signer_address"):
        obj.signer_address = v
    if v := _pop("extension_source"):
        obj.extension_source = v
    if v := _pop("extension_browser"):
        obj.extension_browser = v
    if v := _pop("extension_bridge_url"):
        obj.extension_bridge_url = v


def _with_tier(tier: Tier) -> Callable[[Callable], Callable]:
    """Decorator factory: add the tier's global options to a hand-written command.

    The wrapped command keeps its own options; the globals are appended and merged
    into the AppContext before the command body runs.
    """
    specs = _TIERS[tier]

    def decorate(fn: Callable) -> Callable:
        original = inspect.signature(fn)
        owned = set(original.parameters)
        added = [p for p in parameters(tier) if p.name not in owned]

        @functools.wraps(fn)
        def wrapper(*args, **kwargs):
            ctx = kwargs.get("ctx") or (args[0] if args else None)
            apply(ctx, kwargs, skip=owned)
            return fn(*args, **kwargs)

        wrapper.__signature__ = original.replace(
            parameters=list(original.parameters.values()) + added
        )
        annotations = dict(getattr(fn, "__annotations__", {}))
        for name, ann, _ in specs:
            if name not in owned:
                annotations[name] = ann
        wrapper.__annotations__ = annotations
        # The tier marks which commands touch the local wallet (tx/unlock), so
        # the generic prompt round (prompt.py) can confirm the wallet *first*.
        wrapper.__btcli_tier__ = tier
        return wrapper

    return decorate


def evm_key_signed(fn: Callable) -> Callable:
    """Mark a tx-tier command as signing with a stored EVM key, not the wallet.

    The generic prompt round confirms the signing wallet before a tx command's
    own missing params; these commands don't sign with the wallet, so that
    confirmation would only mislead.
    """
    fn.__btcli_wallet_signs__ = False
    return fn


# Read-only commands: no confirmation, unlocking, or signing options.
with_globals = _with_tier("read")
# Commands that unlock local coldkeys without submitting (wallet sign/unlock/...).
with_unlock_globals = _with_tier("unlock")
# The `extension` command group: bridge-page options only.
with_extension_globals = _with_tier("extension")
# Mutations: the full surface, including --yes/--dry-run and the signer backend.
with_tx_globals = _with_tier("tx")
