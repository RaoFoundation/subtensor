"""Mirror of runtime ``fee_filters.rs``: hotkey-origin calls whose tx fee is
charged to the owning coldkey instead of the hotkey.

Keep in sync with ``runtime/src/fee_filters.rs`` when the runtime list changes.
"""

from __future__ import annotations

from typing import Any, Iterator

# (pallet, call_function) pairs whose fee is debited from the hotkey owner's coldkey.
COLDKEY_PAYS_FEE_CALLS: frozenset[tuple[str, str]] = frozenset(
    {
        ("SubtensorModule", "set_weights"),
        ("SubtensorModule", "set_mechanism_weights"),
        ("SubtensorModule", "batch_set_weights"),
        ("SubtensorModule", "commit_weights"),
        ("SubtensorModule", "commit_mechanism_weights"),
        ("SubtensorModule", "batch_commit_weights"),
        ("SubtensorModule", "commit_crv3_mechanism_weights"),
        ("SubtensorModule", "commit_timelocked_weights"),
        ("SubtensorModule", "commit_timelocked_mechanism_weights"),
        ("SubtensorModule", "reveal_weights"),
        ("SubtensorModule", "reveal_mechanism_weights"),
        ("SubtensorModule", "batch_reveal_weights"),
        ("SubtensorModule", "serve_axon"),
        ("SubtensorModule", "serve_axon_tls"),
        ("SubtensorModule", "serve_prometheus"),
        ("SubtensorModule", "associate_evm_key"),
        ("Commitments", "set_commitment"),
    }
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
    call_dict = _call_dict(call)
    if call_dict is None:
        return
    module = call_dict["call_module"]
    function = call_dict["call_function"]

    if module == "Proxy" and function == "proxy":
        inner = _arg_value(call_dict, "call")
        if inner is not None:
            yield from iter_call_leaves(inner)
        return

    if module == "Utility" and function in {"batch_all", "batch", "force_batch"}:
        for inner in _arg_value(call_dict, "calls") or []:
            yield from iter_call_leaves(inner)
        return

    if module == "Multisig" and function in {"as_multi", "as_multi_threshold_1"}:
        inner = _arg_value(call_dict, "call")
        if inner is not None:
            yield from iter_call_leaves(inner)
        return

    yield module, function


def charges_coldkey_fee(call: Any) -> bool:
    """True when any leaf call in ``call`` routes its fee to the owning coldkey."""
    return any(pair in COLDKEY_PAYS_FEE_CALLS for pair in iter_call_leaves(call))
