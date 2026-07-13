"""Hyperparameter views: the listing with normalized readings, and the
one-parameter explainer.

Raw chain values stay primary (they are what `sudo set` writes and what JSON
carries); the human reading (``≈ 0.5``, ``= τ0.7``) rides alongside as a dim
annotation. The one-parameter view adds the explanation and a ready-to-paste
set command for owner-settable parameters.
"""

from __future__ import annotations

from typing import Any, Optional

import typer

from .. import hyperparams as hp
from ..intents.hyperparameters import OWNER_HYPERPARAMETERS
from .context import AppContext


def show_hyperparameters(
    app_ctx: AppContext,
    netuid: int,
    params: Any,
    name: Optional[str],
    hint: Optional[str] = None,
) -> None:
    """Render the full listing, or one parameter in depth when ``name`` is given.

    ``hint`` overrides the listing's default help line (the `sudo set` prompt
    flow reuses the listing with its own guidance).
    """
    params = _params_by_name(params)
    if name is None:
        _listing(app_ctx, netuid, params, hint)
        return
    if name not in params:
        app_ctx.output.error(
            f"no hyperparameter named {name!r} on netuid {netuid}",
            help="run without --name to list them all",
        )
        raise typer.Exit(2)
    _single(app_ctx, netuid, name, params[name])


def _listing(
    app_ctx: AppContext, netuid: int, params: dict[str, Any], hint: Optional[str] = None
) -> None:
    rows = [(key, str(value), hp.annotate(key, value)) for key, value in params.items()]
    example = "kappa" if "kappa" in params else next(iter(params), "tempo")
    app_ctx.output.hyperparameters(
        f"hyperparameters netuid {netuid}",
        rows,
        params,
        hint=hint or f"`--name {example}` explains one parameter and how to set it",
    )


def _single(app_ctx: AppContext, netuid: int, name: str, raw: Any) -> None:
    kind = hp.kind_of(name)
    fraction = hp.normalized(name, raw) if isinstance(raw, int) else None
    annotation = hp.annotate(name, raw)

    fields: dict[str, Any] = {"value": raw}
    if fraction is not None:
        fields["normalized"] = f"{fraction:.6g}"
    elif annotation:
        # rao amounts, block durations, u64::MAX — reuse the listing reading.
        fields["reading"] = annotation.lstrip("=≈ ")

    settable = name in OWNER_HYPERPARAMETERS
    if settable:
        example = _example_value(kind, fraction, raw)
        help_text = f"set it: `btcli sudo set --netuid {netuid} --name {name} --value {example}`"
        note = f"--value takes {hp.value_forms(name)}"
    else:
        help_text = None
        note = "not settable by the subnet owner (root/governance only)"

    app_ctx.output.hyperparameter(
        f"{name}  netuid {netuid}",
        fields,
        hp.doc_of(name),
        {
            "netuid": netuid,
            "name": name,
            "value": raw,
            "normalized": fraction,
            "kind": kind,
            "doc": hp.doc_of(name),
            "owner_settable": settable,
        },
        help=help_text,
        note=note,
    )


def _params_by_name(params: Any) -> dict[str, Any]:
    if params is None:
        return {}
    if isinstance(params, dict):
        return params
    if not isinstance(params, list):
        raise TypeError("subnet hyperparameters must be a V3 list or dict")
    out: dict[str, Any] = {}
    for entry in params:
        if not isinstance(entry, dict):
            continue
        name = entry.get("name")
        if isinstance(name, (bytes, bytearray)):
            name = bytes(name).decode("utf8")
        if not isinstance(name, str):
            continue
        out[name] = _v3_value(entry.get("value"))
    return out


def _v3_value(value: Any) -> Any:
    if not isinstance(value, dict) or len(value) != 1:
        return value
    payload = next(iter(value.values()))
    if isinstance(payload, dict) and set(payload) == {"bits"}:
        return payload["bits"]
    return payload


def _example_value(kind: str, fraction: Optional[float], raw: Any) -> str:
    """The current value in its most natural input form, for the set hint."""
    if fraction is not None:
        return _with_decimal_point(f"{fraction:.4g}")
    if kind == "rao" and isinstance(raw, int):
        return _with_decimal_point(f"{raw / hp.RAO_PER_TAO:g}")
    if kind == "bool":
        return "true" if raw else "false"
    return str(raw)


def _with_decimal_point(text: str) -> str:
    """Keep the decimal point that marks a value as the human form (`to_raw`
    reads a bare integer as the raw on-chain value)."""
    return text if ("." in text or "e" in text) else f"{text}.0"
