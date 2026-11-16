"""Submission defaults for calls that establish protected account identities."""

from __future__ import annotations

import warnings
from typing import Any

from ._policy import evaluate_policy
from .fee_filters import _arg_value, _call_dict

ACCOUNT_REGISTRATION_CALLS = frozenset(
    tuple(pair) for pair in evaluate_policy("protected.registration_rules", {})
)


async def contains_account_registration(substrate, call: Any) -> bool:
    """Recognize registration in builders, decoded calls and composed SCALE calls."""
    decoded = _call_dict(call)
    if decoded is None and isinstance(call, dict) and len(call) == 1:
        # RuntimeCall enum input: {"Utility": {"batch_all": {"calls": [...]}}}.
        module, variants = next(iter(call.items()))
        if isinstance(variants, dict) and len(variants) == 1:
            function, params = next(iter(variants.items()))
            if isinstance(params, dict):
                decoded = {
                    "call_module": module,
                    "call_function": function,
                    "call_args": [{"name": k, "value": v} for k, v in params.items()],
                }
    if decoded is None:
        data = getattr(call, "data", call)
        if not isinstance(data, (bytes, bytearray, str)):
            return False
        decoded = await substrate.decode_scale("Call", data)
    node = evaluate_policy(
        "protected.registration_node",
        {
            "pallet": decoded["call_module"],
            "call": decoded["call_function"],
            "expanded": True,
            "registrations": sorted(ACCOUNT_REGISTRATION_CALLS),
        },
    )
    if node["found"]:
        return True
    children = []
    for field in node["fields"]:
        value = _arg_value(decoded, field)
        children.extend((value or []) if field == "calls" else [value])
    for child in children:
        if child is not None and await contains_account_registration(substrate, child):
            return True
    return False


async def registration_shield_default(substrate, call: Any) -> bool:
    """Use Shield for registration when available; report a default-only fallback."""
    if not await contains_account_registration(substrate, call):
        return False
    if await substrate.mev_next_key() is not None:
        return True
    warnings.warn(
        "MEV Shield is not active on this network; submitting account registration "
        "unshielded. Pass shielded=True to require encrypted submission.",
        UserWarning,
        stacklevel=3,
    )
    return False
