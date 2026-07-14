"""Chain error descriptions declared (first) by the `SafeMode` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AlreadyDeposited": (
        "The account calling safe-mode `enter` or `extend` already has a safe-mode deposit on "
        "hold, so another cannot be placed. Check the account's `Deposits` entries and its "
        "balance held under the `EnterOrExtend` hold reason."
    ),
    "CannotReleaseYet": (
        "`release_deposit` was called too early: the current block must exceed the deposit's "
        "block plus `ReleaseDelay`, and safe-mode must be exited. Check the block key of the "
        "entry in `Deposits` against the `ReleaseDelay` config."
    ),
    "CurrencyError": (
        "A balance hold, release, or burn inside pallet-safe-mode failed while managing an "
        "enter/extend deposit. Check the account's free balance and existing holds under the "
        "safe-mode `EnterOrExtend` hold reason."
    ),
    "Entered": (
        "Safe-mode is currently active, so `enter` or `force_enter` cannot activate it again "
        "and `release_deposit` is blocked until it exits. Check `EnteredUntil` for the block at "
        "which safe-mode disengages."
    ),
    "Exited": (
        "Safe-mode is not currently active, so `extend`, `force_extend`, or `force_exit` have "
        "nothing to act on. Check that `EnteredUntil` contains a value before extending or "
        "exiting."
    ),
    "NoDeposit": (
        "No safe-mode deposit exists for the given account and block combination, so nothing "
        "can be released or slashed. Check the `Deposits` storage map for an entry under that "
        "account and deposit block."
    ),
    "NotConfigured": (
        "The permissionless safe-mode operation is disabled because its config option is "
        "`None`: `EnterDepositAmount` for `enter`, `ExtendDepositAmount` for `extend`, or "
        "`ReleaseDelay` for `release_deposit`. Check the runtime config or use the root-only "
        "force variants."
    ),
}
