"""Mirror of runtime ``fee_filters.rs``: hotkey-origin calls whose tx fee is
charged to the owning coldkey instead of the hotkey.

Keep in sync with ``runtime/src/fee_filters.rs`` when the runtime list changes.
"""

from __future__ import annotations

from typing import Any, Iterator

from ._policy import evaluate_policy

# (pallet, call_function) pairs whose fee is debited from the hotkey owner's coldkey.
COLDKEY_PAYS_FEE_CALLS: frozenset[tuple[str, str]] = frozenset(
    tuple(name.split(".", 1)) for name in evaluate_policy("financial.fee.constants", {})
)

COLDKEY_FEE_WARNING = (
    "transaction fee is charged to the hotkey owner's coldkey, not the signing hotkey"
)


def _params_to_args(params: dict[str, Any]) -> list[dict[str, Any]]:
    return [{"name": key, "value": value} for key, value in params.items()]


def _call_dict(call: Any) -> dict[str, Any] | None:
    if isinstance(call, dict) and call.get("call_module") and call.get("call_function"):
        return call
    module = getattr(call, "module", None)
    function = getattr(call, "function", None)
    if isinstance(module, str) and isinstance(function, str):
        params = getattr(call, "params", None)
        return {
            "call_module": module,
            "call_function": function,
            "call_args": _params_to_args(params if isinstance(params, dict) else {}),
        }
    value = getattr(call, "value", None)
    if isinstance(value, dict) and value.get("call_module") and value.get("call_function"):
        return value
    return None


def _arg_value(call_dict: dict[str, Any], name: str) -> Any:
    for arg in call_dict.get("call_args") or []:
        if isinstance(arg, dict) and arg.get("name") == name:
            return arg.get("value")
    return None


def iter_call_leaves(call: Any) -> Iterator[tuple[str, str]]:
    """Yield (pallet, call_function) for each leaf in a possibly nested call."""
    yield from (
        tuple(row)
        for row in evaluate_policy("financial.fee.leaves", {"call": _portable_call(call)})
    )


def charges_coldkey_fee(call: Any) -> bool:
    """True when any leaf call in ``call`` routes its fee to the owning coldkey."""
    return any(pair in COLDKEY_PAYS_FEE_CALLS for pair in iter_call_leaves(call))


def _portable_call(value):
    """Adapt Python call objects and codec wrappers without choosing traversal policy."""
    if isinstance(value, dict):
        return {key: _portable_call(child) for key, child in value.items()}
    if isinstance(value, (list, tuple)):
        return [_portable_call(child) for child in value]
    call = _call_dict(value)
    return (
        _portable_call(call)
        if call is not None
        else value
        if isinstance(value, (str, int, float, bool, type(None), bytes))
        else None
    )
