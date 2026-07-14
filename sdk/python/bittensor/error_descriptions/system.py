"""Chain error descriptions declared (first) by the `System` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "CallFiltered": (
        "The runtime's origin call filter (e.g. `BaseCallFilter` or a restricted origin) "
        "rejected this call before dispatch. Check whether the specific call is permitted for "
        "the origin you used, including any proxy or safe-mode filtering in effect."
    ),
    "FailedToExtractRuntimeVersion": (
        "The new runtime code passed to `set_code` did not yield a readable version: calling "
        "`Core_version` or decoding `RuntimeVersion` failed. Check that the submitted blob is a "
        "valid, complete runtime wasm and not truncated or compressed incorrectly."
    ),
    "InvalidSpecName": (
        "The new runtime's `spec_name` does not match the current runtime, so `set_code` "
        "refuses the upgrade. Check the `RuntimeVersion` embedded in the new wasm and ensure "
        "the spec name is identical to the chain's current one."
    ),
    "MultiBlockMigrationsOngoing": (
        "Runtime code replacement is blocked while a multi-block migration is still executing. "
        "Wait for the ongoing migrations to complete (check the multi-block migrations cursor) "
        "before retrying `set_code` or the authorized upgrade."
    ),
    "NonDefaultComposite": (
        "The account cannot be killed because its composite account data is not in the default "
        "state. Check the account's `System.Account` entry; all balance and data fields must be "
        "default before the account can be removed this way."
    ),
    "NonZeroRefCount": (
        "The account cannot be purged because other pallets still reference it. Check the "
        "`consumers`, `providers`, and `sufficients` counters in the account's `System.Account` "
        "record; all references must be released first."
    ),
    "NothingAuthorized": (
        "No code upgrade has been authorized, so `apply_authorized_upgrade` has nothing to "
        "apply. Check the `AuthorizedUpgrade` storage item in System; an authorization must be "
        "recorded before applying the new code."
    ),
    "SpecVersionNeedsToIncrease": (
        "The new runtime's `spec_version` is not greater than the current one, so the upgrade "
        "is rejected. Check the `RuntimeVersion` in the new wasm and bump `spec_version` above "
        "the version currently on chain."
    ),
    "Unauthorized": (
        "In System, the code passed to `apply_authorized_upgrade` does not hash to the value in "
        "`AuthorizedUpgrade`. In LimitOrders, the caller is not the order's signer, who alone "
        "may cancel it. Check the code hash against the authorization, or the sender against "
        "the order's `signer`."
    ),
}
