"""Interactive prompting for required options omitted on the command line.

Where btcli blindly prompts (even in scripts, one bare question at a time, no
validation), this module: prompts only on a real terminal (non-interactive
sessions get a single error listing *every* missing option); shows the option's
``--help`` text as the hint; validates each answer in place and re-asks on bad
input; and afterwards echoes the full non-interactive command so the flags are
learnable and copy-pasteable into scripts.

Two entry points feed this machinery:
- the generated ``tx`` commands build their own :class:`PromptSpec` list (they
  know about ss58 resolution, money units, and signer defaults);
- every other command is covered generically by :func:`run_app`, which drives
  the Typer app itself, catches click's ``MissingParameter``, prompts for all
  of the command's missing required params, and re-runs with them injected.
"""

from __future__ import annotations

import difflib
import functools
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Optional

import typer
from rich.console import Console
from rich.text import Text
from typer import rich_utils

try:  # typer >= 0.17 vendors click as typer._click
    from typer._click import exceptions as click_exceptions
except ImportError:  # older typer uses the external click package
    from click import (  # type: ignore[no-redef]  # ty: ignore[unresolved-import]
        exceptions as click_exceptions,
    )

from .. import wallets
from .context import AppContext
from .output import STYLE_COMMAND, STYLE_HINT

# Parses one prompted answer into the value the command body expects; raises
# ValueError (or typer.Exit, for address resolution) to trigger a re-prompt.
Parser = Callable[[AppContext, str], Any]

# A self-contained prompt flow (e.g. the stake-position picker): renders its own
# UI, writes one or more answers into the kwargs dict, and returns the argv
# tokens to include in the skip-the-prompts hint.
CustomFlow = Callable[[Console, AppContext, dict], list[str]]

# Argv tokens answered interactively this run. Prompt rounds can come from
# several layers (click's MissingParameter, the command body, the signer
# confirmation inside submit), so each round only records its answers here and
# one combined skip-the-prompts hint is printed when the command finishes.
_entered_tokens: list[str] = []


@dataclass
class PromptSpec:
    """One required option that may be prompted for: its flag, hint, and parser."""

    field: str  # intent field name (e.g. dest_ss58)
    flag: str  # CLI flag (e.g. --dest), or the bare name for positionals
    help: Optional[str]  # the option's --help text, shown as the prompt hint
    parse: Parser
    placeholder: Optional[str] = None  # short input-shape cue (e.g. "y/n")
    default: Optional[str] = None  # shown as `flag [default]:`; Enter accepts it
    custom: Optional[CustomFlow] = None  # replaces the plain ask (may fill several fields)
    positional: bool = False  # argument, not option: the hint omits the flag token


def interactive(app_ctx: AppContext) -> bool:
    """Prompting is only allowed on a real terminal, in human output mode."""
    out = app_ctx.output
    return not out.json_mode and not out.quiet and sys.stdin.isatty() and sys.stderr.isatty()


def _missing_error(app_ctx: AppContext, flags: list[str]) -> None:
    """Report every missing required option at once and exit (script-safe)."""
    joined = ", ".join(f"`{flag}`" for flag in flags)
    plural = "s" if len(flags) > 1 else ""
    app_ctx.output.error(
        f"missing required option{plural}: {joined}",
        help="pass the options explicitly, or run on a terminal to be prompted",
    )
    raise typer.Exit(2)


def _ask(console: Console, app_ctx: AppContext, spec: PromptSpec) -> tuple[Any, str]:
    """Prompt for one option until an answer parses; returns (value, raw text)."""
    if spec.help:
        hint = Text("  ")
        hint.append(spec.flag, style=STYLE_COMMAND)
        hint.append("  ")
        hint.append(spec.help, style=STYLE_HINT)
        console.print(hint)
    label = spec.flag.lstrip("-")
    if spec.placeholder:
        label += f" ({spec.placeholder})"
    prompt = Text("  ")
    prompt.append(label, style=STYLE_COMMAND)
    if spec.default is not None:
        prompt.append(f" [{spec.default}]", style=STYLE_HINT)
    prompt.append(": ", style=STYLE_COMMAND)
    while True:
        try:
            raw = console.input(prompt).strip()
        except (KeyboardInterrupt, EOFError):
            console.print()
            app_ctx.output.message("aborted.")
            raise typer.Exit(130)
        if not raw:
            if spec.default is None:
                console.print("  a value is required", style=STYLE_HINT)
                continue
            raw = spec.default
        try:
            value = spec.parse(app_ctx, raw)
        except typer.Exit:
            continue  # the resolver already printed its own error
        except ValueError as error:
            app_ctx.output.error(str(error) or f"invalid value for `{spec.flag}`")
            continue
        return value, raw


def fill_missing(app_ctx: AppContext, missing: list[PromptSpec], kwargs: dict[str, Any]) -> None:
    """Collect values for ``missing`` required options, writing them into ``kwargs``.

    On a terminal each option is prompted with its help text as a hint and its
    answer validated in place. Otherwise (piped stdin, ``--json``, ``--quiet``)
    every missing flag is reported at once and the command exits — a script is
    never left hanging on an invisible prompt.
    """
    if not interactive(app_ctx):
        # Options with a default keep it (the AppContext already carries it);
        # only options with no fallback are an error here.
        hard = [spec for spec in missing if spec.default is None]
        if hard:
            _missing_error(app_ctx, [spec.flag for spec in hard])
        return

    console = Console(stderr=True, highlight=False)
    console.print()
    for spec in missing:
        if kwargs.get(spec.field) is not None:
            continue  # an earlier custom flow already answered this one
        if spec.custom is not None:
            _entered_tokens.extend(spec.custom(console, app_ctx, kwargs))
            console.print()
            continue
        kwargs[spec.field], raw = _ask(console, app_ctx, spec)
        _entered_tokens.extend([raw] if spec.positional else [spec.flag, raw])
        console.print()


def _unknown_name_error(kind: str, raw: str, known: list[str]) -> ValueError:
    """A short 'no such name' error: full listing when small, suggestions when not."""
    if len(known) <= 8:
        available = ", ".join(sorted(known)) or "none"
        return ValueError(f"no {kind} named {raw!r} (available: {available})")
    matches = difflib.get_close_matches(raw, known, n=3, cutoff=0.5)
    if matches:
        return ValueError(f"no {kind} named {raw!r} — did you mean {', '.join(matches)}?")
    return ValueError(f"no {kind} named {raw!r} — `btcli wallet list` shows all {len(known)}")


def _parse_wallet(app_ctx: AppContext, raw: str, *, require_coldkey: bool = True) -> str:
    """Validate the wallet exists on disk and select it on the context.

    A bad name re-prompts with the wallets that *are* available. The coldkey is
    only demanded when it is the signing key (hotkey-signed intents may use a
    coldkey-less wallet dir).
    """
    known = wallets.list_wallets(app_ctx.wallet_path)
    if raw not in known:
        raise _unknown_name_error("wallet", raw, sorted(known))
    try:
        address = wallets.open_wallet(
            raw, app_ctx.hotkey_name, app_ctx.wallet_path
        ).coldkeypub.ss58_address
    except Exception as error:
        if require_coldkey:
            raise ValueError(f"wallet {raw!r} has no usable coldkey: {error}")
        address = None
    app_ctx.wallet_name = raw
    app_ctx.wallet_given = True  # explicitly confirmed; don't ask again this run
    if address:
        app_ctx.output.name_address(address, raw)
    return raw


def _parse_wallet_name(app_ctx: AppContext, raw: str) -> str:
    """Select a wallet by name without requiring it to exist yet (create/regen flows)."""
    app_ctx.wallet_name = raw
    app_ctx.wallet_given = True
    return raw


def _parse_hotkey_name(app_ctx: AppContext, raw: str) -> str:
    """Select a hotkey by name without requiring it to exist yet (create/regen flows)."""
    app_ctx.hotkey_name = raw
    app_ctx.hotkey_given = True
    return raw


def _parse_hotkey(app_ctx: AppContext, raw: str) -> str:
    """Validate the hotkey exists in the selected wallet and select it on the context."""
    known = wallets.list_wallets(app_ctx.wallet_path).get(app_ctx.wallet_name, [])
    if raw not in known:
        raise _unknown_name_error(f"hotkey in wallet {app_ctx.wallet_name!r}", raw, sorted(known))
    try:
        handle = wallets.open_wallet(app_ctx.wallet_name, raw, app_ctx.wallet_path)
        address = handle.hotkey.ss58_address
    except Exception as error:
        raise ValueError(f"cannot open hotkey {raw!r} in wallet {app_ctx.wallet_name!r}: {error}")
    app_ctx.hotkey_name = raw
    app_ctx.hotkey_given = True
    app_ctx.output.name_address(address, f"{app_ctx.wallet_name}/{raw}")
    return raw


def confirm_wallet(
    app_ctx: AppContext,
    *,
    help_text: str = "Wallet this command targets.",
    require_coldkey: bool = True,
    must_exist: bool = True,
    hotkey_help: Optional[str] = None,
    hotkey_must_exist: bool = True,
) -> None:
    """Confirm the target wallet (and optionally hotkey) when only defaulted.

    Wallet-scoped commands call this so a bare invocation doesn't silently act
    on the configured wallet: on a terminal the wallet is prompted for with the
    configured name as the Enter-to-accept default (same flow as the tx signer
    confirmation); non-interactive sessions keep the default silently. Passing
    ``--wallet``/``-w`` (or ``BT_WALLET``), ``--yes``, or the extension signer
    skips the prompt. ``must_exist=False`` accepts any name (create/regen
    flows, multisig names).

    Hotkey-scoped commands additionally pass ``hotkey_help`` so the hotkey
    name is confirmed in the same round (skipped by ``--wallet-hotkey``/``-H``
    or ``BT_WALLET_HOTKEY``); ``hotkey_must_exist=False`` accepts any name.
    """
    skip = app_ctx.assume_yes or app_ctx.uses_external_signer()
    specs: list[PromptSpec] = []
    if not app_ctx.wallet_given and not skip:
        parse: Parser = (
            functools.partial(_parse_wallet, require_coldkey=require_coldkey)
            if must_exist
            else _parse_wallet_name
        )
        specs.append(
            PromptSpec(
                field="wallet",
                flag="--wallet",
                help=help_text,
                parse=parse,
                default=app_ctx.wallet_name,
            )
        )
    if hotkey_help is not None and not app_ctx.hotkey_given and not skip:
        specs.append(
            PromptSpec(
                field="wallet_hotkey",
                flag="--wallet-hotkey",
                help=hotkey_help,
                parse=_parse_hotkey if hotkey_must_exist else _parse_hotkey_name,
                default=app_ctx.hotkey_name,
            )
        )
    if specs:
        fill_missing(app_ctx, specs, {})
    # Confirmed (or silently kept in a non-interactive session) — later
    # default fallbacks in the same invocation must not ask again.
    app_ctx.wallet_given = True
    if hotkey_help is not None:
        app_ctx.hotkey_given = True


def signer_specs(
    app_ctx: AppContext, signer: str, *, wallet_given: bool, hotkey_given: bool
) -> list[PromptSpec]:
    """Prompts for the signing key: the wallet (and hotkey, for hotkey-signed
    intents) is always confirmed interactively, with the configured name as the
    Enter-to-accept default. Passing ``--wallet`` / ``--wallet-hotkey`` skips it."""
    specs: list[PromptSpec] = []
    if not wallet_given:
        wallet_help = (
            "Wallet containing the signing hotkey."
            if signer == "hotkey"
            else "Wallet whose coldkey signs this transaction."
        )
        specs.append(
            PromptSpec(
                field="wallet",
                flag="--wallet",
                help=wallet_help,
                parse=functools.partial(_parse_wallet, require_coldkey=signer == "coldkey"),
                default=app_ctx.wallet_name,
            )
        )
    if signer == "hotkey" and not hotkey_given:
        specs.append(
            PromptSpec(
                field="wallet_hotkey",
                flag="--wallet-hotkey",
                help="Hotkey (within the wallet) that signs this transaction.",
                parse=_parse_hotkey,
                default=app_ctx.hotkey_name,
            )
        )
    return specs


def _flush_command_hint(exit_code: int) -> None:
    """Echo the equivalent non-interactive command so the flags are learnable.

    Printed once, when the command finishes successfully, combining the answers
    from every prompt round of the run (command params, signer confirmation...).
    """
    if exit_code != 0 or not _entered_tokens:
        return
    tokens = [Path(sys.argv[0]).name, *sys.argv[1:], *_entered_tokens]
    _entered_tokens.clear()
    command = " ".join(shlex.quote(part) for part in tokens)
    line = Text(overflow="ignore", no_wrap=True)
    line.append("hint:".rjust(7), style=STYLE_HINT)
    line.append(" skip the prompts next time: ", style=STYLE_HINT)
    line.append(command, style=STYLE_COMMAND)
    console = Console(stderr=True, highlight=False)
    console.print()
    console.print(line, soft_wrap=True)


# --- Generic coverage: drive the whole app and intercept MissingParameter ----


def _click_flag(param: Any) -> str:
    """The display/injection flag for a click parameter (options keep their
    long flag; positional arguments show as their name)."""
    if param.param_type_name == "option":
        return next((o for o in param.opts if o.startswith("--")), param.opts[0])
    return param.name.replace("_", "-")


def _click_placeholder(param: Any) -> Optional[str]:
    """Input-shape cue from the click type (choices, integer, number...)."""
    type_name = getattr(param.type, "name", "")
    if type_name == "choice":
        return "|".join(param.type.choices)
    if type_name in ("integer", "float", "boolean"):
        return {"integer": "integer", "float": "number", "boolean": "y/n"}[type_name]
    return None


def _click_parse(param: Any, _app_ctx: AppContext, raw: str) -> str:
    """Validate a prompted answer with the param's own click type; the raw
    string is what gets injected into argv (click converts it again there)."""
    try:
        param.type.convert(raw, param, None)
    except click_exceptions.UsageError as error:
        raise ValueError(error.message)
    return raw


def _supplied(param: Any, args: list[str]) -> bool:
    """Whether an option was passed on the command line (``--flag`` or ``--flag=x``)."""
    given = {token.split("=", 1)[0] for token in args}
    return bool(given.intersection(param.opts) or given.intersection(param.secondary_opts))


def _signing_wallet_spec(
    command: Any, app_ctx: AppContext, args: list[str]
) -> Optional[PromptSpec]:
    """The wallet-confirmation spec for the generic prompt round, or None.

    Commands that use the local wallet (the tx and unlock tiers, see
    globals.py) confirm it *before* their own missing params, so prompt order
    always narrows down — whose keys first, then what to do with them — the
    same order signer_specs gives the generated tx commands. The answer is
    injected into argv as ``--wallet``, which marks the wallet as given on the
    re-run so the submit-time confirmation doesn't ask again.
    """
    callback = getattr(command, "callback", None)
    tier = getattr(callback, "__btcli_tier__", None)
    if tier not in ("tx", "unlock") or not getattr(callback, "__btcli_wallet_signs__", True):
        return None
    if app_ctx.wallet_given or app_ctx.assume_yes or app_ctx.uses_external_signer():
        return None
    given = {token.split("=", 1)[0] for token in args}
    if given & {"--wallet", "-w", "--yes", "-y", "--signer", "--ledger"}:
        return None
    return PromptSpec(
        field="wallet",
        flag="--wallet",
        help=(
            "Wallet that signs this transaction."
            if tier == "tx"
            else "Wallet this command targets."
        ),
        parse=functools.partial(_parse_wallet, require_coldkey=False),
        default=app_ctx.wallet_name,
    )


def _missing_click_params(error: Any, args: list[str]) -> list[Any]:
    """Every required param the command line omits, in declaration order, so one
    round of prompts covers them all (instead of one error per re-run).

    Options are checked against argv directly. For positional arguments only the
    failing one and those after it can be known missing (positionals fill in
    order, so the first gap implies the rest are empty too).
    """
    failing_is_argument = error.param.param_type_name == "argument"
    after_failing = False
    missing = []
    for param in error.ctx.command.params:
        if param is error.param:
            missing.append(param)
            after_failing = True
        elif not param.required:
            continue
        elif param.param_type_name == "option":
            if not _supplied(param, args):
                missing.append(param)
        elif failing_is_argument and after_failing:
            missing.append(param)
    return missing


def run_app(app: typer.Typer) -> None:
    """Run the Typer app, prompting for any missing required parameters.

    Drives the app in non-standalone mode so click's ``MissingParameter`` is
    interceptable for *every* command — generated or hand-written — without
    per-command wiring: the command's missing required params are prompted for
    (help text as the hint, answers validated by the param's own click type)
    and the invocation is retried with the values injected. All other
    exceptions reproduce typer's standalone rendering and exit codes.
    """
    try:
        _run_app(app)
    except click_exceptions.Exit as error:
        # typer.Exit raised by our own prompt helpers (abort, missing-in-script).
        _flush_command_hint(error.exit_code)
        sys.exit(error.exit_code)


def _run_app(app: typer.Typer) -> None:
    args = list(sys.argv[1:])
    for _ in range(10):  # one re-parse per prompt round; bounded for safety
        try:
            result = app(args=args, standalone_mode=False)
        except click_exceptions.MissingParameter as error:
            obj = getattr(error.ctx, "obj", None)
            app_ctx = obj if isinstance(obj, AppContext) else None
            if app_ctx is None or error.param is None:
                rich_utils.rich_format_error(error)
                sys.exit(error.exit_code)
            # Per-command --json/--quiet overrides are applied by the command
            # body, which never ran; honor them from the raw argv.
            if "--json" in args:
                app_ctx.output.json_mode = True
            if "--quiet" in args or "-q" in args:
                app_ctx.output.quiet = True
            missing = _missing_click_params(error, args)
            specs = [
                PromptSpec(
                    field=param.name,
                    flag=_click_flag(param),
                    help=getattr(param, "help", None),
                    parse=functools.partial(_click_parse, param),
                    placeholder=_click_placeholder(param),
                    positional=param.param_type_name != "option",
                )
                for param in missing
            ]
            if not interactive(app_ctx):
                _missing_error(app_ctx, [spec.flag for spec in specs])
            # Wallet-using commands confirm the signing wallet before their own
            # params (has a default, so it is never part of the missing error).
            wallet_spec = _signing_wallet_spec(error.ctx.command, app_ctx, args)
            if wallet_spec is not None:
                specs.insert(0, wallet_spec)
            console = Console(stderr=True, highlight=False)
            console.print()
            for spec in specs:
                _, raw = _ask(console, app_ctx, spec)
                entered = [raw] if spec.positional else [spec.flag, raw]
                args += entered
                _entered_tokens.extend(entered)
                console.print()
            continue
        except click_exceptions.ClickException as error:
            # Mirrors typer's standalone handling: NoArgsIsHelpError prints the
            # help itself; everything else gets the rich usage-error box.
            if error.__class__.__name__ == "NoArgsIsHelpError":
                error.show()
            else:
                rich_utils.rich_format_error(error)
            sys.exit(error.exit_code)
        except click_exceptions.Abort:
            rich_utils.rich_abort_error()
            sys.exit(1)
        # In non-standalone mode typer returns ``Exit``'s code instead of raising.
        code = result if isinstance(result, int) else 0
        _flush_command_hint(code)
        sys.exit(code)
    raise AssertionError("prompt loop did not converge")
