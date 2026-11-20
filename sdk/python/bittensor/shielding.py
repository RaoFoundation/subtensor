"""Submission defaults for calls that establish protected account identities."""

from __future__ import annotations

import warnings
from typing import Any

from .fee_filters import _arg_value, _call_dict

ACCOUNT_REGISTRATION_CALLS = frozenset(
    {
        ("HashedAccounts", "register"),
        ("HashedAccounts", "register_derived"),
        ("HashedAccounts", "register_derived_sponsored"),
        ("Proxy", "create_protected_pure"),
        ("Utility", "as_protected_derivative"),
    }
)

# Only recurse through RuntimeCall arguments, never arbitrary descriptor/data fields.
_SINGLE_CALL_WRAPPERS = frozenset(
    {
        ("Proxy", "proxy"),
        ("Proxy", "proxy_announced"),
        ("Multisig", "as_multi"),
        ("Multisig", "as_multi_safe"),
        ("Multisig", "as_multi_threshold_1"),
        ("Multisig", "as_multi_threshold_1_safe"),
        ("Utility", "as_derivative"),
        ("Utility", "as_safe_derivative"),
        ("Utility", "as_protected_derivative"),
        ("Utility", "dispatch_as"),
        ("Utility", "dispatch_as_fallible"),
        ("Utility", "with_weight"),
        ("Sudo", "sudo"),
        ("Sudo", "sudo_unchecked_weight"),
        ("Sudo", "sudo_as"),
    }
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
    pair = (decoded["call_module"], decoded["call_function"])
    if pair in ACCOUNT_REGISTRATION_CALLS:
        return True
    children = []
    if pair in _SINGLE_CALL_WRAPPERS:
        children = [_arg_value(decoded, "call")]
    elif pair == ("Utility", "if_else"):
        children = [_arg_value(decoded, "main"), _arg_value(decoded, "fallback")]
    elif pair[0] == "Utility" and pair[1] in {"batch", "batch_all", "force_batch"}:
        children = _arg_value(decoded, "calls") or []
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
