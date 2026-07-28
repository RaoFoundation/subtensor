"""Chain error descriptions declared (first) by the `Preimage` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AlreadyNoted": (
        "The preimage for this hash has already been noted on-chain, so `note_preimage` has "
        "nothing to add. Check `RequestStatusFor` (or legacy `StatusFor`) for the hash before "
        "submitting the bytes again."
    ),
    "NotAuthorized": (
        "The caller is not permitted to manage this preimage; unnoting or unrequesting requires "
        "the pallet's manager origin or the account that originally deposited it. Check which "
        "origin noted or requested the preimage and use that origin or the configured "
        "`ManagerOrigin`."
    ),
    "NotNoted": (
        "The preimage cannot be unnoted because no preimage was ever noted for this hash. Check "
        "`RequestStatusFor` (or legacy `StatusFor`) for the hash to confirm what, if anything, "
        "is stored."
    ),
    "NotRequested": (
        "The preimage request cannot be removed because there are no outstanding requests for "
        "this hash. Check the request status for the hash in `RequestStatusFor` before calling "
        "`unrequest_preimage`."
    ),
    "Requested": (
        "The preimage cannot be unnoted while there are still outstanding requests for it. "
        "Check the request count in the hash's request status; all requests must be cleared "
        "before the preimage can be removed."
    ),
    "TooBig": (
        "The preimage exceeds the maximum size the pallet will store on-chain (4 MiB). Check "
        "the byte length of the preimage against the pallet's `MAX_SIZE` limit before noting "
        "it."
    ),
    "TooFew": (
        "The bulk preimage upgrade was requested with zero hashes, so there is nothing to do. "
        "Pass at least one hash to `ensure_updated`."
    ),
    "TooMany": (
        "A limit was exceeded: more preimage hashes than `MAX_HASH_UPGRADE_BULK_COUNT` were "
        "passed to `ensure_updated`, or the account has hit its proxy or announcement cap. "
        "Check the account's `Proxies` and `Announcements` entries against `MaxProxies` and "
        "`MaxPending`."
    ),
}
