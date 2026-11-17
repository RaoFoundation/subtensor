"""Adapt Python intent values for the shared saved-call binding policy."""

from __future__ import annotations

from ._pinned import pinned_inputs
from ._policy import evaluate_policy
from .intents._shared import policy_arguments
from .intents.registry import build as build_intent


async def _intent_spec(substrate, intent, depth=0):
    from .executor import _coerce_addresses
    from .intents.batch import Batch
    from .intents.children import SetChildren
    from .intents.proxy import ExecuteProxyAnnounced
    from .receiving import account_for_read, coerce_payment_address
    from .signing import is_address_param

    if depth > 32:
        raise ValueError("saved call intent nesting exceeds 32")
    intent = await _coerce_addresses(substrate, intent)
    args = policy_arguments(intent)
    for name, value in args.items():
        if is_address_param(name):
            args[name] = await account_for_read(substrate, value)
    if args.get("service_hotkey_ss58") is not None:
        from .intents.gamma import _service_spender

        args["spender_ss58"] = await _service_spender(
            substrate, args["netuid"], args["service_hotkey_ss58"]
        )
        args.pop("service_hotkey_ss58")
    if isinstance(intent, Batch):
        args["intents"] = [
            await _intent_spec(substrate, child, depth + 1) for child in intent._children
        ]
    if isinstance(args.get("call"), dict) and "op" in args["call"]:
        child = args["call"]
        args["call"] = await _intent_spec(substrate, build_intent(child["op"], child), depth + 1)
    if isinstance(intent, ExecuteProxyAnnounced):
        args["inner"] = await _intent_spec(
            substrate, build_intent(intent.inner_op, intent.inner_args), depth + 1
        )
        # The normalized child above is the semantic input; raw arguments may
        # contain address objects that are not JSON values.
        args.pop("inner_args", None)
    if isinstance(intent, SetChildren):
        args["children"] = [
            [
                proportion,
                await account_for_read(
                    substrate,
                    await coerce_payment_address(substrate, address, "hotkey_ss58"),
                ),
            ]
            for proportion, address in intent.children
        ]
    return {"op": intent.op, "args": args, "origin": intent.origin}


async def bind_saved_intent(substrate, intent, origin, inspected, data, *, wrapper):
    """Verify the declared operation while retaining approved dynamic inputs.

    Rust handles operations it can reconstruct without reading current state.
    Other operations use their existing builder with pinned derived inputs; the
    shared policy then compares the complete independently reconstructed call.
    """
    from .executor import _compose_nested_intent_call
    from .intents.proxy import _announced_decoded_call

    binding = {
        "intent": await _intent_spec(substrate, intent),
        "call": inspected,
        "dispatch_origin": origin.ss58_address,
    }
    if evaluate_policy("transaction.bind_saved_call", binding)["verified"]:
        return {}
    with pinned_inputs(substrate, data, decoded=inspected):
        prepared, extras = await _compose_nested_intent_call(
            substrate, intent, origin, wrapper=wrapper
        )
    binding["prepared"] = evaluate_policy(
        "protected.decoded_call",
        {"call": await _announced_decoded_call(substrate, prepared)},
    )
    if not evaluate_policy("transaction.bind_saved_call", binding)["verified"]:
        raise ValueError("saved call bytes could not be bound to the declared semantic intent")
    return extras
