"""Resolve address-book / wallet names inside raw call parameters.

``btcli call --args`` and the intent specs nested inside ``btcli tx`` options
(a multisig inner ``--call``, batch children) take call parameters as JSON,
where the flag-level name resolution never runs. A user who writes an
address-book name ("izzi") where the chain expects an AccountId would only
see the codec's cryptic "Base 58 requirement is violated".

This module closes that gap through the same canonical lookup the flags use —
address book, proxy book, saved multisigs, and local wallet keys — plus a
did-you-mean error for anything unresolvable. Account-typed values are
recognized two ways:

- generated call builders (``bittensor.calls``) annotate scalar account
  params as ``AccountId32`` / ``MultiAddress``; known signatory-list fields
  are resolved explicitly because structural ``Vec<AccountId32>`` annotations
  currently degrade to ``Any`` in codegen;
- intent specs resolve their ``*_ss58`` args and signatory lists.

Both paths recurse through nested call structures: sudo-wrapped and batch
call dicts resolve against their own builders' annotations, multisig inner
calls and batch children resolve as intent specs.
"""

from __future__ import annotations

import difflib
from typing import Any, Optional

import typer

from .. import config as cfg
from .. import wallets
from .._generated import calls
from ..wallets import is_bittensor_address

# Scalar annotations on generated call builders that mean "an account".
_ACCOUNT_ANNOTATIONS = {"AccountId32", "MultiAddress"}

# The nested-call dict shapes accepted inside raw params: the generated Call
# tuple's field names, and the substrate-conventional call_* keys.
_CALL_SPEC_KEYS = (
    ("module", "function", "params"),
    ("call_module", "call_function", "call_args"),
)

# Intent fields that hold a list of accounts without the *_ss58 suffix.
_SIGNATORY_FIELDS = ("other_signatories", "signatories")


# --- raw builder params (`btcli call --args`) ---------------------------------------------


def resolve_builder_params(
    app_ctx, target: str, params: dict, *, param_hint: str = "--args"
) -> dict:
    """Resolve names in raw ``Pallet.function`` params, recursively.

    Unknown targets pass through untouched — composing against live metadata
    reports those with the chain's own error.
    """
    builder = _builder(target)
    annotations = getattr(builder, "__annotations__", {}) if builder is not None else {}
    return {
        name: _resolve_value(app_ctx, name, annotations.get(name), value, param_hint)
        for name, value in params.items()
    }


def _builder(target: str):
    pallet_name, _, function = target.partition(".")
    pallet = getattr(calls, pallet_name, None)
    builder = getattr(pallet, function, None) if isinstance(pallet, type) else None
    return builder if callable(builder) else None


def _resolve_value(app_ctx, param: str, annotation: Optional[str], value: Any, hint: str) -> Any:
    if isinstance(value, str) and annotation in _ACCOUNT_ANNOTATIONS:
        return _resolve_account(app_ctx, param, value, hint)
    if _is_nested_call(value):
        return _resolve_nested_call(app_ctx, value, hint)
    if isinstance(value, list) and value:
        if all(_is_nested_call(item) for item in value):
            return [_resolve_nested_call(app_ctx, item, hint) for item in value]
        if param in _SIGNATORY_FIELDS and all(isinstance(item, str) for item in value):
            return [_resolve_account(app_ctx, param, item, hint) for item in value]
    return value


def _is_nested_call(value: Any) -> bool:
    return _call_spec_keys(value) is not None or _variant_call(value) is not None


def _call_spec_keys(value: Any) -> Optional[tuple[str, str, str]]:
    """The (module, function, params) key names when ``value`` is a keyed call dict."""
    if not isinstance(value, dict):
        return None
    for keys in _CALL_SPEC_KEYS:
        if keys[0] in value and keys[1] in value:
            return keys
    return None


def _variant_call(value: Any) -> Optional[tuple[str, str, dict]]:
    """(module, function, args) when ``value`` is a variant-shaped nested call —
    ``{"Balances": {"transfer_keep_alive": {...}}}``, the shape the codec
    encodes a ``RuntimeCall`` param from. Requiring a known generated builder
    keeps single-key data dicts (e.g. a MultiAddress ``{"Id": ...}``) out.
    """
    if not (isinstance(value, dict) and len(value) == 1):
        return None
    module, inner = next(iter(value.items()))
    if not (isinstance(inner, dict) and len(inner) == 1):
        return None
    function, args = next(iter(inner.items()))
    if not isinstance(args, dict) or _builder(f"{module}.{function}") is None:
        return None
    return module, function, args


def _resolve_nested_call(app_ctx, value: dict, hint: str) -> dict:
    keys = _call_spec_keys(value)
    if keys:
        module_key, function_key, params_key = keys
        inner = value.get(params_key)
        if not isinstance(inner, (dict, type(None))):
            return value
        target = f"{value[module_key]}.{value[function_key]}"
        resolved = dict(value)
        resolved[params_key] = resolve_builder_params(app_ctx, target, inner or {}, param_hint=hint)
        return resolved
    module, function, args = _variant_call(value)
    return {
        module: {
            function: resolve_builder_params(app_ctx, f"{module}.{function}", args, param_hint=hint)
        }
    }


# --- intent specs (`btcli tx` args, multisig inner calls, batch children) ------------------


def resolve_intent_args(app_ctx, args: dict, *, param_hint: Optional[str] = None) -> dict:
    """Resolve names in an intent's args: ``*_ss58`` strings, signatory lists,
    and nested intent specs (a multisig inner ``call``, batch ``intents``)."""
    out = dict(args)
    for name, value in args.items():
        hint = param_hint or "--" + name.replace("_", "-")
        if name.endswith("_ss58") and isinstance(value, str):
            out[name] = _resolve_account(app_ctx, name, value, hint)
        elif name in _SIGNATORY_FIELDS and isinstance(value, list):
            out[name] = [
                _resolve_account(app_ctx, name, item, hint) if isinstance(item, str) else item
                for item in value
            ]
        elif _is_intent_spec(value):
            out[name] = _resolve_intent_spec(app_ctx, value, hint)
        elif isinstance(value, list):
            out[name] = [
                _resolve_intent_spec(app_ctx, item, hint) if _is_intent_spec(item) else item
                for item in value
            ]
    return out


def _is_intent_spec(value: Any) -> bool:
    return isinstance(value, dict) and "op" in value


def _resolve_intent_spec(app_ctx, spec: dict, hint: str) -> dict:
    args = {name: value for name, value in spec.items() if name != "op"}
    return {**spec, **resolve_intent_args(app_ctx, args, param_hint=hint)}


# --- the shared account resolver ------------------------------------------------------------


def _resolve_account(app_ctx, param: str, value: str, hint: str) -> str:
    """An ss58 address for ``value``: as-is when valid, otherwise looked up in
    the address book, proxy book, or local wallets — or a did-you-mean error."""
    if is_bittensor_address(value):
        return value
    found = _lookup(app_ctx, param, value)
    if found is None:
        raise typer.BadParameter(_unresolved(app_ctx, param, value), param_hint=hint)
    address, source = found
    app_ctx.output.name_address(address, value)
    app_ctx.output.classify_address(address, "hotkey" if "hotkey" in param else "coldkey")
    app_ctx.output.message(f"[dim]{param}: resolved {source} to {address}[/dim]")
    return address


def _lookup(app_ctx, param: str, name: str) -> Optional[tuple[str, str]]:
    """(address, source description) for a known name, or None.

    Uses the same canonical lookup as ordinary CLI flags, including saved
    multisig names for coldkey parameters.
    """
    try:
        resolved = app_ctx.resolve_address_ref(param, name)
    except Exception:
        return None
    return resolved.address, resolved.source


def _unresolved(app_ctx, param: str, value: str) -> str:
    base = f"{param}: {value!r} is not a valid ss58 address"
    suggestion = _closest_name(app_ctx, value)
    if suggestion:
        name, address, source = suggestion
        return f"{base} — did you mean {source} {name!r} ({address})?"
    return f"{base} and matches no address-book, proxy-book, or wallet name"


def _closest_name(app_ctx, value: str) -> Optional[tuple[str, str, str]]:
    """The known name closest to ``value`` as (name, address, source), or None."""
    candidates: dict[str, tuple[str, str]] = {}
    for entry in cfg.load_addresses():
        name, address = entry.get("name"), entry.get("address")
        if name and isinstance(address, str):
            candidates.setdefault(name, (address, "address-book entry"))
    for entry in cfg.load_proxies():
        name, address = entry.get("name"), entry.get("address")
        if name and isinstance(address, str):
            candidates.setdefault(name, (address, "proxy-book entry"))
    try:
        for info in wallets.list_wallets_detailed(app_ctx.wallet_path):
            if info.ss58:
                candidates.setdefault(info.name, (info.ss58, "wallet"))
            for hk in info.hotkeys:
                if hk.ss58:
                    candidates.setdefault(f"{info.name}/{hk.name}", (hk.ss58, "hotkey"))
    except OSError:
        pass  # unreadable wallet dir; suggestions are cosmetic
    close = difflib.get_close_matches(value, candidates, n=1, cutoff=0.6)
    if not close:
        return None
    name = close[0]
    address, source = candidates[name]
    return name, address, source
