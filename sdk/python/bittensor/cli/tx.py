"""The `btcli tx` command group — generated from the intent registry.

Every registered intent becomes a subcommand whose options are derived from the
intent's dataclass fields (name, type, required-ness). There is no hand-written
command per operation: adding an intent to the SDK adds its CLI command for free,
and the two can't drift. The command body builds the intent from the parsed
options and runs it through the plan/confirm/execute choke point.

This is the CLI analogue of the tool manifest — both are projections of the same
single source of truth (the intents), one for humans, one for agents.
"""

from __future__ import annotations

import functools
import inspect
import json
from dataclasses import MISSING, fields
from decimal import Decimal, InvalidOperation
from typing import Any, Optional

import typer

from ..intents import REGISTRY
from ..intents.base import Intent, is_money
from ..intents.proxy import ProxyTypeChoice
from . import globals as g
from .context import AppContext, address_cli_name, ctx_of, ss58_param_help
from .prompt import PromptSpec, fill_missing, interactive, signer_specs
from .stake_picker import STAKE_SOURCE_FIELDS, stake_source_spec

# Field annotation (as a string, under PEP 563) -> the Python type Typer should
# parse the option as. List/complex fields are taken as strings and parsed in the
# command body (see `_coerce`).
_SCALAR_TYPES = {"str": str, "int": int, "float": float, "bool": bool}


def _base_annotation(annotation: str) -> str:
    a = annotation.strip()
    if a.startswith("Optional[") and a.endswith("]"):
        a = a[len("Optional[") : -1].strip()
    if a.endswith("| None"):
        a = a[: -len("| None")].strip()
    return a


def _is_list(annotation: str) -> bool:
    return _base_annotation(annotation) in ("list",) or _base_annotation(annotation).startswith(
        "list["
    )


def _is_dict(annotation: str) -> bool:
    return _base_annotation(annotation) == "dict" or _base_annotation(annotation).startswith(
        "dict["
    )


def _option_type(annotation: str) -> type:
    """Typer parses scalars natively; everything else (lists, dicts) comes in as a string."""
    return _SCALAR_TYPES.get(_base_annotation(annotation), str)


def _coerce(field_annotation: str, value: Any) -> Any:
    """Parse a raw CLI value into what the intent field expects.

    List fields arrive as strings: JSON when they start with '[' (needed for
    lists of pairs like set_children), otherwise comma-separated scalars. Dict
    fields (e.g. a multisig inner call) arrive as a JSON string.
    """
    if value is None:
        return value
    if _is_dict(field_annotation):
        return json.loads(value) if isinstance(value, str) else value
    if not _is_list(field_annotation):
        return value
    text = str(value).strip()
    if text.startswith("["):
        return json.loads(text)
    inner = _base_annotation(field_annotation)
    cast = int if inner == "list[int]" else float if inner == "list[float]" else str
    return [cast(part.strip()) for part in text.split(",") if part.strip()]


_TRUE = {"y", "yes", "true", "1"}
_FALSE = {"n", "no", "false", "0"}


def _parse_money(raw: Any, allow_all: bool) -> str:
    """Validate a money value, returning it as a string so the amount stays exact
    (the intent normalizes it to rao; a float here would lose precision)."""
    text = str(raw).strip()
    if text.lower() == "all":
        if allow_all:
            return "all"
        raise ValueError(f"expected a number, got {raw!r}")
    try:
        value = Decimal(text)
    except InvalidOperation:
        expected = "a number or 'all'" if allow_all else "a number"
        raise ValueError(f"expected {expected}, got {raw!r}")
    if not value.is_finite() or value < 0:
        raise ValueError(f"expected a finite non-negative number, got {raw!r}")
    return text


def _parse_prompted(
    field_name: str, annotation: str, allow_all: bool, app_ctx: AppContext, raw: str
) -> Any:
    """Validate one prompted answer; raise ValueError to re-prompt.

    Address fields resolve through the same path as the flag form (ss58,
    address-book name, or local wallet name). Lists/dicts are validated now but
    stored raw — the command body re-parses them via ``_coerce`` like any flag.
    """
    if field_name.endswith("_ss58"):
        return app_ctx.resolve_address(field_name, raw)
    if is_money(annotation):
        return _parse_money(raw, allow_all)
    base = _base_annotation(annotation)
    if base == "bool":
        lowered = raw.lower()
        if lowered in _TRUE:
            return True
        if lowered in _FALSE:
            return False
        raise ValueError(f"expected y or n, got {raw!r}")
    if base == "int":
        try:
            return int(raw)
        except ValueError:
            raise ValueError(f"expected an integer, got {raw!r}")
    if base == "float":
        try:
            return float(raw)
        except ValueError:
            raise ValueError(f"expected a number, got {raw!r}")
    if _is_list(annotation) or _is_dict(annotation):
        _coerce(annotation, raw)
        return raw
    return raw


def _placeholder(annotation: str) -> Optional[str]:
    """Short input-shape cue shown next to the prompt label."""
    base = _base_annotation(annotation)
    if is_money(annotation):
        return "number"
    if base == "bool":
        return "y/n"
    if _is_dict(annotation):
        return "JSON"
    if _is_list(annotation):
        return "comma-separated"
    return None


def _list_note(annotation: str) -> Optional[str]:
    """Input-shape cue for typed list options (e.g. ``list[int]`` netuids).

    Bare ``list`` fields (pairs like set_children) document their own JSON
    shape in the field help, so they carry no generic note.
    """
    if _base_annotation(annotation).startswith("list["):
        return "Comma-separated values (e.g. 1,2) or a JSON list."
    return None


def _unit_help(field_name: str) -> Optional[str]:
    """Spell out the unit for money-typed intent fields (``*_tao``/``*_alpha``/``*_rao``)."""
    if field_name.endswith("_tao"):
        return "Amount in TAO."
    if field_name.endswith("_alpha"):
        return (
            "Amount in the subnet's alpha "
            "(the origin subnet for cross-subnet moves; TAO if that netuid is 0)."
        )
    if field_name.endswith("_rao"):
        return "Amount in rao (1 TAO = 1e9 rao)."
    return None


def _privilege_note(intent_cls: type[Intent]) -> Optional[str]:
    """The "Requires:" doc line for privileged intents (root / subnet owner)."""
    if intent_cls.origin == "root":
        return (
            "Requires: the chain sudo key. The call is submitted wrapped in "
            "Sudo.sudo; if root is a multisig, use the multisig flow "
            "(btcli multisig) to dispatch the built call."
        )
    if intent_cls.origin == "subnet_owner":
        return "Requires: the subnet's owner coldkey."
    return None


def _command_doc(intent_cls: type[Intent]) -> str:
    """The intent's docstring plus generated privilege / verify footers, so the
    required key and the post-submission check are visible on --help."""
    parts = [intent_cls.describe()]
    note = _privilege_note(intent_cls)
    if note:
        parts.append(note)
    if intent_cls.verify:
        parts.append(f"Verify afterwards: btcli query {intent_cls.verify.replace('_', '-')}")
    return "\n\n".join(part for part in parts if part)


def _make_command(intent_cls: type[Intent]):
    """Build a Typer command callback whose signature mirrors the intent's fields."""
    specs = list(fields(intent_cls))
    intent_names = {f.name for f in specs}
    # Global --proxy-for / --force-proxy-type wrap any intent in Proxy.proxy. Skip
    # them when the intent already owns a field with the same name (e.g.
    # ExecuteProxyAnnounced.force_proxy_type is the chain param, not the wrapper).
    global_proxy_type = "force_proxy_type" not in intent_names

    def command(ctx: typer.Context, **kwargs: Any) -> None:
        g.apply(ctx, kwargs)
        app_ctx = ctx_of(ctx)
        missing = [spec for spec in prompt_specs if kwargs.get(spec.field) is None]
        # Unstake-style ops pick their source hotkey from the coldkey's live
        # stake positions instead of a bare text prompt (or, worse, a silent
        # fallback to the wallet's own hotkey, which rarely holds the stake).
        source = STAKE_SOURCE_FIELDS.get(intent_cls.op)
        if (
            source is not None
            and kwargs.get(source[0]) is None
            and not app_ctx.assume_yes
            and not app_ctx.uses_extension_signer()
            and interactive(app_ctx)
        ):
            hotkey_field, netuid_field = source
            missing = [spec for spec in missing if spec.field != hotkey_field]
            missing.insert(0, stake_source_spec(hotkey_field, netuid_field))
        # The signing wallet is confirmed too (Enter accepts the configured
        # default); --yes and the extension signer keep the flag-only flow.
        if not app_ctx.assume_yes and not app_ctx.uses_extension_signer():
            missing = (
                signer_specs(
                    app_ctx,
                    intent_cls.signer,
                    wallet_given=app_ctx.wallet_given,
                    hotkey_given=app_ctx.hotkey_given,
                )
                + missing
            )
        if missing:
            fill_missing(app_ctx, missing, kwargs)
        # `self` is a sentinel (bypass the configured proxy_for default, see
        # AppContext.submit), so it must reach submit unresolved.
        raw_proxy_for = kwargs.pop("proxy_for", None)
        proxy_for = (
            raw_proxy_for
            if raw_proxy_for in (None, "self")
            else app_ctx.resolve_address("proxy_for", raw_proxy_for)
        )
        force_proxy_type = kwargs.pop("force_proxy_type", None) if global_proxy_type else None
        if force_proxy_type is not None:
            force_proxy_type = str(getattr(force_proxy_type, "value", force_proxy_type))
        for f in specs:
            if f.name.endswith("_ss58"):
                kwargs[f.name] = app_ctx.resolve_address(f.name, kwargs.get(f.name))
            elif is_money(str(f.type)) and kwargs.get(f.name) is not None:
                # Money options parse as strings so the amount stays exact (and
                # the flag form accepts `all` where allowed); bad values are
                # rejected here.
                try:
                    kwargs[f.name] = _parse_money(
                        kwargs[f.name], f.name in intent_cls.all_amount_fields
                    )
                except ValueError as error:
                    app_ctx.output.error(
                        f"invalid value for `--{f.name.replace('_', '-')}`: {error}"
                    )
                    raise typer.Exit(2)
        args = {
            f.name: _coerce(str(f.type), kwargs[f.name])
            for f in specs
            if kwargs.get(f.name) is not None
        }
        app_ctx.submit(
            intent_cls.from_args(args), proxy_for=proxy_for, force_proxy_type=force_proxy_type
        )

    # Synthesize the signature Typer introspects: ctx first, then one keyword
    # option per intent field, typed and defaulted from the dataclass.
    params = [
        inspect.Parameter("ctx", inspect.Parameter.POSITIONAL_OR_KEYWORD, annotation=typer.Context)
    ]
    annotations: dict[str, Any] = {"ctx": typer.Context}
    prompt_specs: list[PromptSpec] = []
    for f in specs:
        # The canonical own-key params may be omitted: they fall back to the
        # configured wallet (see AppContext.resolve_address).
        wallet_defaulted = f.name in ("hotkey_ss58", "coldkey_ss58")
        required = f.default is MISSING and f.default_factory is MISSING and not wallet_defaulted
        cli_name = (
            address_cli_name(f.name)
            if f.name.endswith("_ss58") or "_ss58" in f.name
            else "--" + f.name.replace("_", "-")
        )
        allows_all = f.name in intent_cls.all_amount_fields
        # The field's declared help (metadata["help"], shared with the JSON
        # schema) says what the parameter *means*; the suffix heuristics add
        # what shapes it *accepts* (ss58/name resolution, money units).
        input_note = (
            ss58_param_help(f.name)
            if f.name.endswith("_ss58")
            else _unit_help(f.name) or _list_note(str(f.type))
        )
        declared = intent_cls.field_help(f.name)
        help_text = " ".join(part for part in (declared, input_note) if part) or None
        if allows_all:
            all_note = "Pass `all` for the entire available amount."
            help_text = f"{help_text} {all_note}" if help_text else all_note
        if required:
            # Required options parse as optional so a missing one reaches the
            # command body, which prompts for it interactively (see prompt.py).
            prompt_specs.append(
                PromptSpec(
                    field=f.name,
                    flag=cli_name,
                    help=help_text,
                    parse=functools.partial(_parse_prompted, f.name, str(f.type), allows_all),
                    placeholder="number or all" if allows_all else _placeholder(str(f.type)),
                )
            )
            # "(required)" not "[required]": rich help parses square brackets as markup.
            help_text = f"{help_text} (required)" if help_text else "(required)"
        default = None if required or wallet_defaulted else f.default
        # Bool options get a negated twin (--flag/--no-flag) so default-True
        # fields (e.g. keep_alive) can be turned off.
        flag_name = (
            f"{cli_name}/--no-{cli_name[2:]}"
            if _base_annotation(str(f.type)) == "bool"
            else cli_name
        )
        option = typer.Option(default, flag_name, help=help_text)
        # Money options parse as strings so amounts stay exact and the `all`
        # sentinel survives typer; the command body validates them (_parse_money).
        opt_type = str if is_money(str(f.type)) else _option_type(str(f.type))
        annotations[f.name] = Optional[opt_type]
        params.append(
            inspect.Parameter(
                f.name,
                inspect.Parameter.KEYWORD_ONLY,
                default=option,
                annotation=annotations[f.name],
            )
        )

    # Proxy signing mode, available on every mutation (see Executor.plan).
    proxy_params = [
        inspect.Parameter(
            "proxy_for",
            inspect.Parameter.KEYWORD_ONLY,
            default=typer.Option(
                None,
                "--proxy-for",
                help="Dispatch as this account (ss58, proxy-book/address-book name, or "
                "local wallet name) via Proxy.proxy; your wallet key signs as its "
                "registered proxy. Pass `self` to bypass a configured default.",
                rich_help_panel=g.PANEL_EXECUTION,
            ),
            annotation=Optional[str],
        ),
        inspect.Parameter(
            "force_proxy_type",
            inspect.Parameter.KEYWORD_ONLY,
            default=typer.Option(
                None,
                "--force-proxy-type",
                help="Require this exact proxy type to be used (with --proxy-for).",
                rich_help_panel=g.PANEL_EXECUTION,
            ),
            annotation=Optional[ProxyTypeChoice],
        ),
    ]
    # Globals go first so the help panels lead with network & wallet; the proxy
    # options join the execution panel wherever they land in the signature.
    for p in g.parameters("tx") + proxy_params:
        if p.name in intent_names:
            continue
        annotations[p.name] = p.annotation
        params.append(p)
    command.__signature__ = inspect.Signature(params)
    command.__annotations__ = annotations
    # Full docstring: typer shows the first line in command listings and the
    # whole text on the command's own --help, so implications aren't lost.
    # Privileged intents also carry a generated "Requires:" footer, and intents
    # with a paired verify read a "Verify afterwards:" one.
    command.__doc__ = _command_doc(intent_cls)
    return command


def intent_command(op: str):
    """The generated command callback for one registered intent, so familiar
    operations can also be mounted under a hand-written group (e.g.
    `stake add` is the same command as `tx add-stake`)."""
    return _make_command(REGISTRY[op])


def _pallet(intent_cls: type[Intent]) -> str:
    """The pallet an intent dispatches to, from its first wrapped chain call."""
    return intent_cls.wraps[0][0] if intent_cls.wraps else "Other"


def _help_panel(intent_cls: type[Intent]) -> str:
    """The --help panel an intent lists under: pallet for plain signed intents,
    with the required privilege made explicit for the others (root intents get
    their own panel; subnet-owner intents keep the pallet with a marker)."""
    if intent_cls.origin == "root":
        return "Root (chain sudo)"
    if intent_cls.origin == "subnet_owner":
        return f"{_pallet(intent_cls)} — subnet owner"
    return _pallet(intent_cls)


def build_tx_app() -> typer.Typer:
    """Assemble the `tx` group with one generated subcommand per registered intent,
    grouped in --help by the pallet each intent dispatches to (privileged intents
    are grouped by the origin the chain demands instead)."""
    app = typer.Typer(no_args_is_help=True, help="Submit transactions (generated from intents).")
    for op, intent_cls in sorted(
        REGISTRY.items(), key=lambda item: (_help_panel(item[1]), item[0])
    ):
        app.command(op.replace("_", "-"), rich_help_panel=_help_panel(intent_cls))(
            _make_command(intent_cls)
        )
    return app
