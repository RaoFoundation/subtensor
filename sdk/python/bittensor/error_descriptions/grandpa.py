"""Chain error descriptions declared (first) by the `Grandpa` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "ChangePending": (
        "A GRANDPA authority-set change has already been signalled and is still pending, so a "
        "new change cannot be scheduled. Check the Grandpa `PendingChange` and `State` storage "
        "and wait for the pending change to be applied first."
    ),
    "DuplicateOffenceReport": (
        "The equivocation proof is valid but this offence has already been reported and "
        "recorded. Check whether an equivocation report for the same offender, session, and "
        "round was previously submitted before reporting again."
    ),
    "InvalidEquivocationProof": (
        "The submitted GRANDPA equivocation proof does not demonstrate a real double-vote: the "
        "two votes may be identical, from different rounds or set ids, or badly signed. Verify "
        "the proof contains two distinct votes by the same authority in the same round and "
        "authority set."
    ),
    "InvalidKeyOwnershipProof": (
        "The key ownership proof does not establish that the offending GRANDPA key belonged to "
        "the claimed validator at that session. Regenerate the proof via the "
        "`generate_key_ownership_proof` runtime API for the exact set id and authority id in "
        "the report."
    ),
    "PauseFailed": (
        "A GRANDPA pause was signalled while the authority set is not live, i.e. it is already "
        "paused or a pause is already pending. Check the Grandpa `State` storage before "
        "signalling a pause."
    ),
    "ResumeFailed": (
        "A GRANDPA resume was signalled while the authority set is not paused, i.e. it is live "
        "or already pending a resume. Check the Grandpa `State` storage before signalling a "
        "resume."
    ),
    "TooSoon": (
        "A forced GRANDPA authority change was signalled too soon after the previous one. Check "
        "the Grandpa `NextForced` storage and wait until the current block passes it before "
        "signalling another forced change."
    ),
}
