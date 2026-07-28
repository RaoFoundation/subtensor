"""Chain error descriptions declared (first) by the `Commitments` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AccountNotAllowedCommit": (
        "Raised by `set_commitment` when the runtime commit check fails: the subnet must exist "
        "and the signing hotkey must be registered on it. Verify the `netuid` and that the "
        "hotkey has a UID on that subnet."
    ),
    "SpaceLimitExceeded": (
        "The commitment would push the account's byte quota for the current epoch over the cap; "
        "each `set_commitment` consumes at least 100 bytes. Check `UsedSpaceOf` for the netuid "
        "and account against `MaxSpace`, or wait for the next epoch to reset usage."
    ),
    "TooManyFieldsInCommitmentInfo": (
        "The `CommitmentInfo` passed to `set_commitment` contains more entries in `fields` than "
        "the pallet's `MaxFields` config allows. Count the fields in the `info` argument and "
        "trim to the `MaxFields` limit."
    ),
    "UnexpectedUnreserveLeftover": (
        "While lowering a commitment deposit, `Currency::unreserve` failed to return the full "
        "difference, leaving a leftover, which signals an internal inconsistency. Check the "
        "account's reserved balance against the deposit recorded in `CommitmentOf`."
    ),
}
