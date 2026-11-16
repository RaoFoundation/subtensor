"""Language adaptation for shared Rust intent construction and security metadata."""

from .._generated.calls import Call
from .._policy import evaluate_policy, evaluate_policy_preserving_values
from ..balance import Balance
from ._money import UNBOUNDED


def policy_arguments(intent):
    return {
        name: value.rao if isinstance(value, Balance) else value
        for name, value in vars(intent).items()
        if not name.startswith("_")
    }


def metadata(intent):
    return evaluate_policy(
        "transaction.intent_metadata",
        {"op": intent.op, "args": policy_arguments(intent), "signer": intent.signer},
    )


def spend(intent):
    value = metadata(intent)["spend_rao"]
    return (
        UNBOUNDED
        if value == "unbounded"
        else None
        if value is None
        else Balance.from_rao(int(value))
    )


async def simple_call(intent, substrate, **extra):
    descriptor = evaluate_policy_preserving_values(
        "transaction.simple_call", {"op": intent.op, "args": {**policy_arguments(intent), **extra}}
    )
    return await substrate.compose(
        Call(descriptor["pallet"], descriptor["call"], descriptor["params"])
    )
