"""Chain error descriptions declared (first) by the `Drand` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "DrandConnectionFailure": (
        "Declared for failures reaching the drand HTTP API, but pulse fetching happens in the "
        "offchain worker, which logs errors instead of raising this. Check the node's offchain "
        "worker logs and outbound connectivity to the drand endpoints."
    ),
    "InvalidRoundNumber": (
        "A submitted drand pulse has an unacceptable round: the first pulse ever stored must "
        "have a round greater than zero, and each later pulse must be exactly `LastStoredRound` "
        "plus one. Compare the pulse round against `LastStoredRound`."
    ),
    "NoneValue": (
        "Template leftover in the drand pallet meaning a storage value was read before ever "
        "being set; no current code path raises it. If seen, inspect the drand pallet's storage "
        "items for missing initialization."
    ),
    "PulseVerificationError": (
        "BLS signature verification of a submitted drand pulse against the stored beacon "
        "configuration failed. Check the pulse's signature and round against the `BeaconConfig` "
        "storage (drand quicknet public key)."
    ),
    "StorageOverflow": (
        "Template leftover in the drand pallet for a counter increment overflowing `u32::MAX`; "
        "no current code path raises it. If seen, inspect the drand pallet's stored counters "
        "for values near the u32 limit."
    ),
    "UnverifiedPulse": (
        "Declared for drand pulses that fail validity checks, but current code raises "
        "`PulseVerificationError` for verification failures and silently skips unverified "
        "pulses. If seen on an older runtime, check the pulse signature against `BeaconConfig`."
    ),
}
