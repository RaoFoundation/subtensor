"""Chain error descriptions declared (first) by the `Utility` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "InvalidDerivedAccount": (
        "Deriving the sub-account for `as_derivative` failed to decode into a valid account id "
        "from the (caller, index) entropy. Check the `index` argument and the caller account "
        "used for derivation."
    ),
    "TooManyCalls": (
        "The batch submitted to the utility pallet contains more calls than the batched-calls "
        "limit allows. Check the length of the `calls` vector and split the work across "
        "multiple smaller `batch`, `batch_all`, or `force_batch` submissions."
    ),
}
