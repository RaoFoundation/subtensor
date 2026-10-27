"""Chain error descriptions declared (first) by the `MevShield` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "BadEncKeyLen": (
        "The `enc_key` passed to `announce_next_key` is not the exact ML-KEM-768 encapsulation "
        "key length (1184 bytes). Check the byte length of the `enc_key` argument before "
        "announcing."
    ),
    "TooManyPendingExtrinsics": (
        "`store_encrypted` was rejected because the shield pallet's queue of encrypted "
        "extrinsics is already at capacity. Compare the `PendingExtrinsics` count against "
        "`MaxPendingExtrinsicsLimit` and wait for queued items to be processed or expire."
    ),
    "Unreachable": (
        "`announce_next_key` could not identify the current block author via the `FindAuthors` "
        "lookup, which should be impossible in a normally authored block. Check the block's "
        "author digest and the shield pallet's authorship wiring."
    ),
    "WeightExceedsAbsoluteMax": (
        "`set_on_initialize_weight` or `set_max_extrinsic_weight` was given a value above the "
        "shield pallet's hard cap of half the total block weight. Compare the `value` argument "
        "against the `MAX_ON_INITIALIZE_WEIGHT` constant."
    ),
}
