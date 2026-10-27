"""Chain error descriptions declared (first) by the `Multisig` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AlreadyApproved": (
        "The sender has already approved this multisig call, so a repeat approval is redundant. "
        "Check the `Multisigs` entry for the call hash: the sender's account already appears in "
        "its `approvals` list."
    ),
    "AlreadyStored": (
        "The call data supplied for storage is already stored on-chain for this multisig "
        "operation. Check whether the call bytes were previously stored for this call hash "
        "before submitting them again."
    ),
    "MaxWeightTooLow": (
        "The `max_weight` argument supplied with the final multisig approval is lower than the "
        "actual weight of the call being dispatched. Compute the call's real dispatch weight "
        "and pass a `max_weight` at least that large to `as_multi`."
    ),
    "MinimumThreshold": (
        "The multisig `threshold` argument was below 2, which these calls do not accept. Pass a "
        "threshold of 2 or more, or use `as_multi_threshold_1` for the single-approval case."
    ),
    "NoApprovalsNeeded": (
        "The multisig call does not need any more approvals; it is only waiting for final "
        "execution with the full call data. Instead of another `approve_as_multi`, submit "
        "`as_multi` with the complete call to dispatch it."
    ),
    "NoTimepoint": (
        "No timepoint was supplied but this multisig operation is already underway, so the "
        "approval cannot be matched to it. Read the operation's `when` field from the "
        "`Multisigs` entry for the call hash and pass that height and index as "
        "`maybe_timepoint`."
    ),
    "NotFound": (
        "The referenced item does not exist in storage: no multisig operation for that call "
        "hash in `Multisigs`, no scheduled task at that slot or name in `Agenda`/`Lookup`, or "
        "no matching proxy registration in `Proxies`. Verify the identifier against current "
        "chain state."
    ),
    "NotOwner": (
        "Only the account that opened the multisig operation (its depositor) may cancel it or "
        "adjust its deposit. Compare the sender against the `depositor` field stored in the "
        "`Multisigs` entry for this call hash."
    ),
    "SenderInSignatories": (
        "The multisig sender was included in the `other_signatories` list, but that list must "
        "contain only the remaining signatories. Remove the sender's own account from "
        "`other_signatories` before resubmitting."
    ),
    "SignatoriesOutOfOrder": (
        "The `other_signatories` list is not sorted in strictly ascending account order, which "
        "the multisig pallet requires for a canonical account derivation. Sort the list and "
        "remove duplicates before resubmitting."
    ),
    "TooFewSignatories": (
        "The multisig signatory list is too short: `other_signatories` must contain at least "
        "one account besides the sender. Check the length of `other_signatories` against the "
        "threshold you are using."
    ),
    "TooManySignatories": (
        "The multisig signatory list exceeds the maximum allowed. Compare the length of "
        "`other_signatories` plus the sender against the pallet's `MaxSignatories` constant."
    ),
    "UnexpectedTimepoint": (
        "A timepoint was supplied but no multisig operation is underway for this call hash; the "
        "first approval must open the operation with no timepoint. Pass `maybe_timepoint: None` "
        "on the opening call, or verify the call hash matches an existing `Multisigs` entry."
    ),
    "WrongTimepoint": (
        "The timepoint supplied does not match the one recorded when this multisig operation "
        "was opened. Read the correct `when` height and index from the `Multisigs` entry for "
        "the call hash and resubmit with that exact timepoint."
    ),
}
